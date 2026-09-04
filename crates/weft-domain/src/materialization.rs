use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::{ActorId, ChangeId, RevisionId, UnixMillis};

macro_rules! materialization_id {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(String);

        impl $name {
            /// Creates a non-empty materialization identifier.
            ///
            /// # Errors
            ///
            /// Returns [`MaterializationError::EmptyIdentifier`] for an empty value.
            pub fn new(value: impl Into<String>) -> Result<Self, MaterializationError> {
                let value = value.into();
                if value.trim().is_empty() {
                    return Err(MaterializationError::EmptyIdentifier(stringify!($name)));
                }
                Ok(Self(value))
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

materialization_id!(MaterializationId);
materialization_id!(WorkspaceId);
materialization_id!(ProviderId);
materialization_id!(ProviderRef);
materialization_id!(ProviderEvidence);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderObservation {
    state: MaterializationState,
    provider_ref: ProviderRef,
    evidence: ProviderEvidence,
}

impl ProviderObservation {
    #[must_use]
    pub const fn new(
        state: MaterializationState,
        provider_ref: ProviderRef,
        evidence: ProviderEvidence,
    ) -> Self {
        Self {
            state,
            provider_ref,
            evidence,
        }
    }

    #[must_use]
    pub fn into_parts(self) -> (MaterializationState, ProviderRef, ProviderEvidence) {
        (self.state, self.provider_ref, self.evidence)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaterializationPlacement {
    workspace_id: WorkspaceId,
    provider_id: ProviderId,
    provider_ref: ProviderRef,
}

impl MaterializationPlacement {
    #[must_use]
    pub const fn new(
        workspace_id: WorkspaceId,
        provider_id: ProviderId,
        provider_ref: ProviderRef,
    ) -> Self {
        Self {
            workspace_id,
            provider_id,
            provider_ref,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct MaterializationVersion(i64);

impl MaterializationVersion {
    pub const EMPTY: Self = Self(0);
    pub const INITIAL: Self = Self(1);

    /// Creates a non-negative optimistic-concurrency version.
    ///
    /// # Errors
    ///
    /// Returns [`MaterializationError::InvalidVersion`] for a negative value.
    pub const fn new(value: i64) -> Result<Self, MaterializationError> {
        if value < 0 {
            return Err(MaterializationError::InvalidVersion);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub const fn value(self) -> i64 {
        self.0
    }

    fn next(self) -> Result<Self, MaterializationError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or(MaterializationError::VersionExhausted)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MaterializationState {
    Clean,
    Dirty,
    Diverged,
    Suspended,
    Released,
    Invalidated,
}

impl MaterializationState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::Dirty => "dirty",
            Self::Diverged => "diverged",
            Self::Suspended => "suspended",
            Self::Released => "released",
            Self::Invalidated => "invalidated",
        }
    }

    /// Parses a stable storage/API value.
    ///
    /// # Errors
    ///
    /// Returns [`MaterializationError::InvalidState`] for an unknown value.
    pub fn parse(value: &str) -> Result<Self, MaterializationError> {
        match value {
            "clean" => Ok(Self::Clean),
            "dirty" => Ok(Self::Dirty),
            "diverged" => Ok(Self::Diverged),
            "suspended" => Ok(Self::Suspended),
            "released" => Ok(Self::Released),
            "invalidated" => Ok(Self::Invalidated),
            _ => Err(MaterializationError::InvalidState(value.to_owned())),
        }
    }

    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Released | Self::Invalidated)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Materialization {
    id: MaterializationId,
    change_id: ChangeId,
    revision_id: RevisionId,
    placement: MaterializationPlacement,
    state: MaterializationState,
    version: MaterializationVersion,
    created_at: UnixMillis,
    created_by: ActorId,
    state_changed_at: UnixMillis,
    released_at: Option<UnixMillis>,
}

impl Materialization {
    #[must_use]
    pub const fn new(
        id: MaterializationId,
        change_id: ChangeId,
        revision_id: RevisionId,
        placement: MaterializationPlacement,
        created_at: UnixMillis,
        created_by: ActorId,
    ) -> Self {
        Self {
            id,
            change_id,
            revision_id,
            placement,
            state: MaterializationState::Clean,
            version: MaterializationVersion::INITIAL,
            created_at,
            created_by,
            state_changed_at: created_at,
            released_at: None,
        }
    }

    #[must_use]
    pub const fn id(&self) -> &MaterializationId {
        &self.id
    }

    #[must_use]
    pub const fn change_id(&self) -> &ChangeId {
        &self.change_id
    }

    #[must_use]
    pub const fn revision_id(&self) -> &RevisionId {
        &self.revision_id
    }

    #[must_use]
    pub const fn workspace_id(&self) -> &WorkspaceId {
        &self.placement.workspace_id
    }

    #[must_use]
    pub const fn provider_id(&self) -> &ProviderId {
        &self.placement.provider_id
    }

    #[must_use]
    pub const fn provider_ref(&self) -> &ProviderRef {
        &self.placement.provider_ref
    }

    #[must_use]
    pub const fn state(&self) -> MaterializationState {
        self.state
    }

    #[must_use]
    pub const fn version(&self) -> MaterializationVersion {
        self.version
    }

    #[must_use]
    pub const fn created_at(&self) -> UnixMillis {
        self.created_at
    }

    #[must_use]
    pub const fn created_by(&self) -> &ActorId {
        &self.created_by
    }

    #[must_use]
    pub const fn state_changed_at(&self) -> UnixMillis {
        self.state_changed_at
    }

    #[must_use]
    pub const fn released_at(&self) -> Option<UnixMillis> {
        self.released_at
    }

    /// Records a verified provider observation without changing exact revision,
    /// workspace, provider, or Materialization identity.
    ///
    /// # Errors
    ///
    /// Rejects stale versions, terminal transitions, time reversal, and a request
    /// that changes neither state nor provider reference.
    pub fn transition(
        &mut self,
        expected_version: MaterializationVersion,
        state: MaterializationState,
        provider_ref: ProviderRef,
        occurred_at: UnixMillis,
    ) -> Result<(), MaterializationError> {
        if self.version != expected_version {
            return Err(MaterializationError::StaleVersion {
                expected: expected_version,
                actual: self.version,
            });
        }
        if self.state.is_terminal() {
            return Err(MaterializationError::TerminalState(self.state));
        }
        if occurred_at < self.state_changed_at {
            return Err(MaterializationError::TimestampBeforePriorEvent);
        }
        if state == self.state && provider_ref == self.placement.provider_ref {
            return Err(MaterializationError::NoChange);
        }
        self.version = self.version.next()?;
        self.state = state;
        self.placement.provider_ref = provider_ref;
        self.state_changed_at = occurred_at;
        self.released_at = (state == MaterializationState::Released).then_some(occurred_at);
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MaterializationError {
    EmptyIdentifier(&'static str),
    InvalidState(String),
    InvalidVersion,
    VersionExhausted,
    StaleVersion {
        expected: MaterializationVersion,
        actual: MaterializationVersion,
    },
    TerminalState(MaterializationState),
    TimestampBeforePriorEvent,
    NoChange,
}

impl Display for MaterializationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyIdentifier(kind) => write!(formatter, "{kind} cannot be empty"),
            Self::InvalidState(value) => {
                write!(formatter, "invalid materialization state: {value}")
            }
            Self::InvalidVersion => {
                formatter.write_str("materialization version cannot be negative")
            }
            Self::VersionExhausted => formatter.write_str("materialization version is exhausted"),
            Self::StaleVersion { expected, actual } => write!(
                formatter,
                "stale materialization version: expected {}, actual {}",
                expected.value(),
                actual.value()
            ),
            Self::TerminalState(state) => write!(
                formatter,
                "materialization state is terminal: {}",
                state.as_str()
            ),
            Self::TimestampBeforePriorEvent => {
                formatter.write_str("materialization event timestamp precedes prior state")
            }
            Self::NoChange => formatter.write_str("materialization transition changes no state"),
        }
    }
}

impl Error for MaterializationError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(value: i64) -> UnixMillis {
        UnixMillis::new(value).unwrap()
    }

    fn materialization() -> Materialization {
        Materialization::new(
            MaterializationId::new("materialization-1").unwrap(),
            ChangeId::new("change-1").unwrap(),
            RevisionId::new("revision-1").unwrap(),
            MaterializationPlacement::new(
                WorkspaceId::new("workspace-1").unwrap(),
                ProviderId::new("native-git").unwrap(),
                ProviderRef::new("refs/weft/change-1").unwrap(),
            ),
            at(10),
            ActorId::new("operator-1").unwrap(),
        )
    }

    #[test]
    fn transitions_observed_state_without_retargeting_identity() {
        let mut value = materialization();
        value
            .transition(
                MaterializationVersion::INITIAL,
                MaterializationState::Dirty,
                ProviderRef::new("refs/weft/change-1").unwrap(),
                at(20),
            )
            .unwrap();
        value
            .transition(
                MaterializationVersion::new(2).unwrap(),
                MaterializationState::Clean,
                ProviderRef::new("refs/weft/rewritten").unwrap(),
                at(30),
            )
            .unwrap();

        assert_eq!(value.change_id().as_str(), "change-1");
        assert_eq!(value.revision_id().as_str(), "revision-1");
        assert_eq!(value.workspace_id().as_str(), "workspace-1");
        assert_eq!(value.provider_id().as_str(), "native-git");
        assert_eq!(value.provider_ref().as_str(), "refs/weft/rewritten");
        assert_eq!(value.version().value(), 3);
    }

    #[test]
    fn provider_reference_can_advance_without_claiming_new_revision_content() {
        let mut value = materialization();
        value
            .transition(
                MaterializationVersion::INITIAL,
                MaterializationState::Clean,
                ProviderRef::new("gitbutler-change-id-after-rewrite").unwrap(),
                at(20),
            )
            .unwrap();

        assert_eq!(value.state(), MaterializationState::Clean);
        assert_eq!(value.revision_id().as_str(), "revision-1");
        assert_eq!(value.version().value(), 2);
    }

    #[test]
    fn rejects_stale_noop_and_time_reversing_transitions() {
        let mut value = materialization();
        assert!(matches!(
            value.transition(
                MaterializationVersion::EMPTY,
                MaterializationState::Dirty,
                ProviderRef::new("refs/weft/change-1").unwrap(),
                at(20)
            ),
            Err(MaterializationError::StaleVersion { .. })
        ));
        assert!(matches!(
            value.transition(
                MaterializationVersion::INITIAL,
                MaterializationState::Clean,
                ProviderRef::new("refs/weft/change-1").unwrap(),
                at(20)
            ),
            Err(MaterializationError::NoChange)
        ));
        assert!(matches!(
            value.transition(
                MaterializationVersion::INITIAL,
                MaterializationState::Dirty,
                ProviderRef::new("refs/weft/change-1").unwrap(),
                at(9)
            ),
            Err(MaterializationError::TimestampBeforePriorEvent)
        ));
    }

    #[test]
    fn released_and_invalidated_materializations_are_terminal() {
        for terminal in [
            MaterializationState::Released,
            MaterializationState::Invalidated,
        ] {
            let mut value = materialization();
            value
                .transition(
                    MaterializationVersion::INITIAL,
                    terminal,
                    ProviderRef::new("refs/weft/change-1").unwrap(),
                    at(20),
                )
                .unwrap();
            assert_eq!(
                value.released_at(),
                (terminal == MaterializationState::Released).then_some(at(20))
            );
            assert!(matches!(
                value.transition(
                    MaterializationVersion::new(2).unwrap(),
                    MaterializationState::Clean,
                    ProviderRef::new("refs/weft/change-1").unwrap(),
                    at(30)
                ),
                Err(MaterializationError::TerminalState(_))
            ));
        }
    }

    #[test]
    fn rejects_invalid_storage_values() {
        assert!(MaterializationId::new(" ").is_err());
        assert!(WorkspaceId::new("").is_err());
        assert!(ProviderId::new(" ").is_err());
        assert!(ProviderRef::new(" ").is_err());
        assert!(MaterializationState::parse("missing").is_err());
        assert!(MaterializationVersion::new(-1).is_err());
    }
}
