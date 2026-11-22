//! Deterministic metadata transaction construction and replay, never execution.
use super::{error, json, model::*};
use crate::BridgeError;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub(super) type Result<T> = std::result::Result<T, BridgeError>;
pub(super) fn id(s: &str) -> bool {
    s.len() == 32
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn require(ok: bool) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(error("invalid_input"))
    }
}
fn text(s: &str, max: usize, nonempty: bool) -> Result<()> {
    require(s.len() <= max && (!nonempty || !s.trim().is_empty()) && !s.contains('\0'))
}
fn list(v: &[String]) -> Result<()> {
    require(v.len() <= 16)?;
    for s in v {
        text(s, 512, true)?
    }
    Ok(())
}
pub(super) fn schema(v: u64) -> Result<()> {
    if v == 1 {
        Ok(())
    } else {
        Err(error("unsupported_schema"))
    }
}
pub(super) fn reference(r: &RecordRef) -> Result<()> {
    require(id(&r.id) && r.version > 0 && r.version <= SAFE_INTEGER)
}
pub(super) fn page(limit: u64) -> Result<()> {
    require((1..=256).contains(&limit))
}
pub(super) fn parse<T: DeserializeOwned>(v: Value) -> Result<T> {
    serde_json::from_value(v).map_err(|_| error("invalid_input"))
}
pub(super) fn value(v: &impl Serialize) -> Result<Value> {
    serde_json::to_value(v).map_err(|_| error("invalid_input"))
}
pub(super) fn canonical(v: &impl Serialize) -> Result<String> {
    json::canonical(&value(v)?)
}
fn metadata(m: &ProjectMetadata) -> Result<()> {
    text(&m.title, 1024, true)?;
    require(m.secondary_types.len() <= 4)?;
    let types: BTreeSet<_> = m.secondary_types.iter().collect();
    require(types.len() == m.secondary_types.len())?;
    require(!m.primary_type.as_ref().is_some_and(|p| types.contains(p)))?;
    match (&m.primary_type, &m.type_unknown_reason) {
        (None, Some(s)) => text(s, 512, true),
        (Some(_), None) => Ok(()),
        _ => Err(error("invalid_input")),
    }
}
fn question(q: &QuestionBody) -> Result<()> {
    text(&q.question, 4096, true)?;
    list(&q.objectives)?;
    list(&q.evaluation_questions)?;
    list(&q.constraints)?;
    require(q.hypotheses.len() <= 16)?;
    for h in &q.hypotheses {
        text(&h.text, 512, true)?;
        require(matches!(
            h.timing.as_str(),
            "prespecified" | "post_hoc" | "unknown"
        ))?
    }
    Ok(())
}
fn draft(p: &DraftPlan) -> Result<()> {
    text(&p.objective, 2048, false)?;
    text(&p.methodology, 2048, false)?;
    require(p.milestones.len() <= 6)?;
    let mut ids = BTreeSet::new();
    for s in &p.milestones {
        require(id(&s.id) && ids.insert(&s.id))?;
        require(s.label.chars().count() <= 240)?;
        text(&s.label, 960, true)?;
        text(&s.method, 2048, false)?;
        text(&s.completion_criterion, 512, false)?;
    }
    for l in [
        &p.technical_requirements,
        &p.evidence_criteria,
        &p.risks,
        &p.assumptions,
        &p.required_human_decisions,
        &p.expected_deliverables,
    ] {
        list(l)?
    }
    require(canonical(p)?.len() <= 64 * 1024)
}
pub(super) fn validate(m: &Mutation) -> Result<()> {
    require(id(m.operation_id()))?;
    if let Some(p) = m.project_id() {
        require(id(p))?
    }
    if let Some(v) = m.expected() {
        require(v > 0 && v < SAFE_INTEGER)?
    }
    match m {
        Mutation::Create(a) => {
            schema(a.schema_version)?;
            metadata(&a.metadata)?;
            text(&a.goal, 4096, true)
        }
        Mutation::Update(a) => {
            schema(a.schema_version)?;
            metadata(&a.metadata)
        }
        Mutation::Question(a) => {
            schema(a.schema_version)?;
            reference(&a.previous)?;
            question(&a.body)?;
            text(&a.revision_reason, 512, true)
        }
        Mutation::Plan(a) => {
            schema(a.schema_version)?;
            reference(&a.question)?;
            require(a.question.kind == RecordKind::Question)?;
            if let Some(p) = &a.plan_id {
                require(id(p))?
            }
            if let Some(p) = &a.parent {
                reference(p)?
            }
            require(a.plan_id.is_some() == a.parent.is_some())?;
            draft(&a.body)?;
            text(&a.revision_reason, 512, true)
        }
    }
}

