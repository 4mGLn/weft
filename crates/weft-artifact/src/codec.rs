use weft_domain::{BaseState, FileMode, PathOperation, RepositoryId, TREE_DELTA_V1, TreeDelta};

use crate::ArtifactStoreError;

const MAGIC: &[u8] = b"WEFT-ARTIFACT\0";
const MAX_FIELD_BYTES: usize = 16 * 1024 * 1024;
const MAX_OPERATIONS: usize = 1_000_000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalTreeDelta {
    base: BaseState,
    delta: TreeDelta,
}

impl CanonicalTreeDelta {
    #[must_use]
    pub const fn new(base: BaseState, delta: TreeDelta) -> Self {
        Self { base, delta }
    }

    #[must_use]
    pub const fn base(&self) -> &BaseState {
        &self.base
    }

    #[must_use]
    pub const fn delta(&self) -> &TreeDelta {
        &self.delta
    }

    /// Encodes the artifact into the canonical `tree-delta-v1` byte format.
    ///
    /// # Errors
    ///
    /// Returns [`ArtifactStoreError`] when a field or operation count exceeds
    /// the versioned format's 32-bit length bounds.
    pub fn encode(&self) -> Result<Vec<u8>, ArtifactStoreError> {
        if self.delta.operations().len() > MAX_OPERATIONS {
            return Err(ArtifactStoreError::InvalidManifest(format!(
                "operation count exceeds {MAX_OPERATIONS}"
            )));
        }
        let mut bytes = Vec::new();
        bytes.extend_from_slice(MAGIC);
        encode_string(&mut bytes, TREE_DELTA_V1)?;
        encode_string(&mut bytes, self.base.repository_id().as_str())?;
        encode_string(&mut bytes, self.base.object_id())?;
        encode_u32(&mut bytes, self.delta.operations().len())?;
        for operation in self.delta.operations() {
            match operation {
                PathOperation::Delete { path } => {
                    bytes.push(0);
                    encode_string(&mut bytes, path)?;
                }
                PathOperation::Upsert {
                    path,
                    mode,
                    blob_digest,
                } => {
                    bytes.push(match mode {
                        FileMode::Regular => 1,
                        FileMode::Executable => 2,
                        FileMode::SymbolicLink => 3,
                    });
                    encode_string(&mut bytes, path)?;
                    encode_string(&mut bytes, blob_digest)?;
                }
            }
        }
        Ok(bytes)
    }

    /// Decodes and revalidates canonical artifact bytes.
    ///
    /// # Errors
    ///
    /// Returns [`ArtifactStoreError`] for malformed, unsupported, non-canonical,
    /// or trailing input.
    pub fn decode(bytes: &[u8]) -> Result<Self, ArtifactStoreError> {
        let mut decoder = Decoder::new(bytes);
        decoder.expect(MAGIC)?;
        let version = decoder.string()?;
        if version != TREE_DELTA_V1 {
            return Err(ArtifactStoreError::InvalidManifest(format!(
                "unsupported artifact version: {version}"
            )));
        }
        let repository_id = RepositoryId::new(decoder.string()?)?;
        let base = BaseState::new(repository_id, decoder.string()?)?;
        let operation_count = usize::try_from(decoder.u32()?).map_err(|_| {
            ArtifactStoreError::InvalidManifest("operation count does not fit usize".to_owned())
        })?;
        if operation_count > MAX_OPERATIONS {
            return Err(ArtifactStoreError::InvalidManifest(format!(
                "operation count exceeds {MAX_OPERATIONS}"
            )));
        }
        let mut operations = Vec::with_capacity(operation_count);
        for _ in 0..operation_count {
            let tag = decoder.byte()?;
            let path = decoder.string()?;
            let operation = match tag {
                0 => PathOperation::Delete { path },
                1..=3 => PathOperation::Upsert {
                    path,
                    mode: match tag {
                        1 => FileMode::Regular,
                        2 => FileMode::Executable,
                        3 => FileMode::SymbolicLink,
                        _ => unreachable!(),
                    },
                    blob_digest: decoder.string()?,
                },
                _ => {
                    return Err(ArtifactStoreError::InvalidManifest(format!(
                        "unknown operation tag: {tag}"
                    )));
                }
            };
            operations.push(operation);
        }
        if !decoder.is_finished() {
            return Err(ArtifactStoreError::InvalidManifest(
                "trailing bytes after manifest".to_owned(),
            ));
        }
        Ok(Self::new(base, TreeDelta::new(operations)?))
    }
}

fn encode_string(bytes: &mut Vec<u8>, value: &str) -> Result<(), ArtifactStoreError> {
    if value.len() > MAX_FIELD_BYTES {
        return Err(ArtifactStoreError::InvalidManifest(format!(
            "field exceeds {MAX_FIELD_BYTES} bytes"
        )));
    }
    encode_u32(bytes, value.len())?;
    bytes.extend_from_slice(value.as_bytes());
    Ok(())
}

fn encode_u32(bytes: &mut Vec<u8>, value: usize) -> Result<(), ArtifactStoreError> {
    let value = u32::try_from(value).map_err(|_| {
        ArtifactStoreError::InvalidManifest("value exceeds 32-bit format bound".to_owned())
    })?;
    bytes.extend_from_slice(&value.to_be_bytes());
    Ok(())
}

struct Decoder<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Decoder<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn expect(&mut self, expected: &[u8]) -> Result<(), ArtifactStoreError> {
        if self.take(expected.len())? != expected {
            return Err(ArtifactStoreError::InvalidManifest(
                "invalid artifact magic".to_owned(),
            ));
        }
        Ok(())
    }

    fn byte(&mut self) -> Result<u8, ArtifactStoreError> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, ArtifactStoreError> {
        let value: [u8; 4] = self
            .take(4)?
            .try_into()
            .map_err(|_| ArtifactStoreError::InvalidManifest("invalid u32".to_owned()))?;
        Ok(u32::from_be_bytes(value))
    }

    fn string(&mut self) -> Result<String, ArtifactStoreError> {
        let length = usize::try_from(self.u32()?).map_err(|_| {
            ArtifactStoreError::InvalidManifest("field length does not fit usize".to_owned())
        })?;
        if length > MAX_FIELD_BYTES {
            return Err(ArtifactStoreError::InvalidManifest(format!(
                "field exceeds {MAX_FIELD_BYTES} bytes"
            )));
        }
        let value = std::str::from_utf8(self.take(length)?).map_err(|_| {
            ArtifactStoreError::InvalidManifest("field is not valid UTF-8".to_owned())
        })?;
        Ok(value.to_owned())
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], ArtifactStoreError> {
        let end = self.offset.checked_add(length).ok_or_else(|| {
            ArtifactStoreError::InvalidManifest("manifest offset overflow".to_owned())
        })?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| ArtifactStoreError::InvalidManifest("truncated manifest".to_owned()))?;
        self.offset = end;
        Ok(value)
    }

    const fn is_finished(&self) -> bool {
        self.offset == self.bytes.len()
    }
}
