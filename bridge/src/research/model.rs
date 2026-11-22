//! S1 metadata only. No scientific approval, evidence, provider or run authority.
use serde::{Deserialize, Serialize};

pub const SAFE_INTEGER: u64 = 9_007_199_254_740_991;
pub const MAX_FRAME: usize = 1024 * 1024;
pub const MAX_PROJECT: u64 = 512 * 1024 * 1024;

macro_rules! dto {
    ($name:ident { $($field:ident: $ty:ty),* $(,)? }) => {
        #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
        #[serde(deny_unknown_fields)]
        pub struct $name { $(pub $field: $ty),* }
    };
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ResearchType {
    LiteratureReview,
    ExperimentalResearch,
    ComputationalResearch,
    TechnicalFeasibility,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum RecordKind {
    Project,
    Question,
    Plan,
    PlanRevision,
    Activity,
}

dto!(RecordRef {
    kind: RecordKind,
    id: String,
    version: u64
});
dto!(Clock { at_ms: Option<u64>, reason: Option<String> });
dto!(Metadata {
    schema_version: u64, id: String, project_id: String, owner_id: String,
    version: u64, created_at_ms: Option<u64>, updated_at_ms: Option<u64>,
    clock_status: String, clock_reason: Option<String>
});
dto!(ProjectMetadata {
    title: String, primary_type: Option<ResearchType>, type_unknown_reason: Option<String>,
    secondary_types: Vec<ResearchType>
});
dto!(ResearchProject {
    meta: Metadata, metadata: ProjectMetadata, state: String,
    active_question: RecordRef, active_plan: Option<RecordRef>, iteration_id: String
});
dto!(Hypothesis {
    text: String,
    timing: String
});
dto!(QuestionBody {
    question: String, objectives: Vec<String>, hypotheses: Vec<Hypothesis>,
    evaluation_questions: Vec<String>, constraints: Vec<String>
});
dto!(ResearchQuestion {
    meta: Metadata, body: QuestionBody, previous: Option<RecordRef>, revision_reason: String
});
dto!(DraftStep {
    id: String,
    label: String,
    method: String,
    completion_criterion: String,
    included: bool
});
dto!(DraftPlan {
    objective: String, methodology: String, milestones: Vec<DraftStep>,
    technical_requirements: Vec<String>, evidence_criteria: Vec<String>,
    risks: Vec<String>, assumptions: Vec<String>, required_human_decisions: Vec<String>,
    expected_deliverables: Vec<String>
});
dto!(ResearchPlan {
    meta: Metadata, draft_revision: RecordRef, approved_revision: Option<RecordRef>, closed: bool
});
dto!(PlanRevision {
    meta: Metadata, plan_id: String, revision_number: u64, parent: Option<RecordRef>,
    question: RecordRef, iteration_id: String, body: DraftPlan, origin: String,
    revision_reason: String, digest: Option<String>, identity_status: String
});
dto!(ResearchActivity {
    meta: Metadata, activity_kind: String, state: String, operation_id: String,
    iteration_id: String, targets: Vec<RecordRef>
});

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    content = "record",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Record {
    Project(ResearchProject),
    Question(ResearchQuestion),
    Plan(ResearchPlan),
    PlanRevision(PlanRevision),
    Activity(ResearchActivity),
}
impl Record {
    pub fn meta(&self) -> &Metadata {
        match self {
            Self::Project(v) => &v.meta,
            Self::Question(v) => &v.meta,
            Self::Plan(v) => &v.meta,
            Self::PlanRevision(v) => &v.meta,
            Self::Activity(v) => &v.meta,
        }
    }
    pub fn reference(&self) -> RecordRef {
        let kind = match self {
            Self::Project(_) => RecordKind::Project,
            Self::Question(_) => RecordKind::Question,
            Self::Plan(_) => RecordKind::Plan,
            Self::PlanRevision(_) => RecordKind::PlanRevision,
            Self::Activity(_) => RecordKind::Activity,
        };
        RecordRef {
            kind,
            id: self.meta().id.clone(),
            version: self.meta().version,
        }
    }
}
dto!(ActivityEvent {
    id: String, project_sequence: u64, activity: RecordRef, operation_id: String,
    actor_id: String, at_ms: Option<u64>, kind: String, targets: Vec<RecordRef>
});
dto!(MutationResult {
    status: String, operation_id: String, project_id: String, project_version: u64,
    commit_sequence: u64, records: Vec<RecordRef>
});
dto!(Recovery { orphan_files: u64 });
dto!(ProjectView {
    project: ResearchProject,
    recovery: Recovery
});
dto!(ProjectEntry {
    project_id: String, status: String, project: Option<ResearchProject>, reason: Option<String>
});
dto!(ProjectPage { owner_id: Option<String>, items: Vec<ProjectEntry>, next_cursor: Option<String> });
dto!(EventPage { items: Vec<ActivityEvent>, next_cursor: Option<u64> });
dto!(OperationStatus { status: String, project_id: Option<String>, result: Option<MutationResult> });

dto!(CreateArgs {
    schema_version: u64,
    operation_id: String,
    metadata: ProjectMetadata,
    goal: String
});
dto!(UpdateArgs {
    schema_version: u64,
    operation_id: String,
    project_id: String,
    expected_project_version: u64,
    metadata: ProjectMetadata
});
dto!(QuestionArgs {
    schema_version: u64,
    operation_id: String,
    project_id: String,
    expected_project_version: u64,
    previous: RecordRef,
    body: QuestionBody,
    revision_reason: String
});
dto!(PlanArgs {
    schema_version: u64, operation_id: String, project_id: String,
    expected_project_version: u64, plan_id: Option<String>, parent: Option<RecordRef>,
    question: RecordRef, body: DraftPlan, revision_reason: String
});
dto!(ProjectArgs {
    schema_version: u64,
    project_id: String
});
dto!(ListArgs { schema_version: u64, cursor: Option<String>, limit: u64 });
dto!(RecordArgs {
    schema_version: u64,
    project_id: String,
    reference: RecordRef
});
dto!(EventsArgs {
    schema_version: u64,
    project_id: String,
    cursor: u64,
    limit: u64
});
dto!(OperationArgs { schema_version: u64, project_id: Option<String>, operation_id: String });

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "command",
    content = "args",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(crate) enum Mutation {
    Create(CreateArgs),
    Update(UpdateArgs),
    Question(QuestionArgs),
    Plan(PlanArgs),
}
impl Mutation {
    pub fn operation_id(&self) -> &str {
        match self {
            Self::Create(a) => &a.operation_id,
            Self::Update(a) => &a.operation_id,
            Self::Question(a) => &a.operation_id,
            Self::Plan(a) => &a.operation_id,
        }
    }
    pub fn project_id(&self) -> Option<&str> {
        match self {
            Self::Create(_) => None,
            Self::Update(a) => Some(&a.project_id),
            Self::Question(a) => Some(&a.project_id),
            Self::Plan(a) => Some(&a.project_id),
        }
    }
    pub fn expected(&self) -> Option<u64> {
        match self {
            Self::Create(_) => None,
            Self::Update(a) => Some(a.expected_project_version),
            Self::Question(a) => Some(a.expected_project_version),
            Self::Plan(a) => Some(a.expected_project_version),
        }
    }
}
dto!(Seeds {
    entity_id: String,
    revision_id: String,
    activity_id: String,
    iteration_id: String,
    event_id: String,
    clock: Clock
});
dto!(Commit {
    schema_version: u64, sequence: u64, project_id: String, seeds: Seeds,
    operation_id: String, request: String, records: Vec<Record>, event: ActivityEvent,
    result: MutationResult
});
dto!(Owner {
    schema_version: u64,
    owner_id: String
});
dto!(Reservation {
    schema_version: u64,
    initial: Commit
});
dto!(CreationDone {
    schema_version: u64,
    project_id: String,
    operation_id: String
});
