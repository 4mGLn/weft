use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::{ActorId, ChangeId, RevisionId, UnixMillis};

macro_rules! relationship_id {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(String);

        impl $name {
            /// Creates a non-empty relationship identifier.
            ///
            /// # Errors
            ///
            /// Returns [`RelationshipError::EmptyIdentifier`] for an empty value.
            pub fn new(value: impl Into<String>) -> Result<Self, RelationshipError> {
                let value = value.into();
                if value.trim().is_empty() {
                    return Err(RelationshipError::EmptyIdentifier(stringify!($name)));
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

relationship_id!(RelationshipId);
relationship_id!(DependencyId);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct RelationshipVersion(i64);

impl RelationshipVersion {
    pub const EMPTY: Self = Self(0);
    pub const INITIAL: Self = Self(1);

    /// Creates a non-negative optimistic-concurrency version.
    ///
    /// # Errors
    ///
    /// Returns [`RelationshipError::InvalidVersion`] for a negative value.
    pub const fn new(value: i64) -> Result<Self, RelationshipError> {
        if value < 0 {
            return Err(RelationshipError::InvalidVersion);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub const fn value(self) -> i64 {
        self.0
    }

    fn next(self) -> Result<Self, RelationshipError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or(RelationshipError::VersionExhausted)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RelationshipKind {
    TaskDecomposition,
    RelatedTo,
}

impl RelationshipKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TaskDecomposition => "task_decomposition",
            Self::RelatedTo => "related_to",
        }
    }

    /// Parses a stable storage/API value.
    ///
    /// # Errors
    ///
    /// Returns [`RelationshipError::InvalidRelationshipKind`] for an unknown value.
    pub fn parse(value: &str) -> Result<Self, RelationshipError> {
        match value {
            "task_decomposition" => Ok(Self::TaskDecomposition),
            "related_to" => Ok(Self::RelatedTo),
            _ => Err(RelationshipError::InvalidRelationshipKind(value.to_owned())),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelationshipEndpoints {
    first: ChangeId,
    second: ChangeId,
}

impl RelationshipEndpoints {
    /// Canonicalizes one symmetric pair by Change identity.
    ///
    /// # Errors
    ///
    /// Returns [`RelationshipError::SelfRelationship`] when both ends are identical.
    pub fn new(left: ChangeId, right: ChangeId) -> Result<Self, RelationshipError> {
        if left == right {
            return Err(RelationshipError::SelfRelationship);
        }
        let (first, second) = if left < right {
            (left, right)
        } else {
            (right, left)
        };
        Ok(Self { first, second })
    }

    #[must_use]
    pub const fn first(&self) -> &ChangeId {
        &self.first
    }

    #[must_use]
    pub const fn second(&self) -> &ChangeId {
        &self.second
    }

    #[must_use]
    pub fn contains(&self, change_id: &ChangeId) -> bool {
        self.first == *change_id || self.second == *change_id
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Relationship {
    id: RelationshipId,
    kind: RelationshipKind,
    endpoints: RelationshipEndpoints,
    created_at: UnixMillis,
    created_by: ActorId,
    version: RelationshipVersion,
    removed_at: Option<UnixMillis>,
    removed_by: Option<ActorId>,
}

impl Relationship {
    #[must_use]
    pub const fn new(
        id: RelationshipId,
        kind: RelationshipKind,
        endpoints: RelationshipEndpoints,
        created_at: UnixMillis,
        created_by: ActorId,
    ) -> Self {
        Self {
            id,
            kind,
            endpoints,
            created_at,
            created_by,
            version: RelationshipVersion::INITIAL,
            removed_at: None,
            removed_by: None,
        }
    }

    #[must_use]
    pub const fn id(&self) -> &RelationshipId {
        &self.id
    }

    #[must_use]
    pub const fn kind(&self) -> RelationshipKind {
        self.kind
    }

    #[must_use]
    pub const fn endpoints(&self) -> &RelationshipEndpoints {
        &self.endpoints
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
    pub const fn version(&self) -> RelationshipVersion {
        self.version
    }

    #[must_use]
    pub const fn removed_at(&self) -> Option<UnixMillis> {
        self.removed_at
    }

    #[must_use]
    pub const fn removed_by(&self) -> Option<&ActorId> {
        self.removed_by.as_ref()
    }

    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.removed_at.is_none()
    }

    /// Removes a symmetric relationship without erasing its identity or history.
    ///
    /// # Errors
    ///
    /// Rejects stale versions, repeated removal, and time reversal.
    pub fn remove(
        &mut self,
        expected_version: RelationshipVersion,
        removed_at: UnixMillis,
        removed_by: ActorId,
    ) -> Result<(), RelationshipError> {
        ensure_mutable_version(
            self.version,
            expected_version,
            self.removed_at,
            self.created_at,
            removed_at,
        )?;
        self.version = self.version.next()?;
        self.removed_at = Some(removed_at);
        self.removed_by = Some(removed_by);
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DependencyPins {
    downstream_revision_id: RevisionId,
    upstream_revision_id: RevisionId,
}

impl DependencyPins {
    #[must_use]
    pub const fn new(downstream_revision_id: RevisionId, upstream_revision_id: RevisionId) -> Self {
        Self {
            downstream_revision_id,
            upstream_revision_id,
        }
    }

    #[must_use]
    pub const fn downstream_revision_id(&self) -> &RevisionId {
        &self.downstream_revision_id
    }

    #[must_use]
    pub const fn upstream_revision_id(&self) -> &RevisionId {
        &self.upstream_revision_id
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DependencyFreshness {
    Current,
    DownstreamAdvanced,
    UpstreamAdvanced,
    BothAdvanced,
    Removed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Dependency {
    id: DependencyId,
    downstream_change_id: ChangeId,
    upstream_change_id: ChangeId,
    pins: DependencyPins,
    created_at: UnixMillis,
    created_by: ActorId,
    version: RelationshipVersion,
    updated_at: UnixMillis,
    updated_by: ActorId,
    removed_at: Option<UnixMillis>,
    removed_by: Option<ActorId>,
}

impl Dependency {
    /// Creates a directed exact-revision dependency.
    ///
    /// # Errors
    ///
    /// Returns [`RelationshipError::SelfDependency`] when both Changes are identical.
    pub fn new(
        id: DependencyId,
        downstream_change_id: ChangeId,
        upstream_change_id: ChangeId,
        pins: DependencyPins,
        created_at: UnixMillis,
        created_by: ActorId,
    ) -> Result<Self, RelationshipError> {
        if downstream_change_id == upstream_change_id {
            return Err(RelationshipError::SelfDependency);
        }
        Ok(Self {
            id,
            downstream_change_id,
            upstream_change_id,
            pins,
            created_at,
            created_by: created_by.clone(),
            version: RelationshipVersion::INITIAL,
            updated_at: created_at,
            updated_by: created_by,
            removed_at: None,
            removed_by: None,
        })
    }

    #[must_use]
    pub const fn id(&self) -> &DependencyId {
        &self.id
    }

    #[must_use]
    pub const fn downstream_change_id(&self) -> &ChangeId {
        &self.downstream_change_id
    }

    #[must_use]
    pub const fn upstream_change_id(&self) -> &ChangeId {
        &self.upstream_change_id
    }

    #[must_use]
    pub const fn pins(&self) -> &DependencyPins {
        &self.pins
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
    pub const fn version(&self) -> RelationshipVersion {
        self.version
    }

    #[must_use]
    pub const fn updated_at(&self) -> UnixMillis {
        self.updated_at
    }

    #[must_use]
    pub const fn updated_by(&self) -> &ActorId {
        &self.updated_by
    }

    #[must_use]
    pub const fn removed_at(&self) -> Option<UnixMillis> {
        self.removed_at
    }

    #[must_use]
    pub const fn removed_by(&self) -> Option<&ActorId> {
        self.removed_by.as_ref()
    }

    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.removed_at.is_none()
    }

    #[must_use]
    pub fn freshness(
        &self,
        downstream_head: &RevisionId,
        upstream_head: &RevisionId,
    ) -> DependencyFreshness {
        if !self.is_active() {
            return DependencyFreshness::Removed;
        }
        match (
            self.pins.downstream_revision_id != *downstream_head,
            self.pins.upstream_revision_id != *upstream_head,
        ) {
            (false, false) => DependencyFreshness::Current,
            (true, false) => DependencyFreshness::DownstreamAdvanced,
            (false, true) => DependencyFreshness::UpstreamAdvanced,
            (true, true) => DependencyFreshness::BothAdvanced,
        }
    }

    /// Replaces both exact pins with an explicitly verified pair.
    ///
    /// # Errors
    ///
    /// Rejects stale versions, terminal dependencies, time reversal, and no-op pins.
    pub fn repin(
        &mut self,
        expected_version: RelationshipVersion,
        pins: DependencyPins,
        updated_at: UnixMillis,
        updated_by: ActorId,
    ) -> Result<(), RelationshipError> {
        ensure_mutable_version(
            self.version,
            expected_version,
            self.removed_at,
            self.updated_at,
            updated_at,
        )?;
        if self.pins == pins {
            return Err(RelationshipError::UnchangedPins);
        }
        self.version = self.version.next()?;
        self.pins = pins;
        self.updated_at = updated_at;
        self.updated_by = updated_by;
        Ok(())
    }

    /// Removes a dependency without erasing its exact pin history.
    ///
    /// # Errors
    ///
    /// Rejects stale versions, repeated removal, and time reversal.
    pub fn remove(
        &mut self,
        expected_version: RelationshipVersion,
        removed_at: UnixMillis,
        removed_by: ActorId,
    ) -> Result<(), RelationshipError> {
        ensure_mutable_version(
            self.version,
            expected_version,
            self.removed_at,
            self.updated_at,
            removed_at,
        )?;
        self.version = self.version.next()?;
        self.updated_at = removed_at;
        self.updated_by = removed_by.clone();
        self.removed_at = Some(removed_at);
        self.removed_by = Some(removed_by);
        Ok(())
    }
}

fn ensure_mutable_version(
    actual: RelationshipVersion,
    expected: RelationshipVersion,
    removed_at: Option<UnixMillis>,
    prior_at: UnixMillis,
    occurred_at: UnixMillis,
) -> Result<(), RelationshipError> {
    if actual != expected {
        return Err(RelationshipError::StaleVersion { expected, actual });
    }
    if removed_at.is_some() {
        return Err(RelationshipError::AlreadyRemoved);
    }
    if occurred_at < prior_at {
        return Err(RelationshipError::TimestampBeforePriorEvent);
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RelationshipError {
    EmptyIdentifier(&'static str),
    InvalidRelationshipKind(String),
    InvalidVersion,
    VersionExhausted,
    SelfRelationship,
    SelfDependency,
    StaleVersion {
        expected: RelationshipVersion,
        actual: RelationshipVersion,
    },
    AlreadyRemoved,
    TimestampBeforePriorEvent,
    UnchangedPins,
}

impl Display for RelationshipError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyIdentifier(kind) => write!(formatter, "{kind} cannot be empty"),
            Self::InvalidRelationshipKind(value) => {
                write!(formatter, "invalid relationship kind: {value}")
            }
            Self::InvalidVersion => formatter.write_str("relationship version cannot be negative"),
            Self::VersionExhausted => formatter.write_str("relationship version is exhausted"),
            Self::SelfRelationship => {
                formatter.write_str("a symmetric relationship requires two distinct Changes")
            }
            Self::SelfDependency => formatter.write_str("a Change cannot depend on itself"),
            Self::StaleVersion { expected, actual } => write!(
                formatter,
                "stale relationship version: expected {}, actual {}",
                expected.value(),
                actual.value()
            ),
            Self::AlreadyRemoved => formatter.write_str("relationship is already removed"),
            Self::TimestampBeforePriorEvent => {
                formatter.write_str("relationship event precedes prior history")
            }
            Self::UnchangedPins => formatter.write_str("dependency repin must change an exact pin"),
        }
    }
}

impl Error for RelationshipError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(value: &str) -> ChangeId {
        ChangeId::new(value).unwrap()
    }

    fn revision(value: &str) -> RevisionId {
        RevisionId::new(value).unwrap()
    }

    fn actor() -> ActorId {
        ActorId::new("operator-1").unwrap()
    }

    fn at(value: i64) -> UnixMillis {
        UnixMillis::new(value).unwrap()
    }

    fn dependency() -> Dependency {
        Dependency::new(
            DependencyId::new("dependency-1").unwrap(),
            change("downstream"),
            change("upstream"),
            DependencyPins::new(revision("downstream-r1"), revision("upstream-r1")),
            at(1),
            actor(),
        )
        .unwrap()
    }

    #[test]
    fn symmetric_endpoints_are_canonical_and_distinct() {
        let forward = RelationshipEndpoints::new(change("a"), change("b")).unwrap();
        let reverse = RelationshipEndpoints::new(change("b"), change("a")).unwrap();
        assert_eq!(forward, reverse);
        assert_eq!(forward.first().as_str(), "a");
        assert!(forward.contains(&change("b")));
        assert!(matches!(
            RelationshipEndpoints::new(change("a"), change("a")),
            Err(RelationshipError::SelfRelationship)
        ));
    }

    #[test]
    fn relationship_removal_is_versioned_terminal_history() {
        let mut relationship = Relationship::new(
            RelationshipId::new("relationship-1").unwrap(),
            RelationshipKind::RelatedTo,
            RelationshipEndpoints::new(change("a"), change("b")).unwrap(),
            at(1),
            actor(),
        );
        relationship
            .remove(RelationshipVersion::INITIAL, at(2), actor())
            .unwrap();
        assert!(!relationship.is_active());
        assert_eq!(relationship.version(), RelationshipVersion::new(2).unwrap());
        assert!(matches!(
            relationship.remove(RelationshipVersion::new(2).unwrap(), at(3), actor()),
            Err(RelationshipError::AlreadyRemoved)
        ));
    }

    #[test]
    fn dependency_pins_both_exact_revisions_and_derives_freshness() {
        let dependency = dependency();
        assert_eq!(
            dependency.freshness(&revision("downstream-r1"), &revision("upstream-r1")),
            DependencyFreshness::Current
        );
        assert_eq!(
            dependency.freshness(&revision("downstream-r2"), &revision("upstream-r1")),
            DependencyFreshness::DownstreamAdvanced
        );
        assert_eq!(
            dependency.freshness(&revision("downstream-r1"), &revision("upstream-r2")),
            DependencyFreshness::UpstreamAdvanced
        );
        assert_eq!(
            dependency.freshness(&revision("downstream-r2"), &revision("upstream-r2")),
            DependencyFreshness::BothAdvanced
        );
    }

    #[test]
    fn dependency_repin_preserves_edge_and_requires_new_pins() {
        let mut dependency = dependency();
        let unchanged = dependency
            .repin(
                RelationshipVersion::INITIAL,
                dependency.pins().clone(),
                at(2),
                actor(),
            )
            .unwrap_err();
        assert_eq!(unchanged, RelationshipError::UnchangedPins);
        dependency
            .repin(
                RelationshipVersion::INITIAL,
                DependencyPins::new(revision("downstream-r2"), revision("upstream-r2")),
                at(2),
                actor(),
            )
            .unwrap();
        assert_eq!(dependency.version(), RelationshipVersion::new(2).unwrap());
        assert_eq!(
            dependency.pins().upstream_revision_id().as_str(),
            "upstream-r2"
        );
        assert_eq!(dependency.downstream_change_id().as_str(), "downstream");
        assert_eq!(dependency.upstream_change_id().as_str(), "upstream");
    }

    #[test]
    fn dependency_rejects_self_stale_time_reversal_and_repeated_removal() {
        assert!(matches!(
            Dependency::new(
                DependencyId::new("self").unwrap(),
                change("a"),
                change("a"),
                DependencyPins::new(revision("a1"), revision("a1")),
                at(1),
                actor(),
            ),
            Err(RelationshipError::SelfDependency)
        ));
        let mut dependency = dependency();
        assert!(matches!(
            dependency.repin(
                RelationshipVersion::EMPTY,
                DependencyPins::new(revision("d2"), revision("u2")),
                at(2),
                actor(),
            ),
            Err(RelationshipError::StaleVersion { .. })
        ));
        assert_eq!(
            dependency
                .remove(RelationshipVersion::INITIAL, at(0), actor())
                .unwrap_err(),
            RelationshipError::TimestampBeforePriorEvent
        );
        dependency
            .remove(RelationshipVersion::INITIAL, at(2), actor())
            .unwrap();
        assert_eq!(
            dependency
                .remove(RelationshipVersion::new(2).unwrap(), at(3), actor())
                .unwrap_err(),
            RelationshipError::AlreadyRemoved
        );
    }

    #[test]
    fn rejects_invalid_stored_values() {
        assert!(matches!(
            RelationshipId::new(" "),
            Err(RelationshipError::EmptyIdentifier("RelationshipId"))
        ));
        assert!(matches!(
            RelationshipKind::parse("depends_on"),
            Err(RelationshipError::InvalidRelationshipKind(_))
        ));
        assert_eq!(
            RelationshipVersion::new(-1),
            Err(RelationshipError::InvalidVersion)
        );
    }
}
