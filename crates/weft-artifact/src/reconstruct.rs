use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path};

use weft_domain::{ArtifactRef, BaseState, FileMode, PathOperation};

use crate::{ArtifactStore, ArtifactStoreError};

#[derive(Debug)]
struct SnapshotEntry {
    mode: FileMode,
    content: Vec<u8>,
}

pub(crate) fn reconstruct(
    store: &ArtifactStore,
    reference: &ArtifactRef,
    base: &BaseState,
    base_directory: &Path,
    destination: &Path,
) -> Result<(), ArtifactStoreError> {
    if destination.exists() {
        return Err(ArtifactStoreError::DestinationExists(
            destination.to_path_buf(),
        ));
    }
    let artifact = store.load_manifest(reference)?;
    if artifact.base() != base {
        return Err(ArtifactStoreError::BaseMismatch);
    }
    let mut entries = capture_tree(base_directory)?;
    apply_delta(store, &mut entries, artifact.delta().operations())?;

    fs::create_dir(destination)?;
    if let Err(error) = write_snapshot(destination, &entries) {
        let _ = fs::remove_dir_all(destination);
        return Err(error);
    }
    Ok(())
}

fn capture_tree(root: &Path) -> Result<BTreeMap<String, SnapshotEntry>, ArtifactStoreError> {
    let metadata = fs::symlink_metadata(root)?;
    if !metadata.file_type().is_dir() {
        return Err(ArtifactStoreError::UnsupportedFileType(root.to_path_buf()));
    }
    let mut entries = BTreeMap::new();
    capture_directory(root, root, &mut entries)?;
    Ok(entries)
}

fn capture_directory(
    root: &Path,
    directory: &Path,
    entries: &mut BTreeMap<String, SnapshotEntry>,
) -> Result<(), ArtifactStoreError> {
    let mut children: Vec<_> = fs::read_dir(directory)?.collect::<Result<_, _>>()?;
    children.sort_by_key(std::fs::DirEntry::file_name);
    for child in children {
        let path = child.path();
        let file_type = child.file_type()?;
        if file_type.is_dir() {
            capture_directory(root, &path, entries)?;
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| ArtifactStoreError::StructuralConflict("path escaped base".to_owned()))?;
        let canonical_path = canonical_path(relative)?;
        let entry = if file_type.is_file() {
            SnapshotEntry {
                mode: regular_mode(&path)?,
                content: fs::read(&path)?,
            }
        } else if file_type.is_symlink() {
            SnapshotEntry {
                mode: FileMode::SymbolicLink,
                content: symlink_target_bytes(&fs::read_link(&path)?)?,
            }
        } else {
            return Err(ArtifactStoreError::UnsupportedFileType(path));
        };
        if entries.insert(canonical_path, entry).is_some() {
            return Err(ArtifactStoreError::StructuralConflict(
                "duplicate base path".to_owned(),
            ));
        }
    }
    Ok(())
}

fn apply_delta(
    store: &ArtifactStore,
    entries: &mut BTreeMap<String, SnapshotEntry>,
    operations: &[PathOperation],
) -> Result<(), ArtifactStoreError> {
    for operation in operations {
        if let PathOperation::Delete { path } = operation
            && entries.remove(path).is_none()
        {
            return Err(ArtifactStoreError::StructuralConflict(format!(
                "delete path is absent from exact base: {path}"
            )));
        }
    }
    for operation in operations {
        let PathOperation::Upsert {
            path,
            mode,
            blob_digest,
        } = operation
        else {
            continue;
        };
        validate_structure(entries, path)?;
        let content = store.load_blob(blob_digest)?;
        if *mode == FileMode::SymbolicLink && content.contains(&0) {
            return Err(ArtifactStoreError::StructuralConflict(format!(
                "symbolic link target contains NUL: {path}"
            )));
        }
        entries.insert(
            path.clone(),
            SnapshotEntry {
                mode: *mode,
                content,
            },
        );
    }
    Ok(())
}

