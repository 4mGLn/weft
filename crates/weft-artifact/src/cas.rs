use std::fmt::{self, Display, Formatter};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest, Sha256};

use crate::ArtifactStoreError;

const DEFAULT_MAX_OBJECT_BYTES: u64 = 512 * 1024 * 1024;
const HEX: &[u8; 16] = b"0123456789abcdef";
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CasDigest(String);

impl CasDigest {
    /// Parses a canonical lowercase `sha256:<64-hex>` digest.
    ///
    /// # Errors
    ///
    /// Returns [`ArtifactStoreError::InvalidDigest`] for non-canonical input.
    pub fn parse(value: impl Into<String>) -> Result<Self, ArtifactStoreError> {
        let value = value.into();
        if !is_canonical_digest(&value) {
            return Err(ArtifactStoreError::InvalidDigest(value));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn hex(&self) -> &str {
        &self.0[7..]
    }

    #[must_use]
    pub fn for_bytes(bytes: &[u8]) -> Self {
        let digest = Sha256::digest(bytes);
        let mut value = String::with_capacity(71);
        value.push_str("sha256:");
        for byte in digest {
            value.push(char::from(HEX[usize::from(byte >> 4)]));
            value.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
        Self(value)
    }
}

impl Display for CasDigest {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

fn is_canonical_digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Debug)]
pub struct FilesystemCas {
    objects: PathBuf,
    max_object_bytes: u64,
}

impl FilesystemCas {
    /// Opens a filesystem content-addressed store with a 512 MiB object limit.
    ///
    /// # Errors
    ///
    /// Returns [`ArtifactStoreError`] when the object directory cannot be created.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, ArtifactStoreError> {
        Self::with_max_object_bytes(root, DEFAULT_MAX_OBJECT_BYTES)
    }

    /// Opens a store with an explicit per-object read/write limit.
    ///
    /// # Errors
    ///
    /// Returns [`ArtifactStoreError`] when the limit is zero or the object
    /// directory cannot be created.
    pub fn with_max_object_bytes(
        root: impl AsRef<Path>,
        max_object_bytes: u64,
    ) -> Result<Self, ArtifactStoreError> {
        if max_object_bytes == 0 {
            return Err(ArtifactStoreError::ObjectTooLarge { size: 1, limit: 0 });
        }
        let root = root.as_ref();
        ensure_directory(root)?;
        let objects_directory = root.join("objects");
        ensure_directory(&objects_directory)?;
        let objects = objects_directory.join("sha256");
        ensure_directory(&objects)?;
        Ok(Self {
            objects,
            max_object_bytes,
        })
    }

    /// Stores bytes atomically without replacing an existing object.
    ///
    /// # Errors
    ///
    /// Returns [`ArtifactStoreError`] for over-limit data, filesystem failures,
    /// or corruption at an already occupied digest path.
    pub fn put(&self, bytes: &[u8]) -> Result<CasDigest, ArtifactStoreError> {
        let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if size > self.max_object_bytes {
            return Err(ArtifactStoreError::ObjectTooLarge {
                size,
                limit: self.max_object_bytes,
            });
        }
        let digest = CasDigest::for_bytes(bytes);
        let target = self.object_path(&digest);
        let parent = target.parent().ok_or_else(|| {
            ArtifactStoreError::InvalidManifest("CAS object path has no parent".to_owned())
        })?;
        ensure_directory(parent)?;

        if target.exists() {
            self.get(&digest)?;
            return Ok(digest);
        }

        let (temporary_path, mut temporary) = create_temporary(parent)?;
        let result = (|| -> Result<(), ArtifactStoreError> {
            temporary.write_all(bytes)?;
            temporary.sync_all()?;
            set_read_only(&temporary_path)?;
            match fs::hard_link(&temporary_path, &target) {
                Ok(()) => sync_directory(parent)?,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    self.get(&digest)?;
                }
                Err(error) => return Err(error.into()),
            }
            Ok(())
        })();
        drop(temporary);
        let _ = fs::remove_file(&temporary_path);
        result?;
        Ok(digest)
    }

    /// Loads an object and verifies its content digest before returning bytes.
    ///
    /// # Errors
    ///
    /// Returns [`ArtifactStoreError::ObjectMissing`] when absent,
    /// [`ArtifactStoreError::DigestMismatch`] when corrupted, or another storage
    /// error when the path cannot be read safely.
    pub fn get(&self, digest: &CasDigest) -> Result<Vec<u8>, ArtifactStoreError> {
        let path = self.object_path(digest);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(ArtifactStoreError::ObjectMissing(digest.clone()));
            }
            Err(error) => return Err(error.into()),
        };
        if !metadata.file_type().is_file() {
            return Err(ArtifactStoreError::InvalidObjectType(path));
        }
        if metadata.len() > self.max_object_bytes {
            return Err(ArtifactStoreError::ObjectTooLarge {
                size: metadata.len(),
                limit: self.max_object_bytes,
            });
        }
        let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(0));
        File::open(&path)?
            .take(self.max_object_bytes.saturating_add(1))
            .read_to_end(&mut bytes)?;
        let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if size > self.max_object_bytes {
            return Err(ArtifactStoreError::ObjectTooLarge {
                size,
                limit: self.max_object_bytes,
            });
        }
        let actual = CasDigest::for_bytes(&bytes);
        if &actual != digest {
            return Err(ArtifactStoreError::DigestMismatch {
                expected: digest.clone(),
                actual,
            });
        }
        Ok(bytes)
    }

    pub(crate) fn object_path(&self, digest: &CasDigest) -> PathBuf {
        self.objects
            .join(&digest.hex()[..2])
            .join(&digest.hex()[2..])
    }
}

fn create_temporary(parent: &Path) -> Result<(PathBuf, File), ArtifactStoreError> {
    for _ in 0..100 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(".tmp-{}-{sequence}", std::process::id()));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }
    Err(ArtifactStoreError::Io(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a unique CAS temporary file",
    )))
}

fn set_read_only(path: &Path) -> Result<(), ArtifactStoreError> {
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_readonly(true);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

fn ensure_directory(path: &Path) -> Result<(), ArtifactStoreError> {
    match fs::create_dir(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            if !fs::symlink_metadata(path)?.file_type().is_dir() {
                return Err(ArtifactStoreError::InvalidObjectType(path.to_path_buf()));
            }
        }
        Err(error) => return Err(error.into()),
    }
    sync_directory(path)?;
    if let Some(parent) = parent_directory(path) {
        sync_directory(parent)?;
    }
    Ok(())
}

pub(crate) fn parent_directory(path: &Path) -> Option<&Path> {
    path.parent().map(|parent| {
        if parent.as_os_str().is_empty() {
            Path::new(".")
        } else {
            parent
        }
    })
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), ArtifactStoreError> {
    File::open(path)?.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<(), ArtifactStoreError> {
    Ok(())
}
