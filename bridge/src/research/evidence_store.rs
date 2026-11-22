//! Descriptor-relative schema-2 coordinator. No provider, tool or implicit repair.
use super::*;
use crate::research::evidence as e;
use serde::{Deserialize, Serialize};
use serde_json::json;
#[cfg(test)]
#[path = "evidence_store_tests.rs"]
mod tests;

macro_rules! dto {
    ($name:ident { $($field:ident: $ty:ty),* $(,)? }) => {
        #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
        #[serde(deny_unknown_fields)]
        struct $name { $($field: $ty),* }
    };
}
dto!(Header {
    schema_version: u64,
    owner_id: String,
    project_id: String,
    operation_id: String,
    request: String,
    seeds: Seeds,
    old_sequence: u64,
    old_head: e::Hash
});
dto!(SnapshotFile {
    source: String,
    destination: String,
    byte_length: u64,
    digest: e::Hash
});
dto!(Manifest {schema_version:u64, header:Header, files:Vec<SnapshotFile>});
dto!(Intent {
    schema_version: u64,
    frame: e::Frame
});
dto!(Get {
    schema_version: u64,
    project_id: String,
    reference: e::Ref
});
dto!(List {schema_version:u64,project_id:String,cursor:Option<String>,limit:u64});
dto!(Cursor {
    project_sequence: u64,
    event_index: u64
});
dto!(Events {schema_version:u64,project_id:String,cursor:Option<Cursor>,limit:u64});
dto!(Status {
    schema_version: u64,
    project_id: String,
    operation_id: String
});