#[derive(Default)]
pub(super) struct State {
    pub s2: super::evidence::EvidenceState,
    pub project: Option<ResearchProject>,
    pub records: BTreeMap<(RecordKind, String, u64), Record>,
    pub operations: BTreeMap<String, Commit>,
    pub events: Vec<ActivityEvent>,
    pub used_ids: BTreeSet<String>,
}
impl State {
    pub fn get(&self, r: &RecordRef) -> Result<&Record> {
        self.records
            .get(&(r.kind, r.id.clone(), r.version))
            .ok_or_else(|| error("not_found"))
    }
    pub fn ingest(&mut self, c: Commit) {
        for r in &c.records {
            let rf = r.reference();
            self.records.insert((rf.kind, rf.id, rf.version), r.clone());
            if let Record::Project(p) = r {
                self.project = Some(p.clone())
            }
        }
        for i in seed_ids(&c.seeds) {
            self.used_ids.insert(i.to_owned());
        }
        self.used_ids.insert(c.project_id.clone());
        self.events.push(c.event.clone());
        self.operations.insert(c.operation_id.clone(), c);
    }
}
fn seed_ids(s: &Seeds) -> [&str; 5] {
    [
        &s.entity_id,
        &s.revision_id,
        &s.activity_id,
        &s.iteration_id,
        &s.event_id,
    ]
}
fn meta(
    owner: &str,
    project: &str,
    rid: &str,
    version: u64,
    old: Option<&Metadata>,
    clock: &Clock,
) -> Metadata {
    Metadata {
        schema_version: 1,
        id: rid.into(),
        project_id: project.into(),
        owner_id: owner.into(),
        version,
        created_at_ms: old.map_or(clock.at_ms, |m| m.created_at_ms),
        updated_at_ms: clock.at_ms,
        clock_status: if clock.at_ms.is_some() {
            "known"
        } else {
            "uncertain"
        }
        .into(),
        clock_reason: clock.reason.clone(),
    }
}

