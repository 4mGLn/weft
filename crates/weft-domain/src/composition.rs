use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use sha2::{Digest, Sha256};

use crate::{
    ActorId, BaseState, ChangeId, DependencyId, RelationshipVersion, RevisionId, UnixMillis,
};

const CANDIDATE_MAGIC: &[u8] = b"WEFT-CANDIDATE\0";
pub const COMPOSITION_CANDIDATE_V1: &str = "composition-candidate-v1";

macro_rules! composition_id {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(String);

        impl $name {
            /// Creates a non-empty composition identifier.
            ///
            /// # Errors
            ///
            /// Returns [`CompositionError::EmptyIdentifier`] for an empty value.
            pub fn new(value: impl Into<String>) -> Result<Self, CompositionError> {
                let value = value.into();
                if value.trim().is_empty() {
                    return Err(CompositionError::EmptyIdentifier(stringify!($name)));
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

composition_id!(StackId);
composition_id!(CandidateId);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct StackVersion(i64);

impl StackVersion {
    pub const EMPTY: Self = Self(0);
    pub const INITIAL: Self = Self(1);

    /// Creates a non-negative optimistic-concurrency version.
    ///
    /// # Errors
    ///
    /// Returns [`CompositionError::InvalidVersion`] for a negative value.
    pub const fn new(value: i64) -> Result<Self, CompositionError> {
        if value < 0 {
            return Err(CompositionError::InvalidVersion);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub const fn value(self) -> i64 {
        self.0
    }

    fn next(self) -> Result<Self, CompositionError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or(CompositionError::VersionExhausted)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum StackPolicy {
    OrderOnly,
    PredecessorDependencies,
}

impl StackPolicy {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OrderOnly => "order_only",
            Self::PredecessorDependencies => "predecessor_dependencies",
        }
    }

    /// Parses a stable storage/API value.
    ///
    /// # Errors
    ///
    /// Returns [`CompositionError::InvalidStackPolicy`] for an unknown value.
    pub fn parse(value: &str) -> Result<Self, CompositionError> {
        match value {
            "order_only" => Ok(Self::OrderOnly),
            "predecessor_dependencies" => Ok(Self::PredecessorDependencies),
            _ => Err(CompositionError::InvalidStackPolicy(value.to_owned())),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StackMember {
    change_id: ChangeId,
    predecessor_change_id: Option<ChangeId>,
}

impl StackMember {
    #[must_use]
    pub const fn new(change_id: ChangeId, predecessor_change_id: Option<ChangeId>) -> Self {
        Self {
            change_id,
            predecessor_change_id,
        }
    }

    #[must_use]
    pub const fn change_id(&self) -> &ChangeId {
        &self.change_id
    }

    #[must_use]
    pub const fn predecessor_change_id(&self) -> Option<&ChangeId> {
        self.predecessor_change_id.as_ref()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StackDefinition {
    policy: StackPolicy,
    members: Vec<StackMember>,
}

impl StackDefinition {
    /// Creates a validated explicit predecessor chain.
    ///
    /// # Errors
    ///
    /// Rejects an empty definition, duplicate Changes, or a predecessor that is
    /// not the immediately preceding member.
    pub fn new(policy: StackPolicy, members: Vec<StackMember>) -> Result<Self, CompositionError> {
        if members.is_empty() {
            return Err(CompositionError::EmptyStack);
        }
        let mut seen = HashSet::with_capacity(members.len());
        let mut expected_predecessor: Option<&ChangeId> = None;
        for (position, member) in members.iter().enumerate() {
            if !seen.insert(member.change_id()) {
                return Err(CompositionError::DuplicateStackChange(
                    member.change_id().clone(),
                ));
            }
            if member.predecessor_change_id() != expected_predecessor {
                return Err(CompositionError::InvalidPredecessor { position });
            }
            expected_predecessor = Some(member.change_id());
        }
        Ok(Self { policy, members })
    }

    /// Builds the explicit predecessor chain for an ordered Change list.
    ///
    /// # Errors
    ///
    /// Applies the same validation as [`Self::new`].
    pub fn from_changes(
        policy: StackPolicy,
        changes: Vec<ChangeId>,
    ) -> Result<Self, CompositionError> {
        let mut predecessor = None;
        let members = changes
            .into_iter()
            .map(|change_id| {
                let member = StackMember::new(change_id.clone(), predecessor.take());
                predecessor = Some(change_id);
                member
            })
            .collect();
        Self::new(policy, members)
    }

    #[must_use]
    pub const fn policy(&self) -> StackPolicy {
        self.policy
    }

    #[must_use]
    pub fn members(&self) -> &[StackMember] {
        &self.members
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Stack {
    id: StackId,
    definition: StackDefinition,
    version: StackVersion,
    created_at: UnixMillis,
    created_by: ActorId,
    updated_at: UnixMillis,
    updated_by: ActorId,
}

impl Stack {
    #[must_use]
    pub fn new(
        id: StackId,
        definition: StackDefinition,
        created_at: UnixMillis,
        created_by: ActorId,
    ) -> Self {
        Self {
            id,
            definition,
            version: StackVersion::INITIAL,
            created_at,
            created_by: created_by.clone(),
            updated_at: created_at,
            updated_by: created_by,
        }
    }

    #[must_use]
    pub const fn id(&self) -> &StackId {
        &self.id
    }

    #[must_use]
    pub const fn definition(&self) -> &StackDefinition {
        &self.definition
    }

    #[must_use]
    pub const fn version(&self) -> StackVersion {
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
    pub const fn updated_at(&self) -> UnixMillis {
        self.updated_at
    }

    #[must_use]
    pub const fn updated_by(&self) -> &ActorId {
        &self.updated_by
    }

    /// Replaces a Stack definition with version compare-and-swap.
    ///
    /// # Errors
    ///
    /// Rejects stale writers, no-op replacement, time reversal, and exhausted versions.
    pub fn replace_definition(
        &mut self,
        expected_version: StackVersion,
        definition: StackDefinition,
        updated_at: UnixMillis,
        updated_by: ActorId,
    ) -> Result<(), CompositionError> {
        if self.version != expected_version {
            return Err(CompositionError::StaleStackVersion {
                expected: expected_version,
                actual: self.version,
            });
        }
        if updated_at < self.updated_at {
            return Err(CompositionError::TimestampBeforePriorEvent);
        }
        if definition == self.definition {
            return Err(CompositionError::UnchangedStackDefinition);
        }
        self.version = self.version.next()?;
        self.definition = definition;
        self.updated_at = updated_at;
        self.updated_by = updated_by;
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CandidateInput {
    change_id: ChangeId,
    revision_id: RevisionId,
}

impl CandidateInput {
    #[must_use]
    pub const fn new(change_id: ChangeId, revision_id: RevisionId) -> Self {
        Self {
            change_id,
            revision_id,
        }
    }

    #[must_use]
    pub const fn change_id(&self) -> &ChangeId {
        &self.change_id
    }

    #[must_use]
    pub const fn revision_id(&self) -> &RevisionId {
        &self.revision_id
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateStackRef {
    stack_id: StackId,
    version: StackVersion,
    policy: StackPolicy,
}

impl CandidateStackRef {
    #[must_use]
    pub const fn new(stack_id: StackId, version: StackVersion, policy: StackPolicy) -> Self {
        Self {
            stack_id,
            version,
            policy,
        }
    }

    #[must_use]
    pub const fn stack_id(&self) -> &StackId {
        &self.stack_id
    }

    #[must_use]
    pub const fn version(&self) -> StackVersion {
        self.version
    }

    #[must_use]
    pub const fn policy(&self) -> StackPolicy {
        self.policy
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ResolvedRequirementSource {
    Dependency {
        dependency_id: DependencyId,
        version: RelationshipVersion,
    },
    StackPredecessor {
        stack_id: StackId,
        version: StackVersion,
        downstream_position: usize,
    },
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ResolvedRequirement {
    source: ResolvedRequirementSource,
    downstream: CandidateInput,
    upstream: CandidateInput,
}

impl ResolvedRequirement {
    #[must_use]
    pub const fn new(
        source: ResolvedRequirementSource,
        downstream: CandidateInput,
        upstream: CandidateInput,
    ) -> Self {
        Self {
            source,
            downstream,
            upstream,
        }
    }

    #[must_use]
    pub const fn source(&self) -> &ResolvedRequirementSource {
        &self.source
    }

    #[must_use]
    pub const fn downstream(&self) -> &CandidateInput {
        &self.downstream
    }

    #[must_use]
    pub const fn upstream(&self) -> &CandidateInput {
        &self.upstream
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateDigest(String);

impl CandidateDigest {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompositionCandidate {
    id: CandidateId,
    target_base: BaseState,
    stack: Option<CandidateStackRef>,
    inputs: Vec<CandidateInput>,
    requirements: Vec<ResolvedRequirement>,
    content_digest: CandidateDigest,
    created_at: UnixMillis,
    created_by: ActorId,
}

impl CompositionCandidate {
    /// Creates an immutable exact composition target and its canonical digest.
    ///
    /// # Errors
    ///
    /// Rejects empty/duplicate inputs, invalid requirement endpoints/order or
    /// Stack-predecessor evidence, and fields too large for the v1 encoding.
    pub fn new(
        id: CandidateId,
        target_base: BaseState,
        stack: Option<CandidateStackRef>,
        inputs: Vec<CandidateInput>,
        mut requirements: Vec<ResolvedRequirement>,
        created_at: UnixMillis,
        created_by: ActorId,
    ) -> Result<Self, CompositionError> {
        validate_candidate(stack.as_ref(), &inputs, &requirements)?;
        requirements.sort_unstable();
        if requirements.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(CompositionError::DuplicateRequirement);
        }
        let content_digest =
            digest_candidate(&target_base, stack.as_ref(), &inputs, &requirements)?;
        Ok(Self {
            id,
            target_base,
            stack,
            inputs,
            requirements,
            content_digest,
            created_at,
            created_by,
        })
    }

    #[must_use]
    pub const fn id(&self) -> &CandidateId {
        &self.id
    }

    #[must_use]
    pub const fn target_base(&self) -> &BaseState {
        &self.target_base
    }

    #[must_use]
    pub const fn stack(&self) -> Option<&CandidateStackRef> {
        self.stack.as_ref()
    }

    #[must_use]
    pub fn inputs(&self) -> &[CandidateInput] {
        &self.inputs
    }

    #[must_use]
    pub fn requirements(&self) -> &[ResolvedRequirement] {
        &self.requirements
    }

    #[must_use]
    pub const fn content_digest(&self) -> &CandidateDigest {
        &self.content_digest
    }

    #[must_use]
    pub const fn created_at(&self) -> UnixMillis {
        self.created_at
    }

    #[must_use]
    pub const fn created_by(&self) -> &ActorId {
        &self.created_by
    }
}

fn validate_candidate(
    stack: Option<&CandidateStackRef>,
    inputs: &[CandidateInput],
    requirements: &[ResolvedRequirement],
) -> Result<(), CompositionError> {
    if inputs.is_empty() {
        return Err(CompositionError::EmptyCandidate);
    }
    let mut positions = HashMap::with_capacity(inputs.len());
    for (position, input) in inputs.iter().enumerate() {
        if positions.insert(input.change_id(), position).is_some() {
            return Err(CompositionError::DuplicateCandidateChange(
                input.change_id().clone(),
            ));
        }
    }
    let mut stack_predecessors = vec![false; inputs.len()];
    for requirement in requirements {
        let Some(&downstream_position) = positions.get(requirement.downstream.change_id()) else {
            return Err(CompositionError::RequirementInputMissing(
                requirement.downstream.change_id().clone(),
            ));
        };
        let Some(&upstream_position) = positions.get(requirement.upstream.change_id()) else {
            return Err(CompositionError::RequirementInputMissing(
                requirement.upstream.change_id().clone(),
            ));
        };
        if &inputs[downstream_position] != requirement.downstream()
            || &inputs[upstream_position] != requirement.upstream()
        {
            return Err(CompositionError::RequirementRevisionMismatch);
        }
        if upstream_position >= downstream_position {
            return Err(CompositionError::RequirementOrderInvalid);
        }
        if let ResolvedRequirementSource::StackPredecessor {
            stack_id,
            version,
            downstream_position: source_position,
        } = requirement.source()
        {
            let Some(stack) = stack else {
                return Err(CompositionError::StackRequirementWithoutStack);
            };
            if stack.policy != StackPolicy::PredecessorDependencies
                || stack.stack_id != *stack_id
                || stack.version != *version
                || downstream_position != *source_position
                || upstream_position.checked_add(1) != Some(downstream_position)
            {
                return Err(CompositionError::InvalidStackRequirement);
            }
            if stack_predecessors[downstream_position] {
                return Err(CompositionError::DuplicateRequirement);
            }
            stack_predecessors[downstream_position] = true;
        }
    }
    if stack.is_some_and(|value| value.policy == StackPolicy::PredecessorDependencies)
        && stack_predecessors
            .iter()
            .enumerate()
            .any(|(position, present)| position > 0 && !present)
    {
        return Err(CompositionError::MissingStackRequirement);
    }
    Ok(())
}

fn digest_candidate(
    target_base: &BaseState,
    stack: Option<&CandidateStackRef>,
    inputs: &[CandidateInput],
    requirements: &[ResolvedRequirement],
) -> Result<CandidateDigest, CompositionError> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(CANDIDATE_MAGIC);
    encode_string(&mut bytes, COMPOSITION_CANDIDATE_V1)?;
    encode_string(&mut bytes, target_base.repository_id().as_str())?;
    encode_string(&mut bytes, target_base.object_id())?;
    match stack {
        Some(stack) => {
            bytes.push(1);
            encode_string(&mut bytes, stack.stack_id.as_str())?;
            bytes.extend_from_slice(&stack.version.value().to_be_bytes());
            encode_string(&mut bytes, stack.policy.as_str())?;
        }
        None => bytes.push(0),
    }
    encode_len(&mut bytes, inputs.len())?;
    for input in inputs {
        encode_input(&mut bytes, input)?;
    }
    encode_len(&mut bytes, requirements.len())?;
    for requirement in requirements {
        match requirement.source() {
            ResolvedRequirementSource::Dependency {
                dependency_id,
                version,
            } => {
                bytes.push(0);
                encode_string(&mut bytes, dependency_id.as_str())?;
                bytes.extend_from_slice(&version.value().to_be_bytes());
            }
            ResolvedRequirementSource::StackPredecessor {
                stack_id,
                version,
                downstream_position,
            } => {
                bytes.push(1);
                encode_string(&mut bytes, stack_id.as_str())?;
                bytes.extend_from_slice(&version.value().to_be_bytes());
                encode_len(&mut bytes, *downstream_position)?;
            }
        }
        encode_input(&mut bytes, requirement.downstream())?;
        encode_input(&mut bytes, requirement.upstream())?;
    }
    Ok(CandidateDigest(format!(
        "sha256:{:x}",
        Sha256::digest(bytes)
    )))
}

fn encode_input(bytes: &mut Vec<u8>, input: &CandidateInput) -> Result<(), CompositionError> {
    encode_string(bytes, input.change_id().as_str())?;
    encode_string(bytes, input.revision_id().as_str())
}

fn encode_string(bytes: &mut Vec<u8>, value: &str) -> Result<(), CompositionError> {
    encode_len(bytes, value.len())?;
    bytes.extend_from_slice(value.as_bytes());
    Ok(())
}

fn encode_len(bytes: &mut Vec<u8>, value: usize) -> Result<(), CompositionError> {
    let encoded = u32::try_from(value).map_err(|_| CompositionError::EncodingTooLarge)?;
    bytes.extend_from_slice(&encoded.to_be_bytes());
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompositionError {
    EmptyIdentifier(&'static str),
    InvalidVersion,
    VersionExhausted,
    InvalidStackPolicy(String),
    EmptyStack,
    DuplicateStackChange(ChangeId),
    InvalidPredecessor {
        position: usize,
    },
    StaleStackVersion {
        expected: StackVersion,
        actual: StackVersion,
    },
    TimestampBeforePriorEvent,
    UnchangedStackDefinition,
    EmptyCandidate,
    DuplicateCandidateChange(ChangeId),
    RequirementInputMissing(ChangeId),
    RequirementRevisionMismatch,
    RequirementOrderInvalid,
    StackRequirementWithoutStack,
    InvalidStackRequirement,
    MissingStackRequirement,
    DuplicateRequirement,
    EncodingTooLarge,
}

impl Display for CompositionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyIdentifier(kind) => write!(formatter, "{kind} cannot be empty"),
            Self::InvalidVersion => formatter.write_str("Stack version cannot be negative"),
            Self::VersionExhausted => formatter.write_str("Stack version is exhausted"),
            Self::InvalidStackPolicy(value) => write!(formatter, "invalid Stack policy: {value}"),
            Self::EmptyStack => formatter.write_str("a Stack requires at least one Change"),
            Self::DuplicateStackChange(id) => {
                write!(formatter, "duplicate Stack Change: {}", id.as_str())
            }
            Self::InvalidPredecessor { position } => {
                write!(
                    formatter,
                    "invalid Stack predecessor at position {position}"
                )
            }
            Self::StaleStackVersion { expected, actual } => write!(
                formatter,
                "stale Stack version: expected {}, actual {}",
                expected.value(),
                actual.value()
            ),
            Self::TimestampBeforePriorEvent => {
                formatter.write_str("Stack event precedes prior history")
            }
            Self::UnchangedStackDefinition => {
                formatter.write_str("Stack replacement must change its definition")
            }
            Self::EmptyCandidate => {
                formatter.write_str("a CompositionCandidate requires at least one input")
            }
            Self::DuplicateCandidateChange(id) => {
                write!(formatter, "duplicate candidate Change: {}", id.as_str())
            }
            Self::RequirementInputMissing(id) => write!(
                formatter,
                "resolved requirement references absent Change: {}",
                id.as_str()
            ),
            Self::RequirementRevisionMismatch => {
                formatter.write_str("resolved requirement revision differs from candidate input")
            }
            Self::RequirementOrderInvalid => {
                formatter.write_str("required upstream input must appear before downstream")
            }
            Self::StackRequirementWithoutStack => {
                formatter.write_str("Stack-predecessor requirement needs an exact Stack snapshot")
            }
            Self::InvalidStackRequirement => {
                formatter.write_str("Stack-predecessor requirement does not match exact topology")
            }
            Self::MissingStackRequirement => formatter
                .write_str("predecessor-dependencies candidate requires every direct predecessor"),
            Self::DuplicateRequirement => {
                formatter.write_str("candidate contains a duplicate resolved requirement")
            }
            Self::EncodingTooLarge => {
                formatter.write_str("candidate field exceeds composition-candidate-v1 limits")
            }
        }
    }
}

impl Error for CompositionError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RepositoryId;

    fn change(value: &str) -> ChangeId {
        ChangeId::new(value).unwrap()
    }

    fn revision(value: &str) -> RevisionId {
        RevisionId::new(value).unwrap()
    }

    fn input(change_id: &str, revision_id: &str) -> CandidateInput {
        CandidateInput::new(change(change_id), revision(revision_id))
    }

    fn actor() -> ActorId {
        ActorId::new("operator-1").unwrap()
    }

    fn at(value: i64) -> UnixMillis {
        UnixMillis::new(value).unwrap()
    }

    fn base() -> BaseState {
        BaseState::new(RepositoryId::new("repo-1").unwrap(), "target-1").unwrap()
    }

    #[test]
    fn stack_is_nonempty_duplicate_free_explicit_chain() {
        let definition = StackDefinition::from_changes(
            StackPolicy::OrderOnly,
            vec![change("a"), change("b"), change("c")],
        )
        .unwrap();
        assert_eq!(
            definition.members()[1].predecessor_change_id(),
            Some(&change("a"))
        );
        assert!(matches!(
            StackDefinition::from_changes(StackPolicy::OrderOnly, Vec::new()),
            Err(CompositionError::EmptyStack)
        ));
        assert!(matches!(
            StackDefinition::from_changes(StackPolicy::OrderOnly, vec![change("a"), change("a")]),
            Err(CompositionError::DuplicateStackChange(_))
        ));
    }

    #[test]
    fn stack_replacement_uses_version_cas_and_rejects_noop() {
        let original =
            StackDefinition::from_changes(StackPolicy::OrderOnly, vec![change("a")]).unwrap();
        let mut stack = Stack::new(
            StackId::new("stack-1").unwrap(),
            original.clone(),
            at(1),
            actor(),
        );
        assert_eq!(
            stack.replace_definition(StackVersion::INITIAL, original, at(2), actor()),
            Err(CompositionError::UnchangedStackDefinition)
        );
        let replacement = StackDefinition::from_changes(
            StackPolicy::PredecessorDependencies,
            vec![change("a"), change("b")],
        )
        .unwrap();
        stack
            .replace_definition(StackVersion::INITIAL, replacement, at(2), actor())
            .unwrap();
        assert_eq!(stack.version(), StackVersion::new(2).unwrap());
        assert!(matches!(
            stack.replace_definition(
                StackVersion::INITIAL,
                StackDefinition::from_changes(StackPolicy::OrderOnly, vec![change("b")]).unwrap(),
                at(3),
                actor()
            ),
            Err(CompositionError::StaleStackVersion { .. })
        ));
    }

    #[test]
    fn candidate_digest_is_identity_independent_and_requirement_order_independent() {
        let inputs = vec![input("a", "a-r1"), input("b", "b-r1"), input("c", "c-r1")];
        let first = ResolvedRequirement::new(
            ResolvedRequirementSource::Dependency {
                dependency_id: DependencyId::new("dependency-2").unwrap(),
                version: RelationshipVersion::INITIAL,
            },
            inputs[2].clone(),
            inputs[0].clone(),
        );
        let second = ResolvedRequirement::new(
            ResolvedRequirementSource::Dependency {
                dependency_id: DependencyId::new("dependency-1").unwrap(),
                version: RelationshipVersion::new(2).unwrap(),
            },
            inputs[2].clone(),
            inputs[1].clone(),
        );
        let left = CompositionCandidate::new(
            CandidateId::new("candidate-1").unwrap(),
            base(),
            None,
            inputs.clone(),
            vec![first.clone(), second.clone()],
            at(1),
            actor(),
        )
        .unwrap();
        let right = CompositionCandidate::new(
            CandidateId::new("candidate-2").unwrap(),
            base(),
            None,
            inputs,
            vec![second, first],
            at(99),
            ActorId::new("other").unwrap(),
        )
        .unwrap();
        assert_eq!(left.content_digest(), right.content_digest());
        assert!(left.content_digest().as_str().starts_with("sha256:"));
    }

    #[test]
    fn candidate_digest_changes_with_correctness_inputs() {
        let first = CompositionCandidate::new(
            CandidateId::new("candidate-1").unwrap(),
            base(),
            None,
            vec![input("a", "a-r1")],
            Vec::new(),
            at(1),
            actor(),
        )
        .unwrap();
        let second = CompositionCandidate::new(
            CandidateId::new("candidate-2").unwrap(),
            base(),
            None,
            vec![input("a", "a-r2")],
            Vec::new(),
            at(1),
            actor(),
        )
        .unwrap();
        assert_ne!(first.content_digest(), second.content_digest());
    }

    #[test]
    fn exact_requirements_must_exist_and_point_backwards() {
        let inputs = vec![input("a", "a-r1"), input("b", "b-r1")];
        let reversed = ResolvedRequirement::new(
            ResolvedRequirementSource::Dependency {
                dependency_id: DependencyId::new("dependency-1").unwrap(),
                version: RelationshipVersion::INITIAL,
            },
            inputs[0].clone(),
            inputs[1].clone(),
        );
        assert_eq!(
            CompositionCandidate::new(
                CandidateId::new("candidate-1").unwrap(),
                base(),
                None,
                inputs,
                vec![reversed],
                at(1),
                actor()
            ),
            Err(CompositionError::RequirementOrderInvalid)
        );
    }

    #[test]
    fn stack_predecessor_requirement_matches_exact_snapshot() {
        let inputs = vec![input("a", "a-r1"), input("b", "b-r1")];
        let stack = CandidateStackRef::new(
            StackId::new("stack-1").unwrap(),
            StackVersion::new(3).unwrap(),
            StackPolicy::PredecessorDependencies,
        );
        let requirement = ResolvedRequirement::new(
            ResolvedRequirementSource::StackPredecessor {
                stack_id: stack.stack_id().clone(),
                version: stack.version(),
                downstream_position: 1,
            },
            inputs[1].clone(),
            inputs[0].clone(),
        );
        CompositionCandidate::new(
            CandidateId::new("candidate-1").unwrap(),
            base(),
            Some(stack),
            inputs,
            vec![requirement],
            at(1),
            actor(),
        )
        .unwrap();
    }

    #[test]
    fn predecessor_policy_requires_every_direct_predecessor() {
        let inputs = vec![input("a", "a-r1"), input("b", "b-r1"), input("c", "c-r1")];
        let stack = CandidateStackRef::new(
            StackId::new("stack-1").unwrap(),
            StackVersion::INITIAL,
            StackPolicy::PredecessorDependencies,
        );
        let only_second = ResolvedRequirement::new(
            ResolvedRequirementSource::StackPredecessor {
                stack_id: stack.stack_id().clone(),
                version: stack.version(),
                downstream_position: 1,
            },
            inputs[1].clone(),
            inputs[0].clone(),
        );
        assert_eq!(
            CompositionCandidate::new(
                CandidateId::new("candidate-1").unwrap(),
                base(),
                Some(stack),
                inputs,
                vec![only_second],
                at(1),
                actor(),
            ),
            Err(CompositionError::MissingStackRequirement)
        );
    }
}
