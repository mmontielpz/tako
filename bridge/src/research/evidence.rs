//! Pure S2 record construction. Storage acknowledgement never verifies science.
use super::{
    error,
    model::*,
    state::{self, Result, State},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

macro_rules! dto {
    ($name:ident { $($field:ident: $ty:ty),* $(,)? }) => {
        #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
        #[serde(deny_unknown_fields)]
        pub(super) struct $name { $(pub $field: $ty),* }
    };
}
dto!(Ref {
    kind: String,
    id: String,
    version: u64
});
dto!(Hash {
    algorithm: String,
    value: String
});
pub(super) fn digest(bytes: &[u8]) -> Hash {
    Hash {
        algorithm: "sha256".into(),
        value: Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    }
}
dto!(WriteScope {
    schema_version: u64,
    project_id: String,
    operation_id: String,
    expected_project_version: u64
});
dto!(SourceInput { title: String, authors: Vec<String>, date: Option<String>, url: Option<String>, doi: Option<String>, attribution: String, previous: Option<Ref> });
dto!(Packet {
    schema_version: u64, project_id: String, operation_id: String, expected_project_version: u64,
    previous: Option<Ref>, revision_reason: String, claim: String, stance: String,
    limitations: Vec<String>, origin: String, text: Option<String>, artifact: Option<Ref>,
    source: Option<SourceInput>, source_ref: Option<Ref>, question: Option<Ref>,
    plan_revision: Option<Ref>, association_reason: Option<String>
});
dto!(SnapshotRef {
    operation_id: String,
    old_sequence: u64,
    old_head: Hash,
    manifest_digest: Hash
});
dto!(Event { id: String, version: u64, project_sequence: u64, event_index: u64,
    activity: Ref, operation_id: String, actor_id: String, at_ms: Option<u64>,
    clock_reason: Option<String>, kind: String, targets: Vec<Ref>, outcome: String });
dto!(Frame { schema_version: u64, owner_id: String, project_id: String, sequence: u64,
    operation_id: String, kind: String, request: String, seeds: Seeds,
    records: Vec<Value>, events: Vec<Event>, result: Value, transition: Option<SnapshotRef> });
#[derive(Default)]
pub(super) struct EvidenceState {
    pub transition: Option<SnapshotRef>,
    pub records: BTreeMap<(String, String, u64), Value>,
    pub operations: BTreeMap<String, Frame>,
    pub events: Vec<Event>,
}
pub(super) fn require(ok: bool) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(error("invalid_input"))
    }
}
pub(super) fn text(s: &str, max: usize, nonempty: bool) -> Result<()> {
    require(s.len() <= max && !s.contains('\0') && (!nonempty || !s.trim().is_empty()))
}
pub(super) fn reference(r: &Ref) -> Result<()> {
    require(
        state::id(&r.id)
            && r.version > 0
            && r.version <= SAFE_INTEGER
            && matches!(
                r.kind.as_str(),
                "project"
                    | "question"
                    | "plan"
                    | "plan_revision"
                    | "activity"
                    | "source"
                    | "artifact"
                    | "evidence"
            ),
    )
}
pub(super) fn scope(v: u64, pid: &str) -> Result<()> {
    if v != 2 {
        return Err(error("unsupported_schema"));
    }
    require(state::id(pid))
}
pub(super) fn validate_write(a: &WriteScope) -> Result<()> {
    scope(a.schema_version, &a.project_id)?;
    require(
        state::id(&a.operation_id)
            && a.expected_project_version > 0
            && a.expected_project_version < SAFE_INTEGER,
    )
}
pub(super) fn validate_packet(p: &Packet, correcting: bool) -> Result<()> {
    validate_write(&WriteScope {
        schema_version: p.schema_version,
        project_id: p.project_id.clone(),
        operation_id: p.operation_id.clone(),
        expected_project_version: p.expected_project_version,
    })?;
    require(p.previous.is_some() == correcting)?;
    if let Some(r) = &p.previous {
        reference(r)?;
        require(r.kind == "evidence")?;
    }
    text(&p.claim, 4096, true)?;
    text(&p.revision_reason, 512, true)?;
    require(matches!(
        p.stance.as_str(),
        "supporting" | "challenging" | "contextual" | "insufficient" | "unknown"
    ))?;
    require(matches!(
        p.origin.as_str(),
        "local_input" | "imported_reported" | "ai_generated_reported"
    ))?;
    require(p.limitations.len() <= 16)?;
    for s in &p.limitations {
        text(s, 512, true)?;
    }
    require(p.text.is_some() != p.artifact.is_some())?;
    if !correcting {
        require(p.text.is_some())?;
    }
    if let Some(t) = &p.text {
        text(t, 64 * 1024, false)?;
    }
    if let Some(r) = &p.artifact {
        reference(r)?;
        require(r.kind == "artifact")?;
    }
    require(!(p.source.is_some() && p.source_ref.is_some()))?;
    if let Some(s) = &p.source {
        text(&s.title, 1024, true)?;
        text(&s.attribution, 1024, true)?;
        require(s.authors.len() <= 16)?;
        for v in &s.authors {
            text(v, 512, true)?;
        }
        for v in [&s.date, &s.url, &s.doi].into_iter().flatten() {
            text(v, 2048, true)?;
        }
        if let Some(r) = &s.previous {
            reference(r)?;
            require(r.kind == "source")?;
        }
    }
    for (r, kind) in [
        (&p.source_ref, "source"),
        (&p.question, "question"),
        (&p.plan_revision, "plan_revision"),
    ] {
        if let Some(r) = r {
            reference(r)?;
            require(r.kind == kind)?;
        }
    }
    if p.question.is_none() || p.plan_revision.is_none() {
        text(p.association_reason.as_deref().unwrap_or(""), 512, true)?;
    } else {
        require(p.association_reason.is_none())?;
    }
    require(state::canonical(p)?.len() <= MAX_FRAME)
}
pub(super) fn get(st: &State, r: &Ref) -> Result<Value> {
    reference(r)?;
    if let Some(record) = st
        .s2
        .records
        .get(&(r.kind.clone(), r.id.clone(), r.version))
    {
        return Ok(record.clone());
    }
    if matches!(r.kind.as_str(), "source" | "artifact" | "evidence") {
        return Err(error("not_found"));
    }
    let legacy: RecordRef = state::parse(state::value(r)?)?;
    state::value(st.get(&legacy)?)
}
fn latest(st: &State, r: &Ref) -> Result<Value> {
    let record = get(st, r)?;
    if st
        .s2
        .records
        .keys()
        .any(|(kind, id, version)| kind == &r.kind && id == &r.id && *version > r.version)
    {
        return Err(error("conflict"));
    }
    Ok(record)
}
fn artifact_depth(st: &State, r: &Ref) -> Result<()> {
    let mut next = Some(r.clone());
    let mut visited = BTreeSet::new();
    while let Some(r) = next {
        require(visited.len() < 31 && visited.insert((r.id.clone(), r.version)))?;
        let record = get(st, &r)?;
        next = if record["record"]["supersedes"].is_null() {
            None
        } else {
            Some(state::parse(record["record"]["supersedes"].clone())?)
        };
    }
    Ok(())
}
pub(super) fn record_ref(v: &Value) -> Result<Ref> {
    let meta = &v["record"]["meta"];
    let r = Ref {
        kind: v["kind"]
            .as_str()
            .ok_or_else(|| error("corrupt_store"))?
            .into(),
        id: meta["id"]
            .as_str()
            .ok_or_else(|| error("corrupt_store"))?
            .into(),
        version: meta["version"]
            .as_u64()
            .ok_or_else(|| error("corrupt_store"))?,
    };
    reference(&r)?;
    Ok(r)
}
fn meta(
    owner: &str,
    pid: &str,
    id: &str,
    version: u64,
    old: Option<&Value>,
    clock: &Clock,
) -> Value {
    json!({"schema_version":2,"owner_id":owner,"project_id":pid,"id":id,"version":version,
        "created_at_ms":old.map(|v|v["record"]["meta"]["created_at_ms"].clone()).unwrap_or(json!(clock.at_ms)),
        "updated_at_ms":clock.at_ms,"clock_status":if clock.at_ms.is_some(){"known"}else{"uncertain"},"clock_reason":clock.reason})
}
fn tagged(kind: &str, record: Value) -> Value {
    json!({"kind":kind,"record":record})
}
fn legacy_mutation(cmd: &str, args: Value) -> Result<Mutation> {
    let tag = match cmd {
        "research_project_update" => "update",
        "research_question_save" => "question",
        "research_plan_revision_save" => "plan",
        _ => return Err(error("invalid_input")),
    };
    state::parse(json!({"command":tag,"args":args}))
}
pub(super) fn legacy_command(cmd: &str) -> bool {
    matches!(
        cmd,
        "research_project_update" | "research_question_save" | "research_plan_revision_save"
    )
}
pub(super) fn legacy_event(e: &ActivityEvent, reason: Option<String>) -> Event {
    Event {
        id: e.id.clone(),
        version: 1,
        project_sequence: e.project_sequence,
        event_index: 0,
        activity: Ref {
            kind: "activity".into(),
            id: e.activity.id.clone(),
            version: e.activity.version,
        },
        operation_id: e.operation_id.clone(),
        actor_id: e.actor_id.clone(),
        at_ms: e.at_ms,
        clock_reason: reason,
        kind: e.kind.clone(),
        targets: e
            .targets
            .iter()
            .map(|r| Ref {
                kind: state::value(&r.kind).unwrap().as_str().unwrap().into(),
                id: r.id.clone(),
                version: r.version,
            })
            .collect(),
        outcome: "Local metadata stored; no scientific verification.".into(),
    }
}
pub(super) fn build(
    owner: &str,
    pid: &str,
    st: &State,
    cmd: &str,
    args: Value,
    seeds: Seeds,
    transition: Option<SnapshotRef>,
) -> Result<Frame> {
    let old_project = st.project.as_ref().ok_or_else(|| error("not_found"))?;
    let seq = old_project
        .meta
        .version
        .checked_add(1)
        .filter(|n| *n <= SAFE_INTEGER)
        .ok_or_else(|| error("storage_limit"))?;
    let request = state::canonical(&json!({"command":cmd,"args":args}))?;
    let ids = [
        &seeds.entity_id,
        &seeds.revision_id,
        &seeds.iteration_id,
        &seeds.activity_id,
        &seeds.event_id,
    ];
    require(ids.iter().all(|id| {
        state::id(id) && id.as_str() != owner && id.as_str() != pid && !st.used_ids.contains(*id)
    }))?;
    require(ids.into_iter().collect::<BTreeSet<_>>().len() == 5)?;
    match (&seeds.clock.at_ms, &seeds.clock.reason) {
        (Some(n), None) => require(*n <= SAFE_INTEGER)?,
        (None, Some(s)) => text(s, 512, true)?,
        _ => return Err(error("invalid_input")),
    }
    if legacy_command(cmd) {
        require(st.s2.transition.is_some() && transition.is_none())?;
        let c = state::build(owner, pid, st, &legacy_mutation(cmd, args)?, seeds.clone())?;
        let f = Frame {
            schema_version: 2,
            owner_id: owner.into(),
            project_id: pid.into(),
            sequence: seq,
            operation_id: c.operation_id.clone(),
            kind: "record_commit".into(),
            request,
            seeds,
            records: c.records.iter().map(state::value).collect::<Result<_>>()?,
            events: vec![legacy_event(&c.event, c.seeds.clock.reason.clone())],
            result: state::value(&c.result)?,
            transition: None,
        };
        require(state::canonical(&f)?.len() <= MAX_FRAME)?;
        return Ok(f);
    }
    let op;
    let mut records = Vec::new();
    let activity_ref = Ref {
        kind: "activity".into(),
        id: seeds.activity_id.clone(),
        version: 1,
    };
    let kind;
    if cmd == "research_prepare_evidence_store" {
        let a: WriteScope = state::parse(args)?;
        validate_write(&a)?;
        require(st.s2.transition.is_none() && a.project_id == pid)?;
        if a.expected_project_version != old_project.meta.version {
            return Err(error("conflict"));
        }
        let tr = transition.as_ref().ok_or_else(|| error("invalid_input"))?;
        require(tr.operation_id == a.operation_id && tr.old_sequence == old_project.meta.version)?;
        op = a.operation_id;
        kind = "format_prepared";
    } else {
        require(st.s2.transition.is_some() && transition.is_none())?;
        let correcting = cmd == "research_evidence_correct";
        require(correcting || cmd == "research_evidence_record")?;
        let p: Packet = state::parse(args)?;
        validate_packet(&p, correcting)?;
        require(p.project_id == pid)?;
        if p.expected_project_version != old_project.meta.version {
            return Err(error("conflict"));
        }
        for r in [&p.question, &p.plan_revision].into_iter().flatten() {
            get(st, r)?;
        }
        if let Some(r) = &p.plan_revision {
            let plan = get(st, r)?;
            require(
                p.question.as_ref().map(state::value).transpose()?.as_ref()
                    == Some(&plan["record"]["question"]),
            )?;
        }
        let previous = p.previous.as_ref().map(|r| latest(st, r)).transpose()?;
        let source_ref = if let Some(s) = &p.source {
            let old = s.previous.as_ref().map(|r| latest(st, r)).transpose()?;
            let (id, version) = s
                .previous
                .as_ref()
                .map(|r| (r.id.clone(), r.version + 1))
                .unwrap_or((seeds.entity_id.clone(), 1));
            require(version <= SAFE_INTEGER)?;
            let v = tagged(
                "source",
                json!({"meta":meta(owner,pid,&id,version,old.as_ref(),&seeds.clock),"body":s,
                "origin":"supplied_metadata","activity":activity_ref,"content_state":"metadata_only"}),
            );
            let r = record_ref(&v)?;
            records.push(v);
            Some(r)
        } else {
            if let Some(r) = &p.source_ref {
                get(st, r)?;
            }
            p.source_ref.clone()
        };
        if p.text.is_some() {
            if let Some(old) = &previous {
                artifact_depth(st, &state::parse(old["record"]["artifact"].clone())?)?;
            }
        }
        let artifact_ref = if let Some(t) = &p.text {
            let v = tagged(
                "artifact",
                json!({"meta":meta(owner,pid,&seeds.revision_id,1,None,&seeds.clock),
                "kind":"note","activity":activity_ref,"origin":p.origin,"source":source_ref,
                "supersedes":previous.as_ref().map(|v|v["record"]["artifact"].clone()),
                "blob_id":seeds.revision_id,"media_type":"text/plain","byte_length":t.len(),"digest":digest(t.as_bytes()),"recorded_availability":"available"}),
            );
            let r = record_ref(&v)?;
            records.push(v);
            r
        } else {
            let r = p.artifact.clone().ok_or_else(|| error("invalid_input"))?;
            get(st, &r)?;
            r
        };
        let (id, version) = p
            .previous
            .as_ref()
            .map(|r| (r.id.clone(), r.version + 1))
            .unwrap_or((seeds.iteration_id.clone(), 1));
        require(version <= SAFE_INTEGER)?;
        records.push(tagged("evidence",json!({"meta":meta(owner,pid,&id,version,previous.as_ref(),&seeds.clock),
            "activity":activity_ref,"previous":p.previous,"revision_reason":p.revision_reason,"claim":p.claim,"stance":p.stance,
            "limitations":p.limitations,"origin":p.origin,"source":source_ref,"artifact":artifact_ref,
            "question":p.question,"plan_revision":p.plan_revision,"association_reason":p.association_reason,
            "scientific_assessment":"not_assessed","independent_verification":"not_established"})));
        op = p.operation_id;
        kind = "local_evidence_stored";
    }
    let targets: Vec<Ref> = records.iter().map(record_ref).collect::<Result<_>>()?;
    records.push(tagged("activity",json!({"meta":meta(owner,pid,&seeds.activity_id,1,None,&seeds.clock),
        "activity_kind":"record_only","state":"completed","operation_id":op,"iteration_id":old_project.iteration_id,
        "targets":targets,"outcome":kind})));
    let event = Event {
        id: seeds.event_id.clone(),
        version: 1,
        project_sequence: seq,
        event_index: 0,
        activity: activity_ref,
        operation_id: op.clone(),
        actor_id: owner.into(),
        at_ms: seeds.clock.at_ms,
        clock_reason: seeds.clock.reason.clone(),
        kind: kind.into(),
        targets,
        outcome: "Local storage acknowledged; no scientific verification.".into(),
    };
    let mut project = old_project.clone();
    project.meta.version = seq;
    project.meta.updated_at_ms = seeds.clock.at_ms;
    project.meta.clock_status = if seeds.clock.at_ms.is_some() {
        "known"
    } else {
        "uncertain"
    }
    .into();
    project.meta.clock_reason = seeds.clock.reason.clone();
    records.push(state::value(&Record::Project(project))?);
    for r in &records {
        require(state::canonical(r)?.len() <= 256 * 1024)?;
    }
    let refs = records.iter().map(record_ref).collect::<Result<Vec<_>>>()?;
    let result = json!({"status":"complete","operation_id":op,"project_id":pid,"project_version":seq,"commit_sequence":seq,"records":refs});
    let frame = Frame {
        schema_version: 2,
        owner_id: owner.into(),
        project_id: pid.into(),
        sequence: seq,
        operation_id: op,
        kind: if transition.is_some() {
            "transition"
        } else {
            "record_commit"
        }
        .into(),
        request,
        seeds,
        records,
        events: vec![event],
        result,
        transition,
    };
    require(state::canonical(&frame)?.len() <= MAX_FRAME)?;
    Ok(frame)
}
pub(super) fn ingest(st: &mut State, f: Frame) -> Result<()> {
    let req: Value = serde_json::from_str(&f.request).map_err(|_| error("corrupt_store"))?;
    let cmd = req["command"]
        .as_str()
        .ok_or_else(|| error("corrupt_store"))?;
    if legacy_command(cmd) {
        let c = state::build(
            &f.owner_id,
            &f.project_id,
            st,
            &legacy_mutation(cmd, req["args"].clone())?,
            f.seeds.clone(),
        )?;
        st.ingest(c);
    } else {
        for r in &f.records {
            if r["record"]["meta"]["schema_version"] == 1 {
                let legacy: Record = state::parse(r.clone())?;
                let rf = legacy.reference();
                if let Record::Project(p) = &legacy {
                    st.project = Some(p.clone());
                }
                st.records.insert((rf.kind, rf.id, rf.version), legacy);
            } else {
                let rref = record_ref(r)?;
                st.s2
                    .records
                    .insert((rref.kind, rref.id, rref.version), r.clone());
            }
        }
        st.s2.events.extend(f.events.clone());
    }
    for id in [
        &f.seeds.entity_id,
        &f.seeds.revision_id,
        &f.seeds.iteration_id,
        &f.seeds.activity_id,
        &f.seeds.event_id,
    ] {
        st.used_ids.insert(id.clone());
    }
    if let Some(tr) = &f.transition {
        st.s2.transition = Some(tr.clone());
    }
    st.s2.operations.insert(f.operation_id.clone(), f);
    Ok(())
}
pub(super) fn replay(owner: &str, pid: &str, bytes: &[u8], st: &mut State) -> Result<()> {
    let f: Frame = state::parse(super::json::decode(bytes)?).map_err(|_| error("corrupt_store"))?;
    if f.schema_version != 2 {
        return Err(error("unsupported_schema"));
    }
    require(
        f.owner_id == owner
            && f.project_id == pid
            && !st.s2.operations.contains_key(&f.operation_id)
            && !st.operations.contains_key(&f.operation_id),
    )?;
    require(state::canonical(&f)?.as_bytes() == bytes)?;
    let req: Value = super::json::decode(f.request.as_bytes())?;
    require(
        req.as_object()
            .is_some_and(|v| v.len() == 2 && v.contains_key("command") && v.contains_key("args")),
    )?;
    let expected = build(
        owner,
        pid,
        st,
        req["command"]
            .as_str()
            .ok_or_else(|| error("corrupt_store"))?,
        req["args"].clone(),
        f.seeds.clone(),
        f.transition.clone(),
    )?;
    require(f == expected)?;
    ingest(st, f)
}
