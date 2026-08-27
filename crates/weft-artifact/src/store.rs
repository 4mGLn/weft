use std::path::Path;

use weft_domain::{ArtifactRef, BaseState, PathOperation, TREE_DELTA_V1};

use crate::{
    ArtifactStoreError, CanonicalTreeDelta, CasDigest, FilesystemCas, reconstruct::reconstruct,
};

#[derive(Debug)]
pub struct ArtifactStore {
    cas: FilesystemCas,
}

impl ArtifactStore {
    /// Opens the provider-independent artifact store rooted at `path`.
    ///
    /// # Errors
    ///
    /// Returns [`ArtifactStoreError`] when its filesystem CAS cannot be opened.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ArtifactStoreError> {
        Ok(Self {
            cas: FilesystemCas::open(path)?,
        })
    }

    /// Stores one content blob and returns its durable content identity.
    ///
    /// # Errors
    ///
    /// Returns [`ArtifactStoreError`] for size, corruption, or filesystem errors.
    pub fn store_blob(&self, bytes: &[u8]) -> Result<CasDigest, ArtifactStoreError> {
        self.cas.put(bytes)
    }

    /// Loads and verifies one content blob.
    ///
    /// # Errors
    ///
    /// Returns [`ArtifactStoreError`] when the digest is invalid, missing, too
    /// large, or does not match the stored bytes.
    pub fn load_blob(&self, digest: &str) -> Result<Vec<u8>, ArtifactStoreError> {
        self.cas.get(&CasDigest::parse(digest)?)
    }

    /// Stores a canonical manifest after verifying every referenced blob.
    ///
    /// # Errors
    ///
    /// Returns [`ArtifactStoreError`] when encoding fails, a referenced blob is
    /// absent/corrupt, or the manifest cannot be committed atomically.
    pub fn store_manifest(
        &self,
        artifact: &CanonicalTreeDelta,
    ) -> Result<ArtifactRef, ArtifactStoreError> {
        self.verify_referenced_blobs(artifact)?;
        let digest = self.cas.put(&artifact.encode()?)?;
        Ok(ArtifactRef::tree_delta_v1(digest.as_str())?)
    }

    /// Loads a manifest by exact domain reference and verifies all content.
    ///
    /// # Errors
    ///
    /// Returns [`ArtifactStoreError`] for unsupported references, missing or
    /// corrupt objects, malformed canonical bytes, or missing referenced blobs.
    pub fn load_manifest(
        &self,
        reference: &ArtifactRef,
    ) -> Result<CanonicalTreeDelta, ArtifactStoreError> {
        if reference.version() != TREE_DELTA_V1 {
            return Err(ArtifactStoreError::InvalidManifest(format!(
                "unsupported reference version: {}",
                reference.version()
            )));
        }
        let digest = CasDigest::parse(reference.manifest_digest())?;
        let artifact = CanonicalTreeDelta::decode(&self.cas.get(&digest)?)?;
        self.verify_referenced_blobs(&artifact)?;
        Ok(artifact)
    }

    /// Reconstructs an exact revision tree from a verified base materialization.
    ///
    /// The caller is responsible for proving that `base_directory` contains the
    /// exact `base` state. This method verifies the recorded base identity,
    /// canonical manifest and every blob before creating `destination`.
    ///
    /// # Errors
    ///
    /// Returns [`ArtifactStoreError`] for a mismatched base, corrupt artifact,
    /// structurally incompatible base tree, unsafe file type, or existing output.
    pub fn reconstruct(
        &self,
        reference: &ArtifactRef,
        base: &BaseState,
        base_directory: impl AsRef<Path>,
        destination: impl AsRef<Path>,
    ) -> Result<(), ArtifactStoreError> {
        reconstruct(
            self,
            reference,
            base,
            base_directory.as_ref(),
            destination.as_ref(),
        )
    }

    fn verify_referenced_blobs(
        &self,
        artifact: &CanonicalTreeDelta,
    ) -> Result<(), ArtifactStoreError> {
        for operation in artifact.delta().operations() {
            if let PathOperation::Upsert { blob_digest, .. } = operation {
                let digest = CasDigest::parse(blob_digest)?;
                match self.cas.get(&digest) {
                    Ok(_) => {}
                    Err(ArtifactStoreError::ObjectMissing(_)) => {
                        return Err(ArtifactStoreError::MissingReferencedBlob(digest));
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) const fn cas(&self) -> &FilesystemCas {
        &self.cas
    }
}
