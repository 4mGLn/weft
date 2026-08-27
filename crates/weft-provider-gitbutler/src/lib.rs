//! Exact, version-gated `GitButler` CLI adapter for supported local Weft workflows.

mod command;
mod error;
mod provider;
mod schema;

pub use error::GitButlerProviderError;
pub use provider::{
    CanonicalExport, GitButler, GitButlerCandidate, GitButlerCandidateInput, GitButlerCapabilities,
    GitButlerCapability, GitButlerChange, GitButlerConflict, GitButlerDiscovery, GitButlerStack,
    LandingPlan, LandingReconciliation, LandingResultEvidence, ProjectObservation,
    ProjectReconciliation,
};

#[cfg(test)]
mod tests;