pub(super) fn build(
    owner: &str,
    project_id: &str,
    state: &State,
    m: &Mutation,
    seeds: Seeds,
) -> Result<Commit> {
    validate(m)?;
    require(id(owner) && id(project_id) && owner != project_id)?;
    let ids = seed_ids(&seeds);
    require(
        ids.iter()
            .all(|s| id(s) && *s != owner && *s != project_id && !state.used_ids.contains(*s)),
    )?;
    require(ids.into_iter().collect::<BTreeSet<_>>().len() == 5)?;
    match (&seeds.clock.at_ms, &seeds.clock.reason) {
        (Some(t), None) => require(*t <= SAFE_INTEGER)?,
        (None, Some(r)) => text(r, 512, true)?,
        _ => return Err(error("invalid_input")),
    }
    let seq = state.project.as_ref().map_or(1, |p| p.meta.version + 1);
    require(seq <= SAFE_INTEGER)?;
    let mut records = Vec::new();
    let mut project = if let Mutation::Create(a) = m {
        require(state.project.is_none())?;
        let q = ResearchQuestion {
            meta: meta(owner, project_id, &seeds.entity_id, 1, None, &seeds.clock),
            body: QuestionBody {
                question: a.goal.clone(),
                objectives: vec![],
                hypotheses: vec![],
                evaluation_questions: vec![],
                constraints: vec![],
            },
            previous: None,
            revision_reason: "initial question".into(),
        };
        let qr = Record::Question(q);
        let qref = qr.reference();
        records.push(qr);
        ResearchProject {
            meta: meta(owner, project_id, project_id, 1, None, &seeds.clock),
            metadata: a.metadata.clone(),
            state: "open".into(),
            active_question: qref,
            active_plan: None,
            iteration_id: seeds.iteration_id.clone(),
        }
    } else {
        let old = state.project.as_ref().ok_or_else(|| error("not_found"))?;
        require(m.project_id() == Some(project_id))?;
        if m.expected() != Some(old.meta.version) {
            return Err(error("conflict"));
        }
        let mut p = old.clone();
        p.meta = meta(
            owner,
            project_id,
            project_id,
            seq,
            Some(&old.meta),
            &seeds.clock,
        );
        p
    };
    match m {
        Mutation::Create(_) => {}
        Mutation::Update(a) => project.metadata = a.metadata.clone(),
        Mutation::Question(a) => {
            require(a.previous == project.active_question)?;
            let Record::Question(old) = state.get(&a.previous)? else {
                return Err(error("invalid_input"));
            };
            let q = Record::Question(ResearchQuestion {
                meta: meta(
                    owner,
                    project_id,
                    &old.meta.id,
                    old.meta.version + 1,
                    Some(&old.meta),
                    &seeds.clock,
                ),
                body: a.body.clone(),
                previous: Some(a.previous.clone()),
                revision_reason: a.revision_reason.clone(),
            });
            project.active_question = q.reference();
            project.iteration_id = seeds.iteration_id.clone();
            records.push(q);
        }
        Mutation::Plan(a) => {
            require(matches!(state.get(&a.question)?, Record::Question(_)))?;
            let (plan_id, version, number, oldmeta) = if let Some(pid) = &a.plan_id {
                let previous = a.parent.as_ref().ok_or_else(|| error("invalid_input"))?;
                let Record::PlanRevision(r) = state.get(previous)? else {
                    return Err(error("invalid_input"));
                };
                require(&r.plan_id == pid)?;
                let latest = state
                    .records
                    .values()
                    .filter_map(|v| {
                        if let Record::Plan(p) = v {
                            Some(p)
                        } else {
                            None
                        }
                    })
                    .filter(|p| &p.meta.id == pid)
                    .max_by_key(|p| p.meta.version)
                    .ok_or_else(|| error("not_found"))?;
                require(
                    latest.draft_revision == *previous
                        && !latest.closed
                        && latest.approved_revision.is_none(),
                )?;
                (
                    pid.clone(),
                    latest.meta.version + 1,
                    r.revision_number + 1,
                    Some(&latest.meta),
                )
            } else {
                (seeds.entity_id.clone(), 1, 1, None)
            };
            require(version <= SAFE_INTEGER && number <= SAFE_INTEGER)?;
            let rev = Record::PlanRevision(PlanRevision {
                meta: meta(owner, project_id, &seeds.revision_id, 1, None, &seeds.clock),
                plan_id: plan_id.clone(),
                revision_number: number,
                parent: a.parent.clone(),
                question: a.question.clone(),
                iteration_id: seeds.iteration_id.clone(),
                body: a.body.clone(),
                origin: "local_supplied_draft".into(),
                revision_reason: a.revision_reason.clone(),
                digest: None,
                identity_status: "not_computed".into(),
            });
            let plan = Record::Plan(ResearchPlan {
                meta: meta(owner, project_id, &plan_id, version, oldmeta, &seeds.clock),
                draft_revision: rev.reference(),
                approved_revision: None,
                closed: false,
            });
            project.active_plan = Some(plan.reference());
            project.iteration_id = seeds.iteration_id.clone();
            records.push(rev);
            records.push(plan);
        }
    }
    records.insert(0, Record::Project(project.clone()));
    let targets: Vec<_> = records.iter().map(Record::reference).collect();
    let activity = Record::Activity(ResearchActivity {
        meta: meta(owner, project_id, &seeds.activity_id, 1, None, &seeds.clock),
        activity_kind: "record_only".into(),
        state: "completed".into(),
        operation_id: m.operation_id().into(),
        iteration_id: project.iteration_id.clone(),
        targets: targets.clone(),
    });
    let event = ActivityEvent {
        id: seeds.event_id.clone(),
        project_sequence: seq,
        activity: activity.reference(),
        operation_id: m.operation_id().into(),
        actor_id: owner.into(),
        at_ms: seeds.clock.at_ms,
        kind: "local_metadata_saved".into(),
        targets,
    };
    records.push(activity);
    for r in &records {
        require(canonical(r)?.len() <= 256 * 1024)?
    }
    let result = MutationResult {
        status: "complete".into(),
        operation_id: m.operation_id().into(),
        project_id: project_id.into(),
        project_version: seq,
        commit_sequence: seq,
        records: records.iter().map(Record::reference).collect(),
    };
    let commit = Commit {
        schema_version: 1,
        sequence: seq,
        project_id: project_id.into(),
        seeds,
        operation_id: m.operation_id().into(),
        request: canonical(m)?,
        records,
        event,
        result,
    };
    require(canonical(&commit)?.len() <= MAX_FRAME)?;
    Ok(commit)
}

pub(super) fn replay(owner: &str, pid: &str, bytes: &[u8], state: &mut State) -> Result<()> {
    let v = json::decode(bytes).map_err(|_| error("corrupt_store"))?;
    if v.get("schema_version").and_then(Value::as_u64) != Some(1) {
        return Err(error("unsupported_schema"));
    }
    let c: Commit = parse(v).map_err(|_| error("corrupt_store"))?;
    if canonical(&c)?.as_bytes() != bytes {
        return Err(error("corrupt_store"));
    }
    let m: Mutation =
        parse(json::decode(c.request.as_bytes()).map_err(|_| error("corrupt_store"))?)
            .map_err(|_| error("corrupt_store"))?;
    if state.operations.contains_key(&c.operation_id) {
        return Err(error("corrupt_store"));
    }
    let expected =
        build(owner, pid, state, &m, c.seeds.clone()).map_err(|_| error("corrupt_store"))?;
    if expected != c {
        return Err(error("corrupt_store"));
    }
    state.ingest(c);
    Ok(())
}
