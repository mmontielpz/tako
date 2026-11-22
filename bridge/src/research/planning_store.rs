//! Separate schema-3 journal using the existing owner/project locks and safe files.
use super::*;
use crate::research::{evidence as e, planning as p};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;
#[cfg(test)]
#[path = "planning_store_tests.rs"]
mod tests;

macro_rules! dto {
    ($name:ident { $($field:ident: $ty:ty),* $(,)? }) => {
        #[derive(Debug,Clone,Serialize,Deserialize,PartialEq,Eq)]
        #[serde(deny_unknown_fields)]
        struct $name { $($field:$ty),* }
    };
}
dto!(Frame {schema_version:u64,owner_id:String,project_id:String,sequence:u64,
    legacy_project_version:u64,operation_id:String,request:String,seeds:Seeds,
    revision:Option<p::PlanRevision>,event:p::PlanLifecycleEvent,result:Value});
dto!(Done {
    schema_version: u64,
    operation_id: String,
    sequence: u64,
    digest: p::Hash
});
#[derive(Default)]
struct Plans {
    frames: BTreeMap<String, Frame>,
    revisions: BTreeMap<String, p::PlanRevision>,
    events: Vec<p::PlanLifecycleEvent>,
    states: BTreeMap<String, String>,
    ids: BTreeSet<String>,
    pending: Vec<Frame>,
    sequence: u64,
}
fn area(ctx: &Context, pid: &str, name: &str, create: bool) -> Result<Dir> {
    ctx.research
        .child(name, create)?
        .child("v3", create)?
        .child(&ctx.owner.owner_id, create)?
        .child(pid, create)
}
pub(super) fn usage(ctx: &Context, pid: &str) -> Result<u64> {
    let mut total = 0u64;
    for name in ["s3", "s3-receipts"] {
        let d = match area(ctx, pid, name, false) {
            Ok(d) => d,
            Err(err) if err.code == "research_not_found" => continue,
            Err(err) => return Err(err),
        };
        let files = if name == "s3" {
            p::require(d.entries()?.iter().all(|n| n == "commits"))?;
            match d.child("commits", false) {
                Ok(d) => d,
                Err(err) if err.code == "research_not_found" => continue,
                Err(err) => return Err(err),
            }
        } else {
            d
        };
        for n in files.entries()? {
            total = total
                .checked_add(
                    files
                        .file(&n, MAX_FRAME as u64)?
                        .metadata()
                        .map_err(io_error)?
                        .len(),
                )
                .ok_or_else(|| error("storage_limit"))?;
        }
    }
    Ok(total)
}
fn read<T: serde::de::DeserializeOwned + Serialize>(d: &Dir, name: &str) -> Result<T> {
    let bytes = d.bytes(name, MAX_FRAME)?;
    let value = json::decode(&bytes).map_err(|_| error("corrupt_store"))?;
    if value["schema_version"] != 3 {
        return Err(error("unsupported_schema"));
    }
    let typed: T = state::parse(value).map_err(|_| error("corrupt_store"))?;
    if state::canonical(&typed)?.as_bytes() != bytes {
        return Err(error("corrupt_store"));
    }
    d.file(name, MAX_FRAME as u64)?
        .sync_all()
        .map_err(io_error)?;
    d.0.sync_all().map_err(io_error)?;
    Ok(typed)
}
fn optional_area(ctx: &Context, pid: &str, name: &str) -> Result<Option<Dir>> {
    match area(ctx, pid, name, false) {
        Ok(d) => Ok(Some(d)),
        Err(err) if err.code == "research_not_found" => Ok(None),
        Err(err) => Err(err),
    }
}
fn old_ref(r: &p::Reference) -> Result<e::Ref> {
    state::parse(state::value(r)?)
}
fn record_hash(v: &Value) -> Result<p::Hash> {
    p::hash(v, "TAKO-RESEARCH-INPUT-v1")
}
fn material(project: &Dir, legacy: &State, r: &p::Reference) -> Result<p::Material> {
    let record = e::get(legacy, &old_ref(r)?)?;
    let inspect = v2::inspection(project, legacy, &record);
    Ok(p::Material {
        reference: r.clone(),
        record_digest: record_hash(&record)?,
        integrity: inspect["integrity"]
            .as_str()
            .unwrap_or("not_checked")
            .into(),
        availability: inspect["availability"].as_str().map(String::from),
        content_state: if r.kind == "source" {
            "metadata_only"
        } else {
            "supplied_note"
        }
        .into(),
        independent_verification: "not_established".into(),
        scientific_assessment: "not_assessed".into(),
    })
}
fn materials(project: &Dir, legacy: &State, input: &p::PlanInput) -> Result<Vec<p::Material>> {
    input
        .selected_evidence
        .iter()
        .map(|r| material(project, legacy, r))
        .collect()
}
fn validate_materials(legacy: &State, input: &p::PlanInput, items: &[p::Material]) -> Result<()> {
    p::require(items.len() == input.selected_evidence.len())?;
    for (r, m) in input.selected_evidence.iter().zip(items) {
        p::require(
            &m.reference == r && m.record_digest == record_hash(&e::get(legacy, &old_ref(r)?)?)?,
        )?;
        p::require(matches!(
            m.integrity.as_str(),
            "not_checked" | "matches" | "mismatch" | "unavailable"
        ))?;
        p::require(
            m.availability
                .as_ref()
                .map_or(true, |s| matches!(s.as_str(), "available" | "unavailable")),
        )?;
        p::require(
            m.content_state
                == if r.kind == "source" {
                    "metadata_only"
                } else {
                    "supplied_note"
                },
        )?;
        p::require(
            m.independent_verification == "not_established"
                && m.scientific_assessment == "not_assessed",
        )?;
    }
    Ok(())
}
fn version(legacy: u64, s3: u64) -> Result<u64> {
    legacy
        .checked_add(s3)
        .filter(|n| *n < SAFE_INTEGER)
        .ok_or_else(|| error("storage_limit"))
}
fn latest<'a>(s: &'a Plans, plan: &str) -> Result<&'a p::PlanRevision> {
    s.revisions
        .values()
        .filter(|r| r.plan_id == plan)
        .max_by_key(|r| r.revision_number)
        .ok_or_else(|| error("not_found"))
}
fn target<'a>(s: &'a Plans, plan: &str, revision: &str) -> Result<&'a p::PlanRevision> {
    s.revisions
        .get(revision)
        .filter(|r| r.plan_id == plan)
        .ok_or_else(|| error("not_found"))
}
fn write_scope(op: &str, v: u64) -> Result<()> {
    p::require(state::id(op) && v > 0 && v < SAFE_INTEGER)
}
fn validate_args(cmd: &str, args: Value) -> Result<()> {
    match cmd {
        "create_plan_draft" | "create_plan_revision" => {
            let a: p::Write = state::parse(args)?;
            p::scope(a.schema_version, &a.project_id)?;
            write_scope(&a.operation_id, a.expected_project_version)?;
            p::input(&a.input)?;
            p::text(&a.revision_reason, 512, true)?;
            if cmd == "create_plan_draft" {
                p::require(a.plan_id.is_none() && a.parent_revision_id.is_none())?;
            } else {
                p::require(
                    a.plan_id.as_ref().is_some_and(|n| state::id(n))
                        && a.parent_revision_id.as_ref().is_some_and(|n| state::id(n)),
                )?;
            }
        }
        "submit_plan_for_review" => {
            let a: p::Target = state::parse(args)?;
            p::scope(a.schema_version, &a.project_id)?;
            write_scope(&a.operation_id, a.expected_project_version)?;
            p::require(state::id(&a.plan_id) && state::id(&a.revision_id))?;
            p::digest_valid(&a.expected_revision_digest)?;
        }
        "approve_plan_revision" | "reject_plan_revision" => {
            let a: p::Decision = state::parse(args)?;
            p::scope(a.schema_version, &a.project_id)?;
            write_scope(&a.operation_id, a.expected_project_version)?;
            p::require(
                state::id(&a.plan_id) && state::id(&a.revision_id) && a.human_review_intent,
            )?;
            p::digest_valid(&a.expected_revision_digest)?;
            p::text(&a.review_rationale, 512, true)?;
        }
        "read_plan" => {
            let a: p::Read = state::parse(args)?;
            p::scope(a.schema_version, &a.project_id)?;
            p::require(
                state::id(&a.plan_id) && a.revision_id.as_ref().map_or(true, |n| state::id(n)),
            )?;
        }
        "list_plan_revisions" | "read_plan_review_history" => {
            let a: p::Page = state::parse(args)?;
            p::scope(a.schema_version, &a.project_id)?;
            p::require(state::id(&a.plan_id) && a.cursor <= SAFE_INTEGER)?;
            state::page(a.limit)?;
        }
        "validate_plan_revision" => {
            let a: p::Validate = state::parse(args)?;
            p::scope(a.schema_version, &a.project_id)?;
            p::require(
                (a.proposal_json.is_some() && a.plan_id.is_none() && a.revision_id.is_none())
                    || (a.proposal_json.is_none()
                        && a.plan_id.as_ref().is_some_and(|n| state::id(n))
                        && a.revision_id.as_ref().is_some_and(|n| state::id(n))),
            )?;
            if let Some(raw) = a.proposal_json {
                p::proposal(&raw)?;
            }
        }
        _ => return Err(error("invalid_input")),
    }
    Ok(())
}
fn build(
    ctx: &Context,
    pid: &str,
    legacy: &State,
    s: &Plans,
    cmd: &str,
    args: Value,
    seeds: Seeds,
    observed: u64,
    snapshot: Vec<p::Material>,
) -> Result<Frame> {
    validate_args(cmd, args.clone())?;
    let current = version(observed, s.sequence)?;
    for id in [
        &seeds.entity_id,
        &seeds.revision_id,
        &seeds.iteration_id,
        &seeds.activity_id,
        &seeds.event_id,
    ] {
        p::require(
            state::id(id)
                && !legacy.used_ids.contains(id)
                && !s.ids.contains(id)
                && id != &ctx.owner.owner_id
                && id != pid,
        )?;
    }
    p::require(
        [
            &seeds.entity_id,
            &seeds.revision_id,
            &seeds.iteration_id,
            &seeds.activity_id,
            &seeds.event_id,
        ]
        .into_iter()
        .collect::<BTreeSet<_>>()
        .len()
            == 5,
    )?;
    match (&seeds.clock.at_ms, &seeds.clock.reason) {
        (Some(t), None) => p::require(*t <= SAFE_INTEGER)?,
        (None, Some(reason)) => p::text(reason, 512, true)?,
        _ => return Err(error("invalid_input")),
    }
    let sequence = s
        .sequence
        .checked_add(1)
        .filter(|n| *n < SAFE_INTEGER)
        .ok_or_else(|| error("storage_limit"))?;
    let op;
    let revision;
    let event_kind;
    let reviewed;
    if matches!(cmd, "create_plan_draft" | "create_plan_revision") {
        let a: p::Write = state::parse(args.clone())?;
        if a.expected_project_version != current {
            return Err(error("conflict"));
        }
        let (plan_id, number) = if let Some(id) = &a.plan_id {
            let last = latest(s, id)?;
            if Some(&last.revision_id) != a.parent_revision_id.as_ref() {
                return Err(error("conflict"));
            }
            (
                id.clone(),
                last.revision_number
                    .checked_add(1)
                    .filter(|n| *n < SAFE_INTEGER)
                    .ok_or_else(|| error("storage_limit"))?,
            )
        } else {
            (seeds.entity_id.clone(), 1)
        };
        let question = e::get(legacy, &old_ref(&a.input.question)?)?;
        validate_materials(legacy, &a.input, &snapshot)?;
        let payload = p::Payload {
            input: a.input,
            question_digest: record_hash(&question)?,
            materials: snapshot,
        };
        let r = p::PlanRevision {
            schema_version: 3,
            record_id: seeds.revision_id.clone(),
            project_id: pid.into(),
            plan_id,
            revision_id: seeds.revision_id.clone(),
            revision_number: number,
            created_at: seeds.clock.at_ms,
            clock_reason: seeds.clock.reason.clone(),
            payload_digest: p::hash(&payload, "TAKO-RESEARCH-PLAN-v1")?,
            parent_revision_id: a.parent_revision_id,
            payload,
        };
        event_kind = if cmd == "create_plan_draft" {
            "CREATED"
        } else {
            "REVISED"
        };
        reviewed = r.clone();
        revision = Some(r);
        op = a.operation_id;
    } else if cmd == "submit_plan_for_review" {
        let a: p::Target = state::parse(args.clone())?;
        if a.expected_project_version != current {
            return Err(error("conflict"));
        }
        let r = target(s, &a.plan_id, &a.revision_id)?;
        if r.payload_digest != a.expected_revision_digest
            || latest(s, &a.plan_id)?.revision_id != r.revision_id
        {
            return Err(error("conflict"));
        }
        p::require(s.states.get(&r.revision_id).map(String::as_str) == Some("DRAFT"))?;
        p::require(p::review(&r.payload.input).reviewable)?;
        reviewed = r.clone();
        revision = None;
        op = a.operation_id;
        event_kind = "SUBMITTED_REVIEW";
        p::require(snapshot.is_empty())?;
    } else {
        return Err(error("review_authority_unavailable"));
    }
    let event = p::PlanLifecycleEvent {
        schema_version: 3,
        record_id: seeds.event_id.clone(),
        project_id: pid.into(),
        plan_id: reviewed.plan_id.clone(),
        revision_id: reviewed.revision_id.clone(),
        revision_number: reviewed.revision_number,
        created_at: seeds.clock.at_ms,
        clock_reason: seeds.clock.reason.clone(),
        payload_digest: p::event_digest(event_kind, &reviewed)?,
        sequence,
        operation_id: op.clone(),
        kind: event_kind.into(),
        revision_digest: reviewed.payload_digest.clone(),
    };
    let result = json!({"status":"complete","schema_version":3,"project_id":pid,"operation_id":op,
        "plan_id":reviewed.plan_id,"revision_id":reviewed.revision_id,"revision_digest":reviewed.payload_digest,
        "project_version":version(observed,sequence)?,"legacy_project_version":observed,"s3_sequence":sequence,
        "review_state":if event_kind=="SUBMITTED_REVIEW"{"REVIEW_REQUIRED"}else{"DRAFT"},"event_id":event.record_id,
        "native_review_authority":"unavailable"});
    let frame = Frame {
        schema_version: 3,
        owner_id: ctx.owner.owner_id.clone(),
        project_id: pid.into(),
        sequence,
        legacy_project_version: observed,
        operation_id: op,
        request: state::canonical(&json!({"command":cmd,"args":args}))?,
        seeds,
        revision,
        event,
        result,
    };
    p::require(state::canonical(&frame)?.len() <= MAX_FRAME)?;
    Ok(frame)
}
fn ingest(s: &mut Plans, f: Frame) {
    if let Some(r) = &f.revision {
        s.revisions.insert(r.revision_id.clone(), r.clone());
    }
    s.states.insert(
        f.event.revision_id.clone(),
        if f.event.kind == "SUBMITTED_REVIEW" {
            "REVIEW_REQUIRED"
        } else {
            "DRAFT"
        }
        .into(),
    );
    s.events.push(f.event.clone());
    s.sequence = f.sequence;
    for id in [
        &f.seeds.entity_id,
        &f.seeds.revision_id,
        &f.seeds.iteration_id,
        &f.seeds.activity_id,
        &f.seeds.event_id,
    ] {
        s.ids.insert(id.clone());
    }
    s.frames.insert(f.operation_id.clone(), f);
}
fn verify(ctx: &Context, pid: &str, legacy: &State, s: &Plans, f: &Frame) -> Result<()> {
    p::require(
        f.schema_version == 3
            && f.owner_id == ctx.owner.owner_id
            && f.project_id == pid
            && f.sequence == s.sequence + 1
            && f.legacy_project_version > 0
            && f.legacy_project_version <= legacy.project.as_ref().unwrap().meta.version
            && !s.frames.contains_key(&f.operation_id),
    )?;
    let req = json::decode(f.request.as_bytes())?;
    p::require(
        req.as_object()
            .is_some_and(|v| v.len() == 2 && v.contains_key("command") && v.contains_key("args")),
    )?;
    let rebuilt = build(
        ctx,
        pid,
        legacy,
        s,
        req["command"]
            .as_str()
            .ok_or_else(|| error("corrupt_store"))?,
        req["args"].clone(),
        f.seeds.clone(),
        f.legacy_project_version,
        f.revision
            .as_ref()
            .map(|r| r.payload.materials.clone())
            .unwrap_or_default(),
    )?;
    p::require(&rebuilt == f)
}
fn load(ctx: &Context, pid: &str, legacy: &State) -> Result<Plans> {
    fn account(dir: &Dir, name: &str, total: &mut u64) -> Result<()> {
        *total = total
            .checked_add(
                dir.file(name, MAX_FRAME as u64)?
                    .metadata()
                    .map_err(io_error)?
                    .len(),
            )
            .ok_or_else(|| error("storage_limit"))?;
        if *total > MAX_PROJECT {
            return Err(error("storage_limit"));
        }
        Ok(())
    }
    let mut parsed_bytes = 0;
    let mut s = Plans::default();
    let receipts = optional_area(ctx, pid, "s3-receipts")?;
    let directory = optional_area(ctx, pid, "s3")?;
    let mut intents = BTreeMap::new();
    let mut done = BTreeMap::new();
    if let Some(d) = &receipts {
        for n in d.entries()? {
            if temporary_name(&n) {
                d.file(&n, MAX_FRAME as u64)?;
                continue;
            }
            if let Some(op) = n.strip_suffix(".intent").filter(|id| state::id(id)) {
                account(d, &n, &mut parsed_bytes)?;
                let f: Frame = read(d, &n)?;
                p::require(
                    f.operation_id == op && f.project_id == pid && f.owner_id == ctx.owner.owner_id,
                )?;
                intents.insert(op.to_owned(), f);
            } else if let Some(op) = n.strip_suffix(".done").filter(|id| state::id(id)) {
                account(d, &n, &mut parsed_bytes)?;
                let receipt: Done = read(d, &n)?;
                p::require(receipt.operation_id == op)?;
                done.insert(op.to_owned(), receipt);
            } else {
                return Err(error("corrupt_store"));
            }
        }
    }
    if let Some(d) = &directory {
        p::require(d.entries()?.iter().all(|n| n == "commits"))?;
        match d.child("commits", false) {
            Ok(commits) => {
                for n in commits.entries()? {
                    if temporary_name(&n) {
                        commits.file(&n, MAX_FRAME as u64)?;
                        continue;
                    }
                    p::require(n == format!("{:020}.json", s.sequence + 1))?;
                    account(&commits, &n, &mut parsed_bytes)?;
                    let f: Frame = read(&commits, &n)?;
                    verify(ctx, pid, legacy, &s, &f)?;
                    p::require(intents.get(&f.operation_id) == Some(&f))?;
                    ingest(&mut s, f);
                }
            }
            Err(err) if err.code == "research_not_found" && done.is_empty() => {}
            Err(err) => return Err(err),
        }
    }
    for (op, receipt) in &done {
        let f = s.frames.get(op).ok_or_else(|| error("corrupt_store"))?;
        p::require(
            receipt.sequence == f.sequence
                && receipt.digest == p::hash(f, "TAKO-RESEARCH-S3-FRAME-v1")?,
        )?;
    }
    for (op, f) in intents {
        if !done.contains_key(&op) {
            if !s.frames.contains_key(&op) {
                verify(ctx, pid, legacy, &s, &f)?;
            }
            s.pending.push(f);
        }
    }
    p::require(s.pending.len() <= 1)?;
    Ok(s)
}
pub(super) fn legacy_guard(ctx: &Context, pid: &str, legacy: &State, op: &str) -> Result<()> {
    match load(ctx, pid, legacy) {
        Ok(s) if !s.pending.is_empty() => Err(error("pending_operation")),
        Ok(s) if s.frames.contains_key(op) => Err(error("idempotency_conflict")),
        // A corrupt S3 family blocks this project's writes, not other projects.
        Ok(_) => Ok(()),
        Err(err) => Err(err),
    }
}
fn publish(
    ctx: &Context,
    pid: &str,
    project: &Dir,
    d: &Dir,
    name: &str,
    value: &impl Serialize,
    role: Role,
) -> Result<()> {
    let bytes = state::canonical(value)?;
    match d.bytes(name, MAX_FRAME) {
        Ok(old) => {
            if old != bytes.as_bytes() {
                return Err(error("corrupt_store"));
            }
            d.file(name, MAX_FRAME as u64)?
                .sync_all()
                .map_err(io_error)?;
            d.0.sync_all().map_err(io_error)
        }
        Err(err) if err.code == "research_not_found" => {
            v2::reserve(ctx, pid, project, bytes.len())?;
            d.publish(name, bytes.as_bytes(), role)
        }
        Err(err) => Err(err),
    }
}
fn write(
    ctx: &Context,
    pid: &str,
    project: &Dir,
    legacy: &State,
    s: &Plans,
    cmd: &str,
    args: Value,
) -> Result<Value> {
    let op = args["operation_id"]
        .as_str()
        .ok_or_else(|| error("invalid_input"))?;
    let request = state::canonical(&json!({"command":cmd,"args":args}))?;
    if s.pending.iter().any(|f| f.operation_id != op) {
        return Err(error("pending_operation"));
    }
    let frame = if let Some(f) = s
        .frames
        .get(op)
        .or_else(|| s.pending.iter().find(|f| f.operation_id == op))
    {
        if f.request != request {
            return Err(error("idempotency_conflict"));
        }
        f.clone()
    } else {
        if legacy.operations.contains_key(op) || legacy.s2.operations.contains_key(op) {
            return Err(error("idempotency_conflict"));
        }
        let mut seeds = seeds(legacy)?;
        for _ in 0..16 {
            if [
                &seeds.entity_id,
                &seeds.revision_id,
                &seeds.iteration_id,
                &seeds.activity_id,
                &seeds.event_id,
            ]
            .iter()
            .all(|id| !s.ids.contains(*id))
            {
                break;
            }
            seeds = super::seeds(legacy)?;
        }
        let snapshot = if matches!(cmd, "create_plan_draft" | "create_plan_revision") {
            let a: p::Write = state::parse(args.clone())?;
            materials(project, legacy, &a.input)?
        } else {
            vec![]
        };
        build(
            ctx,
            pid,
            legacy,
            s,
            cmd,
            args.clone(),
            seeds,
            legacy.project.as_ref().unwrap().meta.version,
            snapshot,
        )?
    };
    let receipts = area(ctx, pid, "s3-receipts", true)?;
    publish(
        ctx,
        pid,
        project,
        &receipts,
        &format!("{op}.intent"),
        &frame,
        Role::Metadata,
    )?;
    fault(Phase::S3AfterIntent)?;
    let commits = area(ctx, pid, "s3", true)?.child("commits", true)?;
    publish(
        ctx,
        pid,
        project,
        &commits,
        &format!("{:020}.json", frame.sequence),
        &frame,
        Role::Commit,
    )?;
    fault(Phase::S3AfterFrame)?;
    let done = Done {
        schema_version: 3,
        operation_id: op.into(),
        sequence: frame.sequence,
        digest: p::hash(&frame, "TAKO-RESEARCH-S3-FRAME-v1")?,
    };
    publish(
        ctx,
        pid,
        project,
        &receipts,
        &format!("{op}.done"),
        &done,
        Role::Metadata,
    )?;
    fault(Phase::S3AfterDone)?;
    Ok(frame.result)
}
fn page(pairs: Vec<(Value, u64)>, limit: u64) -> Result<Value> {
    let count = pairs.len();
    let mut items = Vec::new();
    let mut last = None;
    for (v, cursor) in pairs {
        if items.len() == limit as usize {
            break;
        }
        let mut proposed = items.clone();
        proposed.push(v.clone());
        if state::canonical(&json!({"items":proposed,"next_cursor":cursor}))?.len()
            > MAX_FRAME - 256
        {
            break;
        }
        items.push(v);
        last = Some(cursor);
    }
    if count > 0 && items.is_empty() {
        return Err(error("storage_limit"));
    }
    Ok(
        json!({"next_cursor":if items.len()<count{last}else{None},"items":items,"native_review_authority":"unavailable"}),
    )
}
fn view(project: &Dir, legacy: &State, s: &Plans, r: &p::PlanRevision) -> Result<Value> {
    Ok(json!({"revision":r,"review_state":s.states[&r.revision_id],
        "latest_revision_id":latest(s,&r.plan_id)?.revision_id,"current_materials":materials(project,legacy,&r.payload.input)?,
        "project_version":version(legacy.project.as_ref().unwrap().meta.version,s.sequence)?,
        "legacy_project_version":legacy.project.as_ref().unwrap().meta.version,"s3_sequence":s.sequence,
        "validation":p::review(&r.payload.input),"native_review_authority":"unavailable",
        "pending_operations":s.pending.iter().map(|f|f.operation_id.clone()).collect::<Vec<_>>()}))
}
pub(super) fn dispatch(root: &Path, cmd: &str, args: Value) -> Result<Value> {
    p::require(state::canonical(&args)?.len() <= MAX_FRAME)?;
    validate_args(cmd, args.clone())?;
    let pid = args["project_id"]
        .as_str()
        .ok_or_else(|| error("invalid_input"))?;
    let ctx = Context::open(root, false)?;
    let writing = matches!(
        cmd,
        "create_plan_draft"
            | "create_plan_revision"
            | "submit_plan_for_review"
            | "approve_plan_revision"
            | "reject_plan_revision"
    );
    let (project, legacy, _) = ctx.ready(pid, writing)?;
    let s = load(&ctx, pid, &legacy).map_err(|err| {
        if matches!(
            err.code.as_str(),
            "research_unsafe_store" | "research_unsupported_schema" | "research_storage_limit"
        ) {
            err
        } else {
            error("corrupt_store")
        }
    })?;
    if writing {
        v2::ensure_no_pending(&ctx, pid, &legacy, None)?;
    }
    match cmd {
        "create_plan_draft" | "create_plan_revision" | "submit_plan_for_review" => {
            write(&ctx, pid, &project, &legacy, &s, cmd, args.clone())
        }
        "approve_plan_revision" | "reject_plan_revision" => {
            let a: p::Decision = state::parse(args)?;
            let r = target(&s, &a.plan_id, &a.revision_id)?;
            if r.payload_digest != a.expected_revision_digest
                || version(legacy.project.as_ref().unwrap().meta.version, s.sequence)?
                    != a.expected_project_version
            {
                return Err(error("conflict"));
            }
            Err(error("review_authority_unavailable"))
        }
        "read_plan" => {
            let a: p::Read = state::parse(args)?;
            let r = if let Some(id) = a.revision_id {
                target(&s, &a.plan_id, &id)?
            } else {
                latest(&s, &a.plan_id)?
            };
            view(&project, &legacy, &s, r)
        }
        "list_plan_revisions" => {
            let a: p::Page = state::parse(args)?;
            latest(&s, &a.plan_id)?;
            let mut revisions: Vec<_> = s
                .revisions
                .values()
                .filter(|r| r.plan_id == a.plan_id && r.revision_number > a.cursor)
                .collect();
            revisions.sort_by_key(|r| r.revision_number);
            page(
                revisions
                    .into_iter()
                    .map(|r| Ok((view(&project, &legacy, &s, r)?, r.revision_number)))
                    .collect::<Result<_>>()?,
                a.limit,
            )
        }
        "read_plan_review_history" => {
            let a: p::Page = state::parse(args)?;
            latest(&s, &a.plan_id)?;
            let mut result = page(
                s.events
                    .iter()
                    .filter(|e| e.plan_id == a.plan_id && e.sequence > a.cursor)
                    .map(|e| Ok((state::value(e)?, e.sequence)))
                    .collect::<Result<_>>()?,
                a.limit,
            )?;
            result["decisions"] = json!([]);
            Ok(result)
        }
        "validate_plan_revision" => {
            let a: p::Validate = state::parse(args)?;
            let input = if let Some(raw) = a.proposal_json {
                p::proposal(&raw)?
            } else {
                target(&s, &a.plan_id.unwrap(), &a.revision_id.unwrap())?
                    .payload
                    .input
                    .clone()
            };
            e::get(&legacy, &old_ref(&input.question)?)?;
            let context = materials(&project, &legacy, &input)?;
            let mut review = p::review(&input);
            for m in &context {
                if m.integrity != "matches" {
                    review.warnings.push(format!(
                        "Selected {} content integrity: {}; scientific validity not established.",
                        m.reference.kind, m.integrity
                    ));
                }
            }
            state::value(&review)
        }
        _ => Err(error("invalid_input")),
    }
}
