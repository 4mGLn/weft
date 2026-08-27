//! Version-gated Native Git provider adapter for exact local Weft workflows.

mod command;
mod error;
mod provider;

pub use error::GitProviderError;
pub use provider::{
    CandidateComposition, CapturedRevision, GitCandidateInput, GitCapabilities, GitCapability,
    IntegrationPlan, IntegrationResult, MaterializationResult, NativeGit, ReconciliationResult,
    RepositoryDiscovery, RevisionObservation,
};

#[cfg(test)]
mod tests;