pub(super) fn handles(cmd: &str, args: &Value) -> bool {
    matches!(
        cmd,
        "research_prepare_evidence_store"
            | "research_evidence_record"
            | "research_evidence_correct"
            | "research_evidence_list"
            | "research_activity_list"
            | "research_artifact_text_get"
    ) || (args["schema_version"] == 2
        && matches!(
            cmd,
            "research_record_get" | "research_events_page" | "research_operation_status"
        ))
}
fn typed<T: serde::de::DeserializeOwned + Serialize>(d: &Dir, name: &str) -> Result<T> {
    let v: T = d.typed(name)?;
    if state::canonical(&v)?.as_bytes() != d.bytes(name, MAX_FRAME)? {
        return Err(error("corrupt_store"));
    }
    Ok(v)
}
fn aux(ctx: &Context, pid: &str, name: &str, create: bool) -> Result<Dir> {
    ctx.research
        .child(name, create)?
        .child("v1", create)?
        .child(&ctx.owner.owner_id, create)?
        .child(pid, create)
}
fn aux_usage(ctx: &Context, pid: &str) -> Result<u64> {
    let mut total = 0u64;
    for (name, folders) in [("s2-snapshots", true), ("s2-intents", false)] {
        let d = match aux(ctx, pid, name, false) {
            Ok(d) => d,
            Err(e) if e.code == "research_not_found" => continue,
            Err(e) => return Err(e),
        };
        for entry in d.entries()? {
            if folders && state::id(&entry) {
                let sub = d.child(&entry, false)?;
                for file in sub.entries()? {
                    total = total
                        .checked_add(
                            sub.file(&file, 32 * 1024 * 1024)?
                                .metadata()
                                .map_err(io_error)?
                                .len(),
                        )
                        .ok_or_else(|| error("storage_limit"))?;
                }
            } else {
                total = total
                    .checked_add(
                        d.file(&entry, MAX_FRAME as u64)?
                            .metadata()
                            .map_err(io_error)?
                            .len(),
                    )
                    .ok_or_else(|| error("storage_limit"))?;
            }
        }
    }
    total = total
        .checked_add(super::v3::usage(ctx, pid)?)
        .ok_or_else(|| error("storage_limit"))?;
    Ok(total)
}
pub(super) fn reserve(ctx: &Context, pid: &str, project: &Dir, bytes: usize) -> Result<()> {
    if usage(project)?
        .checked_add(aux_usage(ctx, pid)?)
        .and_then(|n| n.checked_add(bytes as u64))
        .map_or(true, |n| n > MAX_PROJECT)
    {
        return Err(error("storage_limit"));
    }
    Ok(())
}
fn immutable(
    ctx: &Context,
    pid: &str,
    project: &Dir,
    d: &Dir,
    name: &str,
    bytes: &[u8],
    role: Role,
) -> Result<()> {
    match d.bytes(name, 32 * 1024 * 1024) {
        Ok(old) => {
            if old != bytes {
                return Err(error("corrupt_store"));
            }
            d.file(name, 32 * 1024 * 1024)?
                .sync_all()
                .map_err(io_error)?;
            d.0.sync_all().map_err(io_error)
        }
        Err(e) if e.code == "research_not_found" => {
            reserve(ctx, pid, project, bytes.len())?;
            d.publish(name, bytes, role)
        }
        Err(e) => Err(e),
    }
}
fn frame_write(ctx: &Context, project: &Dir, f: &e::Frame) -> Result<()> {
    let bytes = state::canonical(f)?;
    reserve(ctx, &f.project_id, project, bytes.len())?;
    project.child("commits", false)?.publish(
        &format!("{:020}.json", f.sequence),
        bytes.as_bytes(),
        Role::Commit,
    )
}
pub(super) fn verify_snapshot(ctx: &Context, pid: &str, st: &State) -> Result<()> {
    let Some(tr) = &st.s2.transition else {
        return Ok(());
    };
    let check = || -> Result<()> {
        let snap = aux(ctx, pid, "s2-snapshots", false)?.child(&tr.operation_id, false)?;
        let manifest_bytes = snap.bytes("manifest.json", MAX_FRAME)?;
        e::require(e::digest(&manifest_bytes) == tr.manifest_digest)?;
        let manifest: Manifest = typed(&snap, "manifest.json")?;
        let f = st
            .s2
            .operations
            .get(&tr.operation_id)
            .ok_or_else(|| error("corrupt_store"))?;
        e::require(
            manifest.header.owner_id == ctx.owner.owner_id
                && manifest.header.project_id == pid
                && manifest.header.operation_id == tr.operation_id
                && manifest.header.old_sequence == tr.old_sequence
                && manifest.header.old_head == tr.old_head
                && manifest.header.request == f.request
                && manifest.header.seeds == f.seeds,
        )?;
        let mut last = None;
        let mut head = false;
        for (i, file) in manifest.files.iter().enumerate() {
            e::require(
                file.destination == format!("{i:020}.bin")
                    && last.as_ref().map_or(true, |s: &String| s < &file.source),
            )?;
            last = Some(file.source.clone());
            let bytes = snap.bytes(&file.destination, 32 * 1024 * 1024)?;
            e::require(bytes.len() as u64 == file.byte_length && e::digest(&bytes) == file.digest)?;
            if file.source == format!("commits/{:020}.json", tr.old_sequence) {
                e::require(file.digest == tr.old_head)?;
                head = true;
            }
        }
        e::require(head && manifest.files.len() >= 4)?;
        let current = ctx
            .dir
            .child(pid, false)?
            .child("commits", false)?
            .bytes(&format!("{:020}.json", tr.old_sequence), MAX_FRAME)?;
        e::require(e::digest(&current) == tr.old_head)?;
        Ok(())
    };
    check().map_err(|_| error("corrupt_store"))
}
fn pending(ctx: &Context, pid: &str, st: &State) -> Result<Vec<Intent>> {
    let d = match aux(ctx, pid, "s2-intents", false) {
        Ok(d) => d,
        Err(e) if e.code == "research_not_found" => return Ok(vec![]),
        Err(e) => return Err(e),
    };
    let mut out = Vec::new();
    for name in d.entries()? {
        if temporary_name(&name) {
            d.file(&name, MAX_FRAME as u64)?;
            continue;
        }
        let op = name
            .strip_suffix(".json")
            .filter(|s| state::id(s))
            .ok_or_else(|| error("corrupt_store"))?;
        let intent: Intent = typed(&d, &name)?;
        if intent.frame.operation_id != op
            || intent.frame.project_id != pid
            || intent.frame.owner_id != ctx.owner.owner_id
        {
            return Err(error("corrupt_store"));
        }
        if let Some(done) = st.s2.operations.get(op) {
            if done != &intent.frame {
                return Err(error("corrupt_store"));
            }
            continue;
        }
        let req = json::decode(intent.frame.request.as_bytes())?;
        let rebuilt = e::build(
            &ctx.owner.owner_id,
            pid,
            st,
            req["command"]
                .as_str()
                .ok_or_else(|| error("corrupt_store"))?,
            req["args"].clone(),
            intent.frame.seeds.clone(),
            None,
        )
        .map_err(|_| error("corrupt_store"))?;
        if rebuilt != intent.frame {
            return Err(error("corrupt_store"));
        }
        out.push(intent);
    }
    if out.len() > 1 {
        return Err(error("corrupt_store"));
    }
    Ok(out)
}
pub(super) fn ensure_no_pending(
    ctx: &Context,
    pid: &str,
    st: &State,
    allowed: Option<&str>,
) -> Result<()> {
    let pending = pending(ctx, pid, st)?;
    if let Some(item) = pending
        .iter()
        .find(|i| Some(i.frame.operation_id.as_str()) != allowed)
    {
        let mut err = error("pending_operation");
        err.details.push(item.frame.operation_id.clone());
        return Err(err);
    }
    Ok(())
}
fn prepare(ctx: &Context, project: &Dir, st: &State, args: Value) -> Result<Value> {
    let a: e::WriteScope = state::parse(args.clone())?;
    e::validate_write(&a)?;
    let request =
        state::canonical(&json!({"command":"research_prepare_evidence_store","args":args}))?;
    if let Some(old) = st.s2.operations.get(&a.operation_id) {
        return if old.request == request {
            Ok(old.result.clone())
        } else {
            Err(error("idempotency_conflict"))
        };
    }
    if st.operations.contains_key(&a.operation_id) {
        return Err(error("idempotency_conflict"));
    }
    if st.s2.transition.is_some() {
        return Err(error("conflict"));
    }
    let oldseq = st
        .project
        .as_ref()
        .ok_or_else(|| error("not_found"))?
        .meta
        .version;
    if oldseq != a.expected_project_version {
        return Err(error("conflict"));
    }
    let commits = project.child("commits", false)?;
    let old_head = e::digest(&commits.bytes(&format!("{oldseq:020}.json"), MAX_FRAME)?);
    let d = aux(ctx, &a.project_id, "s2-snapshots", true)?.child(&a.operation_id, true)?;
    let header: Header = match typed(&d, "header.json") {
        Ok(h) => h,
        Err(err) if err.code == "research_not_found" => {
            let h = Header {
                schema_version: 1,
                owner_id: ctx.owner.owner_id.clone(),
                project_id: a.project_id.clone(),
                operation_id: a.operation_id.clone(),
                request: request.clone(),
                seeds: seeds(st)?,
                old_sequence: oldseq,
                old_head: old_head.clone(),
            };
            immutable(
                ctx,
                &a.project_id,
                project,
                &d,
                "header.json",
                state::canonical(&h)?.as_bytes(),
                Role::Metadata,
            )?;
            h
        }
        Err(err) => return Err(err),
    };
    if header.request != request {
        return Err(error("idempotency_conflict"));
    }
    if header.old_sequence != oldseq || header.old_head != old_head {
        return Err(error("conflict"));
    }
    if header.owner_id != ctx.owner.owner_id
        || header.project_id != a.project_id
        || header.operation_id != a.operation_id
    {
        return Err(error("corrupt_store"));
    }
    fault(Phase::S2AfterHeader)?;
    let first = st
        .operations
        .values()
        .find(|c| c.sequence == 1)
        .ok_or_else(|| error("corrupt_store"))?;
    let creates = ctx.dir.child("creates", false)?;
    let mut inputs = vec![
        "owner-v1.json".to_owned(),
        format!("creates/{}.json", first.operation_id),
        format!("creates/{}.done", first.operation_id),
    ];
    for sub in ["commits", "blobs"] {
        let dir = project.child(sub, false)?;
        for name in dir.entries()? {
            inputs.push(format!("{sub}/{name}"));
        }
    }
    inputs.sort();
    let mut files = Vec::new();
    for (i, source) in inputs.into_iter().enumerate() {
        // Copy one bounded native file at a time, never the whole project in RAM.
        let bytes = if source == "owner-v1.json" {
            ctx._v1.bytes(&source, MAX_FRAME)?
        } else {
            let (area, name) = source
                .split_once('/')
                .ok_or_else(|| error("corrupt_store"))?;
            match area {
                "creates" => creates.bytes(name, MAX_FRAME)?,
                "commits" | "blobs" => project.child(area, false)?.bytes(name, 32 * 1024 * 1024)?,
                _ => return Err(error("corrupt_store")),
            }
        };
        let destination = format!("{i:020}.bin");
        immutable(
            ctx,
            &a.project_id,
            project,
            &d,
            &destination,
            &bytes,
            Role::Blob,
        )?;
        files.push(SnapshotFile {
            source,
            destination,
            byte_length: bytes.len() as u64,
            digest: e::digest(&bytes),
        });
    }
    let manifest = Manifest {
        schema_version: 1,
        header: header.clone(),
        files,
    };
    let bytes = state::canonical(&manifest)?;
    immutable(
        ctx,
        &a.project_id,
        project,
        &d,
        "manifest.json",
        bytes.as_bytes(),
        Role::Metadata,
    )?;
    let tr = e::SnapshotRef {
        operation_id: a.operation_id.clone(),
        old_sequence: oldseq,
        old_head,
        manifest_digest: e::digest(bytes.as_bytes()),
    };
    let f = e::build(
        &ctx.owner.owner_id,
        &a.project_id,
        st,
        "research_prepare_evidence_store",
        args,
        header.seeds,
        Some(tr),
    )?;
    fault(Phase::S2BeforeTransition)?;
    frame_write(ctx, project, &f)?;
    Ok(f.result)
}
fn artifact_data<'a>(st: &'a State, r: &e::Ref) -> Result<Value> {
    e::reference(r)?;
    e::require(r.kind == "artifact")?;
    e::get(st, r)
}
fn content(project: &Dir, artifact: &Value) -> Result<String> {
    let a = &artifact["record"];
    let id = a["blob_id"]
        .as_str()
        .filter(|s| state::id(s))
        .ok_or_else(|| error("corrupt_store"))?;
    let bytes = project
        .child("blobs", false)?
        .bytes(id, 64 * 1024)
        .map_err(|err| {
            if err.code == "research_not_found" {
                error("missing_evidence")
            } else {
                err
            }
        })?;
    let expected: e::Hash =
        state::parse(a["digest"].clone()).map_err(|_| error("corrupt_store"))?;
    if a["media_type"] != "text/plain"
        || a["byte_length"].as_u64() != Some(bytes.len() as u64)
        || expected != e::digest(&bytes)
    {
        return Err(error("integrity_mismatch"));
    }
    String::from_utf8(bytes).map_err(|_| error("integrity_mismatch"))
}
fn check_frame_material(project: &Dir, st: &State, f: &e::Frame) -> Result<()> {
    for record in &f.records {
        if record["kind"] == "artifact" {
            content(project, record)?;
        }
        if record["kind"] == "evidence" {
            let r: e::Ref = state::parse(record["record"]["artifact"].clone())?;
            let artifact = f
                .records
                .iter()
                .find(|v| e::record_ref(v).ok().as_ref() == Some(&r))
                .cloned()
                .map(Ok)
                .unwrap_or_else(|| artifact_data(st, &r))?;
            content(project, &artifact)?;
        }
    }
    Ok(())
}
fn write(ctx: &Context, project: &Dir, st: &State, cmd: &str, args: Value) -> Result<Value> {
    let p: e::Packet = state::parse(args.clone())?;
    e::validate_packet(&p, cmd == "research_evidence_correct")?;
    let request = state::canonical(&json!({"command":cmd,"args":args}))?;
    ensure_no_pending(ctx, &p.project_id, st, Some(&p.operation_id))?;
    if let Some(old) = st.s2.operations.get(&p.operation_id) {
        if old.request != request {
            return Err(error("idempotency_conflict"));
        }
        check_frame_material(project, st, old)?;
        return Ok(old.result.clone());
    }
    if st.operations.contains_key(&p.operation_id) {
        return Err(error("idempotency_conflict"));
    }
    if st.s2.transition.is_none() {
        return Err(error("unsupported_schema"));
    }
    let existing = pending(ctx, &p.project_id, st)?
        .into_iter()
        .find(|i| i.frame.operation_id == p.operation_id);
    let frame = if let Some(intent) = existing {
        if intent.frame.request != request {
            return Err(error("idempotency_conflict"));
        }
        intent.frame
    } else {
        e::build(
            &ctx.owner.owner_id,
            &p.project_id,
            st,
            cmd,
            args,
            seeds(st)?,
            None,
        )?
    };
    let intents = aux(ctx, &p.project_id, "s2-intents", true)?;
    immutable(
        ctx,
        &p.project_id,
        project,
        &intents,
        &format!("{}.json", p.operation_id),
        state::canonical(&Intent {
            schema_version: 1,
            frame: frame.clone(),
        })?
        .as_bytes(),
        Role::Metadata,
    )?;
    fault(Phase::S2AfterIntent)?;
    if let Some(text) = &p.text {
        immutable(
            ctx,
            &p.project_id,
            project,
            &project.child("blobs", false)?,
            &frame.seeds.revision_id,
            text.as_bytes(),
            Role::Blob,
        )?;
    }
    fault(Phase::S2AfterBlob)?;
    check_frame_material(project, st, &frame)?;
    frame_write(ctx, project, &frame)?;
    Ok(frame.result)
}
pub(super) fn legacy_mutate(
    ctx: &Context,
    project: &Dir,
    st: &State,
    m: &Mutation,
) -> Result<MutationResult> {
    let pid = m.project_id().ok_or_else(|| error("invalid_input"))?;
    ensure_no_pending(ctx, pid, st, None)?;
    if let Some(old) = st.operations.get(m.operation_id()) {
        return if old.request == state::canonical(m)? {
            Ok(old.result.clone())
        } else {
            Err(error("idempotency_conflict"))
        };
    }
    if st.s2.operations.contains_key(m.operation_id()) {
        return Err(error("idempotency_conflict"));
    }
    let cmd = match m {
        Mutation::Update(_) => "research_project_update",
        Mutation::Question(_) => "research_question_save",
        Mutation::Plan(_) => "research_plan_revision_save",
        _ => return Err(error("invalid_input")),
    };
    let args = state::value(m)?["args"].clone();
    let f = e::build(&ctx.owner.owner_id, pid, st, cmd, args, seeds(st)?, None)?;
    frame_write(ctx, project, &f)?;
    state::parse(f.result)
}
fn needs_review(st: &State, record: &Value) -> bool {
    let mut review = false;
    if let Ok(r) = e::record_ref(record) {
        review |= st
            .s2
            .records
            .keys()
            .any(|(k, id, v)| k == &r.kind && id == &r.id && *v > r.version);
        if r.kind == "artifact" {
            review |= st.s2.records.values().any(|v| {
                v["kind"] == "artifact"
                    && v["record"]["supersedes"] == state::value(&r).unwrap_or(Value::Null)
            });
        }
    }
    if let Ok(r) = state::parse::<e::Ref>(record["record"]["source"].clone()) {
        review |= st
            .s2
            .records
            .keys()
            .any(|(k, id, v)| k == &r.kind && id == &r.id && *v > r.version);
    }
    if let Ok(r) = state::parse::<e::Ref>(record["record"]["artifact"].clone()) {
        review |= st.s2.records.values().any(|v| {
            v["kind"] == "artifact"
                && v["record"]["supersedes"] == state::value(&r).unwrap_or(Value::Null)
        });
    }
    review
}
pub(super) fn inspection(project: &Dir, st: &State, record: &Value) -> Value {
    let review = needs_review(st, record);
    let artifact = if record["kind"] == "artifact" {
        Ok(record.clone())
    } else {
        state::parse::<e::Ref>(record["record"]["artifact"].clone())
            .and_then(|r| artifact_data(st, &r))
    };
    if !matches!(record["kind"].as_str(), Some("artifact" | "evidence")) {
        return json!({"integrity":"not_checked","scientific_assessment":"not_assessed"});
    }
    match artifact.and_then(|a| content(project, &a)) {
        Ok(_) => {
            json!({"integrity":"matches","availability":"available","review_state":if review{"needs_review"}else{"recorded"},"scientific_assessment":"not_assessed","independent_verification":"not_established"})
        }
        Err(err) => {
            json!({"integrity":if err.code=="research_integrity_mismatch"{"mismatch"}else{"unavailable"},"availability":"unavailable","reason":err.code,"review_state":"needs_review","scientific_assessment":"not_assessed","independent_verification":"not_established"})
        }
    }
}
fn bounded_page(pairs: Vec<(Value, Value)>, limit: u64) -> Result<Value> {
    let count = pairs.len();
    let mut items = Vec::new();
    let mut last = None;
    for (value, cursor) in pairs {
        if items.len() == limit as usize {
            break;
        }
        let mut candidate = items.clone();
        candidate.push(value.clone());
        if state::canonical(&json!({"items":candidate,"next_cursor":cursor}))?.len() > MAX_FRAME {
            break;
        }
        items.push(value);
        last = Some(cursor);
    }
    if count > 0 && items.is_empty() {
        return Err(error("storage_limit"));
    }
    Ok(json!({"next_cursor":if items.len()<count {last}else{None},"items":items}))
}
fn all_events(st: &State) -> Vec<e::Event> {
    let mut out: Vec<_> = st
        .events
        .iter()
        .map(|event| {
            let reason = st
                .get(&event.activity)
                .ok()
                .and_then(|v| v.meta().clock_reason.clone());
            e::legacy_event(event, reason)
        })
        .chain(st.s2.events.clone())
        .collect();
    out.sort_by_key(|e| (e.project_sequence, e.event_index));
    out
}
pub(super) fn dispatch(root: &Path, cmd: &str, args: Value) -> Result<Value> {
    if state::canonical(&args)?.len() > MAX_FRAME {
        return Err(error("invalid_input"));
    }
    let raw = args.as_object().ok_or_else(|| error("invalid_input"))?;
    let version = raw
        .get("schema_version")
        .and_then(Value::as_u64)
        .ok_or_else(|| error("invalid_input"))?;
    let pid = raw
        .get("project_id")
        .and_then(Value::as_str)
        .ok_or_else(|| error("invalid_input"))?
        .to_owned();
    e::scope(version, &pid)?;
    // Parse the whole strict request before opening or creating any namespace.
    match cmd {
        "research_prepare_evidence_store" => e::validate_write(&state::parse(args.clone())?)?,
        "research_evidence_record" | "research_evidence_correct" => e::validate_packet(
            &state::parse(args.clone())?,
            cmd == "research_evidence_correct",
        )?,
        "research_record_get" | "research_artifact_text_get" => {
            let a: Get = state::parse(args.clone())?;
            e::reference(&a.reference)?;
        }
        "research_evidence_list" => {
            let a: List = state::parse(args.clone())?;
            state::page(a.limit)?;
            if let Some(c) = a.cursor {
                e::require(state::id(&c))?;
            }
        }
        "research_activity_list" | "research_events_page" => {
            let a: Events = state::parse(args.clone())?;
            state::page(a.limit)?;
            if let Some(c) = a.cursor {
                e::require(c.project_sequence <= SAFE_INTEGER && c.event_index <= SAFE_INTEGER)?;
            }
        }
        "research_operation_status" => {
            let a: Status = state::parse(args.clone())?;
            e::require(state::id(&a.operation_id))?;
        }
        _ => return Err(error("invalid_input")),
    }
    let ctx = Context::open_mode(root, false, cmd == "research_prepare_evidence_store")?;
    let writing = matches!(
        cmd,
        "research_prepare_evidence_store"
            | "research_evidence_record"
            | "research_evidence_correct"
    );
    let (project, st, _) = ctx.ready(&pid, writing)?;
    if writing {
        super::v3::legacy_guard(
            &ctx,
            &pid,
            &st,
            args["operation_id"]
                .as_str()
                .ok_or_else(|| error("invalid_input"))?,
        )?;
    }
    if cmd == "research_prepare_evidence_store" {
        return prepare(&ctx, &project, &st, args);
    }
    if cmd == "research_operation_status" {
        let a: Status = state::parse(args)?;
        let pending = pending(&ctx, &pid, &st)?;
        if let Some(frame) = st.s2.operations.get(&a.operation_id) {
            return Ok(
                json!({"status":"complete","result":frame.result,"material_status":if check_frame_material(&project,&st,frame).is_ok(){"available"}else{"unavailable"},"pending_operations":pending.iter().map(|i|i.frame.operation_id.clone()).collect::<Vec<_>>()}),
            );
        }
        if let Some(frame) = st.operations.get(&a.operation_id) {
            return Ok(
                json!({"status":"complete","result":frame.result,"pending_operations":pending.iter().map(|i|i.frame.operation_id.clone()).collect::<Vec<_>>()}),
            );
        }
        if pending
            .iter()
            .any(|i| i.frame.operation_id == a.operation_id)
        {
            return Ok(
                json!({"status":"pending","result":null,"pending_operations":[a.operation_id]}),
            );
        }
        match aux(&ctx, &pid, "s2-snapshots", false).and_then(|d| d.child(&a.operation_id, false)) {
            Ok(d) => {
                let h: Header = typed(&d, "header.json")?;
                if h.owner_id != ctx.owner.owner_id
                    || h.project_id != pid
                    || h.operation_id != a.operation_id
                {
                    return Err(error("corrupt_store"));
                }
                return Ok(
                    json!({"status":if h.old_sequence==st.project.as_ref().unwrap().meta.version {"pending_preparation"}else{"conflict"},"result":null}),
                );
            }
            Err(err) if err.code == "research_not_found" => {}
            Err(err) => return Err(err),
        }
        return Ok(
            json!({"status":"unknown","result":null,"pending_operations":pending.iter().map(|i|i.frame.operation_id.clone()).collect::<Vec<_>>()}),
        );
    }
    if st.s2.transition.is_none() {
        return Err(error("unsupported_schema"));
    }
    match cmd {
        "research_evidence_record" | "research_evidence_correct" => {
            write(&ctx, &project, &st, cmd, args)
        }
        "research_record_get" | "research_artifact_text_get" => {
            let a: Get = state::parse(args)?;
            let mut record = e::get(&st, &a.reference)?;
            if cmd == "research_artifact_text_get" {
                let a = artifact_data(&st, &a.reference)?;
                return Ok(
                    json!({"reference":e::record_ref(&a)?,"text":content(&project,&a)?,"media_type":"text/plain","integrity":"matches"}),
                );
            }
            let view = inspection(&project, &st, &record);
            record["inspection"] = view;
            Ok(record)
        }
        "research_evidence_list" => {
            let a: List = state::parse(args)?;
            let mut latest = BTreeMap::new();
            for ((kind, id, _), value) in &st.s2.records {
                if kind == "evidence" {
                    latest.insert(id.clone(), value.clone());
                }
            }
            let values = latest
                .into_iter()
                .filter(|(id, _)| a.cursor.as_ref().map_or(true, |c| id > c))
                .map(|(id, mut v)| {
                    v["inspection"] = json!({"integrity":"not_checked","scientific_assessment":"not_assessed", "review_state":if needs_review(&st,&v){"needs_review"}else{"recorded"}});
                    (v, json!(id))
                })
                .collect();
            bounded_page(values, a.limit)
        }
        "research_activity_list" | "research_events_page" => {
            let a: Events = state::parse(args)?;
            let mut values = Vec::new();
            for event in all_events(&st) {
                if a.cursor.as_ref().is_some_and(|c| {
                    (event.project_sequence, event.event_index)
                        <= (c.project_sequence, c.event_index)
                }) {
                    continue;
                }
                let cursor = json!({"project_sequence":event.project_sequence,"event_index":event.event_index});
                let value = if cmd == "research_events_page" {
                    state::value(&event)?
                } else {
                    e::get(&st, &event.activity)?
                };
                values.push((value, cursor));
            }
            bounded_page(values, a.limit)
        }
        _ => Err(error("invalid_input")),
    }
}
