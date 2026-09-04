use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::{ActorId, ChangeId, UnixMillis};

macro_rules! coordination_id {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(String);

        impl $name {
            /// Creates a non-empty coordination identifier.
            ///
            /// # Errors
            ///
            /// Returns [`CoordinationError::EmptyIdentifier`] for an empty value.
            pub fn new(value: impl Into<String>) -> Result<Self, CoordinationError> {
                let value = value.into();
                if value.trim().is_empty() {
                    return Err(CoordinationError::EmptyIdentifier(stringify!($name)));
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

coordination_id!(AssignmentId);
coordination_id!(SubjectId);
coordination_id!(LeaseId);
coordination_id!(LeaseOperation);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubjectKind {
    Human,
    Agent,
    Session,
    Integration,
}

impl SubjectKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Human => "human",
            Self::Agent => "agent",
            Self::Session => "session",
            Self::Integration => "integration",
        }
    }

    /// Parses a stable storage/API value.
    ///
    /// # Errors
    ///
    /// Returns [`CoordinationError::InvalidSubjectKind`] for unknown values.
    pub fn parse(value: &str) -> Result<Self, CoordinationError> {
        match value {
            "human" => Ok(Self::Human),
            "agent" => Ok(Self::Agent),
            "session" => Ok(Self::Session),
            "integration" => Ok(Self::Integration),
            _ => Err(CoordinationError::InvalidSubjectKind(value.to_owned())),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Subject {
    kind: SubjectKind,
    id: SubjectId,
}

impl Subject {
    #[must_use]
    pub const fn new(kind: SubjectKind, id: SubjectId) -> Self {
        Self { kind, id }
    }

    #[must_use]
    pub const fn kind(&self) -> SubjectKind {
        self.kind
    }

    #[must_use]
    pub const fn id(&self) -> &SubjectId {
        &self.id
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssignmentRole {
    Owner,
    Implementer,
    Reviewer,
    Resolver,
    Integrator,
    Observer,
}

impl AssignmentRole {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Implementer => "implementer",
            Self::Reviewer => "reviewer",
            Self::Resolver => "resolver",
            Self::Integrator => "integrator",
            Self::Observer => "observer",
        }
    }

    /// Parses a stable storage/API value.
    ///
    /// # Errors
    ///
    /// Returns [`CoordinationError::InvalidAssignmentRole`] for unknown values.
    pub fn parse(value: &str) -> Result<Self, CoordinationError> {
        match value {
            "owner" => Ok(Self::Owner),
            "implementer" => Ok(Self::Implementer),
            "reviewer" => Ok(Self::Reviewer),
            "resolver" => Ok(Self::Resolver),
            "integrator" => Ok(Self::Integrator),
            "observer" => Ok(Self::Observer),
            _ => Err(CoordinationError::InvalidAssignmentRole(value.to_owned())),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct CoordinationVersion(i64);

impl CoordinationVersion {
    pub const EMPTY: Self = Self(0);
    pub const INITIAL: Self = Self(1);

    /// Creates a non-negative optimistic-concurrency version.
    ///
    /// # Errors
    ///
    /// Returns [`CoordinationError::InvalidVersion`] for negative values.
    pub const fn new(value: i64) -> Result<Self, CoordinationError> {
        if value < 0 {
            return Err(CoordinationError::InvalidVersion);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub const fn value(self) -> i64 {
        self.0
    }

    fn next(self) -> Result<Self, CoordinationError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or(CoordinationError::VersionExhausted)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Assignment {
    id: AssignmentId,
    change_id: ChangeId,
    subject: Subject,
    role: AssignmentRole,
    assigned_at: UnixMillis,
    assigned_by: ActorId,
    version: CoordinationVersion,
    released_at: Option<UnixMillis>,
    released_by: Option<ActorId>,
}

impl Assignment {
    #[must_use]
    pub const fn new(
        id: AssignmentId,
        change_id: ChangeId,
        subject: Subject,
        role: AssignmentRole,
        assigned_at: UnixMillis,
        assigned_by: ActorId,
    ) -> Self {
        Self {
            id,
            change_id,
            subject,
            role,
            assigned_at,
            assigned_by,
            version: CoordinationVersion::INITIAL,
            released_at: None,
            released_by: None,
        }
    }

    #[must_use]
    pub const fn id(&self) -> &AssignmentId {
        &self.id
    }

    #[must_use]
    pub const fn change_id(&self) -> &ChangeId {
        &self.change_id
    }

    #[must_use]
    pub const fn subject(&self) -> &Subject {
        &self.subject
    }

    #[must_use]
    pub const fn role(&self) -> AssignmentRole {
        self.role
    }

    #[must_use]
    pub const fn assigned_at(&self) -> UnixMillis {
        self.assigned_at
    }

    #[must_use]
    pub const fn assigned_by(&self) -> &ActorId {
        &self.assigned_by
    }

    #[must_use]
    pub const fn version(&self) -> CoordinationVersion {
        self.version
    }

    #[must_use]
    pub const fn released_at(&self) -> Option<UnixMillis> {
        self.released_at
    }

    #[must_use]
    pub const fn released_by(&self) -> Option<&ActorId> {
        self.released_by.as_ref()
    }

    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.released_at.is_none()
    }

    /// Releases an active assignment using an exact version.
    ///
    /// # Errors
    ///
    /// Returns a stale-version, already-released, or timestamp-ordering error.
    pub fn release(
        &mut self,
        expected_version: CoordinationVersion,
        released_at: UnixMillis,
        released_by: ActorId,
    ) -> Result<(), CoordinationError> {
        if self.version != expected_version {
            return Err(CoordinationError::StaleVersion {
                expected: expected_version,
                actual: self.version,
            });
        }
        if !self.is_active() {
            return Err(CoordinationError::AssignmentAlreadyReleased);
        }
        if released_at < self.assigned_at {
            return Err(CoordinationError::TimestampBeforePriorEvent);
        }
        self.version = self.version.next()?;
        self.released_at = Some(released_at);
        self.released_by = Some(released_by);
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LeaseScope {
    change_id: ChangeId,
    operation: LeaseOperation,
}

impl LeaseScope {
    #[must_use]
    pub const fn new(change_id: ChangeId, operation: LeaseOperation) -> Self {
        Self {
            change_id,
            operation,
        }
    }

    #[must_use]
    pub const fn change_id(&self) -> &ChangeId {
        &self.change_id
    }

    #[must_use]
    pub const fn operation(&self) -> &LeaseOperation {
        &self.operation
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LeaseStatus {
    NotYetAcquired,
    Active,
    Expired,
    Released,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Lease {
    id: LeaseId,
    scope: LeaseScope,
    holder: Subject,
    predecessor: Option<LeaseId>,
    acquired_at: UnixMillis,
    expires_at: UnixMillis,
    version: CoordinationVersion,
    released_at: Option<UnixMillis>,
}

impl Lease {
    /// Creates a lease snapshot at the supplied scope version.
    ///
    /// # Errors
    ///
    /// The version must be positive and expiry must be after acquisition.
    pub fn new(
        id: LeaseId,
        scope: LeaseScope,
        holder: Subject,
        predecessor: Option<LeaseId>,
        acquired_at: UnixMillis,
        expires_at: UnixMillis,
        version: CoordinationVersion,
    ) -> Result<Self, CoordinationError> {
        if version == CoordinationVersion::EMPTY {
            return Err(CoordinationError::InvalidVersion);
        }
        if expires_at <= acquired_at {
            return Err(CoordinationError::InvalidLeaseExpiry);
        }
        Ok(Self {
            id,
            scope,
            holder,
            predecessor,
            acquired_at,
            expires_at,
            version,
            released_at: None,
        })
    }

    #[must_use]
    pub const fn id(&self) -> &LeaseId {
        &self.id
    }

    #[must_use]
    pub const fn scope(&self) -> &LeaseScope {
        &self.scope
    }

    #[must_use]
    pub const fn holder(&self) -> &Subject {
        &self.holder
    }

    #[must_use]
    pub const fn predecessor(&self) -> Option<&LeaseId> {
        self.predecessor.as_ref()
    }

    #[must_use]
    pub const fn acquired_at(&self) -> UnixMillis {
        self.acquired_at
    }

    #[must_use]
    pub const fn expires_at(&self) -> UnixMillis {
        self.expires_at
    }

    #[must_use]
    pub const fn version(&self) -> CoordinationVersion {
        self.version
    }

    #[must_use]
    pub const fn released_at(&self) -> Option<UnixMillis> {
        self.released_at
    }

    #[must_use]
    pub fn status_at(&self, at: UnixMillis) -> LeaseStatus {
        if at < self.acquired_at {
            LeaseStatus::NotYetAcquired
        } else if self
            .released_at
            .is_some_and(|released_at| at >= released_at)
        {
            LeaseStatus::Released
        } else if at >= self.expires_at {
            LeaseStatus::Expired
        } else {
            LeaseStatus::Active
        }
    }

    /// Renews an active lease using an exact scope version.
    ///
    /// # Errors
    ///
    /// Returns an error for stale, expired/released, or non-extending requests.
    pub fn renew(
        &mut self,
        expected_version: CoordinationVersion,
        at: UnixMillis,
        new_expires_at: UnixMillis,
    ) -> Result<(), CoordinationError> {
        self.require_current(expected_version, at)?;
        if new_expires_at <= self.expires_at {
            return Err(CoordinationError::LeaseRenewalDoesNotExtend);
        }
        self.version = self.version.next()?;
        self.expires_at = new_expires_at;
        Ok(())
    }

    /// Releases an active lease using an exact scope version.
    ///
    /// # Errors
    ///
    /// Returns an error for stale, expired, released, or time-reversing requests.
    pub fn release(
        &mut self,
        expected_version: CoordinationVersion,
        at: UnixMillis,
    ) -> Result<(), CoordinationError> {
        self.require_current(expected_version, at)?;
        self.version = self.version.next()?;
        self.released_at = Some(at);
        Ok(())
    }

    fn require_current(
        &self,
        expected_version: CoordinationVersion,
        at: UnixMillis,
    ) -> Result<(), CoordinationError> {
        if self.version != expected_version {
            return Err(CoordinationError::StaleVersion {
                expected: expected_version,
                actual: self.version,
            });
        }
        if self.released_at.is_some() {
            return Err(CoordinationError::LeaseReleased);
        }
        if at < self.acquired_at {
            return Err(CoordinationError::TimestampBeforePriorEvent);
        }
        if at >= self.expires_at {
            return Err(CoordinationError::LeaseExpired);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CoordinationError {
    EmptyIdentifier(&'static str),
    InvalidSubjectKind(String),
    InvalidAssignmentRole(String),
    InvalidVersion,
    VersionExhausted,
    StaleVersion {
        expected: CoordinationVersion,
        actual: CoordinationVersion,
    },
    AssignmentAlreadyReleased,
    TimestampBeforePriorEvent,
    InvalidLeaseExpiry,
    LeaseRenewalDoesNotExtend,
    LeaseExpired,
    LeaseReleased,
}

impl Display for CoordinationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyIdentifier(kind) => write!(formatter, "{kind} cannot be empty"),
            Self::InvalidSubjectKind(value) => write!(formatter, "invalid subject kind: {value}"),
            Self::InvalidAssignmentRole(value) => {
                write!(formatter, "invalid assignment role: {value}")
            }
            Self::InvalidVersion => formatter.write_str("coordination version cannot be negative or zero where an active version is required"),
            Self::VersionExhausted => formatter.write_str("coordination version is exhausted"),
            Self::StaleVersion { expected, actual } => write!(
                formatter,
                "stale coordination version: expected {}, actual {}",
                expected.value(),
                actual.value()
            ),
            Self::AssignmentAlreadyReleased => formatter.write_str("assignment is already released"),
            Self::TimestampBeforePriorEvent => formatter.write_str("event timestamp precedes prior durable state"),
            Self::InvalidLeaseExpiry => formatter.write_str("lease expiry must be after acquisition"),
            Self::LeaseRenewalDoesNotExtend => formatter.write_str("lease renewal must extend the current expiry"),
            Self::LeaseExpired => formatter.write_str("lease has expired"),
            Self::LeaseReleased => formatter.write_str("lease has been released"),
        }
    }
}

impl Error for CoordinationError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(value: i64) -> UnixMillis {
        UnixMillis::new(value).unwrap()
    }

    fn actor() -> ActorId {
        ActorId::new("operator-1").unwrap()
    }

    fn subject(id: &str) -> Subject {
        Subject::new(SubjectKind::Agent, SubjectId::new(id).unwrap())
    }

    fn scope() -> LeaseScope {
        LeaseScope::new(
            ChangeId::new("change-1").unwrap(),
            LeaseOperation::new("revision.capture").unwrap(),
        )
    }

    #[test]
    fn assignment_release_is_versioned_and_preserves_identity() {
        let mut assignment = Assignment::new(
            AssignmentId::new("assignment-1").unwrap(),
            ChangeId::new("change-1").unwrap(),
            subject("agent-1"),
            AssignmentRole::Implementer,
            at(10),
            actor(),
        );
        assignment
            .release(CoordinationVersion::INITIAL, at(20), actor())
            .unwrap();

        assert_eq!(assignment.id().as_str(), "assignment-1");
        assert_eq!(assignment.version().value(), 2);
        assert!(!assignment.is_active());
        assert_eq!(assignment.released_at(), Some(at(20)));
    }

    #[test]
    fn assignment_rejects_stale_repeat_and_time_reversal() {
        let new_assignment = || {
            Assignment::new(
                AssignmentId::new("assignment-1").unwrap(),
                ChangeId::new("change-1").unwrap(),
                subject("agent-1"),
                AssignmentRole::Reviewer,
                at(10),
                actor(),
            )
        };
        let mut stale = new_assignment();
        assert!(matches!(
            stale.release(CoordinationVersion::EMPTY, at(20), actor()),
            Err(CoordinationError::StaleVersion { .. })
        ));
        assert!(matches!(
            stale.release(CoordinationVersion::INITIAL, at(9), actor()),
            Err(CoordinationError::TimestampBeforePriorEvent)
        ));
        stale
            .release(CoordinationVersion::INITIAL, at(20), actor())
            .unwrap();
        assert!(matches!(
            stale.release(CoordinationVersion::new(2).unwrap(), at(21), actor()),
            Err(CoordinationError::AssignmentAlreadyReleased)
        ));
    }

    #[test]
    fn lease_renews_releases_and_expires_at_the_exact_boundary() {
        let mut lease = Lease::new(
            LeaseId::new("lease-1").unwrap(),
            scope(),
            subject("agent-1"),
            None,
            at(10),
            at(20),
            CoordinationVersion::INITIAL,
        )
        .unwrap();
        assert_eq!(lease.status_at(at(19)), LeaseStatus::Active);
        assert_eq!(lease.status_at(at(20)), LeaseStatus::Expired);

        lease
            .renew(CoordinationVersion::INITIAL, at(19), at(30))
            .unwrap();
        lease
            .release(CoordinationVersion::new(2).unwrap(), at(25))
            .unwrap();
        assert_eq!(lease.version().value(), 3);
        assert_eq!(lease.status_at(at(9)), LeaseStatus::NotYetAcquired);
        assert_eq!(lease.status_at(at(24)), LeaseStatus::Active);
        assert_eq!(lease.status_at(at(25)), LeaseStatus::Released);
        assert_eq!(lease.status_at(at(30)), LeaseStatus::Released);
    }

    #[test]
    fn lease_rejects_stale_expired_and_non_extending_mutations() {
        let new_lease = || {
            Lease::new(
                LeaseId::new("lease-1").unwrap(),
                scope(),
                subject("agent-1"),
                None,
                at(10),
                at(20),
                CoordinationVersion::INITIAL,
            )
            .unwrap()
        };
        let mut lease = new_lease();
        assert!(matches!(
            lease.renew(CoordinationVersion::EMPTY, at(15), at(30)),
            Err(CoordinationError::StaleVersion { .. })
        ));
        assert!(matches!(
            lease.renew(CoordinationVersion::INITIAL, at(15), at(20)),
            Err(CoordinationError::LeaseRenewalDoesNotExtend)
        ));
        assert!(matches!(
            lease.release(CoordinationVersion::INITIAL, at(20)),
            Err(CoordinationError::LeaseExpired)
        ));
        assert!(matches!(
            lease.release(CoordinationVersion::INITIAL, at(9)),
            Err(CoordinationError::TimestampBeforePriorEvent)
        ));
    }
}