fn validate_structure(
    entries: &BTreeMap<String, SnapshotEntry>,
    path: &str,
) -> Result<(), ArtifactStoreError> {
    let mut offset = 0;
    for component in path
        .split('/')
        .take(path.split('/').count().saturating_sub(1))
    {
        offset += component.len();
        let ancestor = &path[..offset];
        if entries.contains_key(ancestor) {
            return Err(ArtifactStoreError::StructuralConflict(format!(
                "path has a file or symlink ancestor: {ancestor}"
            )));
        }
        offset += 1;
    }
    let descendant_prefix = format!("{path}/");
    if entries
        .range(descendant_prefix.clone()..)
        .next()
        .is_some_and(|(candidate, _)| candidate.starts_with(&descendant_prefix))
    {
        return Err(ArtifactStoreError::StructuralConflict(format!(
            "path would replace a directory without deleting its children: {path}"
        )));
    }
    Ok(())
}

fn write_snapshot(
    destination: &Path,
    entries: &BTreeMap<String, SnapshotEntry>,
) -> Result<(), ArtifactStoreError> {
    for (path, entry) in entries {
        let destination_path = destination.join(path);
        if let Some(parent) = destination_path.parent() {
            fs::create_dir_all(parent)?;
        }
        match entry.mode {
            FileMode::Regular | FileMode::Executable => {
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&destination_path)?;
                file.write_all(&entry.content)?;
                file.sync_all()?;
                set_file_mode(&destination_path, entry.mode)?;
            }
            FileMode::SymbolicLink => {
                create_symlink(&entry.content, &destination_path)?;
            }
        }
    }
    Ok(())
}

fn canonical_path(path: &Path) -> Result<String, ArtifactStoreError> {
    let mut parts = Vec::new();
    for component in path.components() {
        let Component::Normal(part) = component else {
            return Err(ArtifactStoreError::StructuralConflict(
                "base contains a non-normal path".to_owned(),
            ));
        };
        let part = part
            .to_str()
            .ok_or_else(|| ArtifactStoreError::NonUtf8Path(path.to_path_buf()))?;
        if part.contains(['\\', '\0']) {
            return Err(ArtifactStoreError::StructuralConflict(format!(
                "base path is not canonically representable: {}",
                path.display()
            )));
        }
        parts.push(part);
    }
    Ok(parts.join("/"))
}

#[cfg(unix)]
fn regular_mode(path: &Path) -> Result<FileMode, ArtifactStoreError> {
    use std::os::unix::fs::PermissionsExt;

    let mode = fs::metadata(path)?.permissions().mode();
    Ok(if mode & 0o111 == 0 {
        FileMode::Regular
    } else {
        FileMode::Executable
    })
}

#[cfg(not(unix))]
fn regular_mode(_path: &Path) -> Result<FileMode, ArtifactStoreError> {
    Ok(FileMode::Regular)
}

#[cfg(unix)]
fn set_file_mode(path: &Path, mode: FileMode) -> Result<(), ArtifactStoreError> {
    use std::os::unix::fs::PermissionsExt;

    let permissions = fs::Permissions::from_mode(match mode {
        FileMode::Regular => 0o644,
        FileMode::Executable => 0o755,
        FileMode::SymbolicLink => unreachable!(),
    });
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_file_mode(_path: &Path, _mode: FileMode) -> Result<(), ArtifactStoreError> {
    Ok(())
}

#[cfg(unix)]
fn symlink_target_bytes(path: &Path) -> Result<Vec<u8>, ArtifactStoreError> {
    use std::os::unix::ffi::OsStrExt;

    let bytes = path.as_os_str().as_bytes().to_vec();
    if bytes.contains(&0) {
        return Err(ArtifactStoreError::StructuralConflict(
            "symbolic link target contains NUL".to_owned(),
        ));
    }
    Ok(bytes)
}

#[cfg(not(unix))]
fn symlink_target_bytes(path: &Path) -> Result<Vec<u8>, ArtifactStoreError> {
    path.to_str()
        .map(|value| value.as_bytes().to_vec())
        .ok_or_else(|| ArtifactStoreError::NonUtf8Path(path.to_path_buf()))
}

#[cfg(unix)]
fn create_symlink(target: &[u8], destination: &Path) -> Result<(), ArtifactStoreError> {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::symlink;

    symlink(OsStr::from_bytes(target), destination)?;
    Ok(())
}

#[cfg(not(unix))]
fn create_symlink(_target: &[u8], destination: &Path) -> Result<(), ArtifactStoreError> {
    Err(ArtifactStoreError::UnsupportedFileType(
        destination.to_path_buf(),
    ))
}
