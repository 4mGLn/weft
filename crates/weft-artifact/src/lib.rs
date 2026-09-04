//! Durable provider-independent canonical artifacts for Weft.

mod cas;
mod codec;
mod error;
mod reconstruct;
mod store;

pub use cas::{CasDigest, FilesystemCas};
pub use codec::CanonicalTreeDelta;
pub use error::ArtifactStoreError;
pub use store::ArtifactStore;

#[cfg(test)]
mod tests;
