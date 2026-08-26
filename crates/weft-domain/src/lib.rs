//! Provider-neutral domain invariants for Weft.

mod artifact;
mod change;

pub use artifact::{ArtifactError, FileMode, PathOperation, TreeDelta};
pub use change::{
    ArtifactRef, BaseState, Change, ChangeError, ChangeId, ChangeRevision, NewRevision,
    RepositoryId, RevisionId,
};
