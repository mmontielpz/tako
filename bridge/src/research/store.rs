//! Linux descriptor-relative immutable store. Publication never replaces old data.
use super::{
    error, json,
    model::*,
    state::{self, Result, State},
};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::BTreeMap;
use std::ffi::{CStr, CString};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

fn io_error(e: io::Error) -> crate::BridgeError {
    match e.raw_os_error() {
        Some(libc::ENOENT) => error("not_found"),
        Some(libc::ELOOP | libc::ENOTDIR | libc::ENXIO) => error("unsafe_store"),
        Some(libc::ENOSPC | libc::EDQUOT | libc::EFBIG) => error("storage_limit"),
        _ => error("write_failed"),
    }
}
fn native_id() -> Result<String> {
    let mut bytes = [0u8; 16];
    let mut offset = 0;
    while offset < bytes.len() {
        let n = unsafe {
            libc::getrandom(bytes[offset..].as_mut_ptr().cast(), bytes.len() - offset, 0)
        };
        if n < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error("unavailable"));
        }
        if n == 0 {
            return Err(error("unavailable"));
        }
        offset += n as usize;
    }
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
fn c(s: &str) -> Result<CString> {
    CString::new(s).map_err(|_| error("invalid_input"))
}
fn temporary_name(name: &str) -> bool {
    name.strip_prefix(".tmp-").is_some_and(state::id)
}

struct Dir(File, std::cell::Cell<Option<libc::pid_t>>);
impl Drop for Dir {
    fn drop(&mut self) {
        // flock belongs to the open description, so closing our descriptor alone
        // can leave the request locked in an unrelated pre-exec child. Release it
        // at the end of this request, but never from a forked copy of its guard.
        if self.1.get() == Some(unsafe { libc::getpid() }) {
            while unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) } != 0 {
                if io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
                    break; // File's destructor still closes the descriptor.
                }
            }
        }
    }
}
impl Dir {
    fn root(path: &Path) -> Result<Self> {
        let f = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)
            .map_err(io_error)?;
        let m = f.metadata().map_err(io_error)?;
        if m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o022 != 0 {
            return Err(error("unsafe_store"));
        }
        Ok(Self(f, std::cell::Cell::new(None)))
    }
    fn child(&self, name: &str, create: bool) -> Result<Self> {
        let name = c(name)?;
        if create {
            let made = unsafe { libc::mkdirat(self.0.as_raw_fd(), name.as_ptr(), 0o700) };
            if made != 0 && io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST) {
                return Err(io_error(io::Error::last_os_error()));
            }
            self.0.sync_all().map_err(io_error)?;
        }
        let fd = unsafe {
            libc::openat(
                self.0.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY
                    | libc::O_DIRECTORY
                    | libc::O_NOFOLLOW
                    | libc::O_NONBLOCK
                    | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(io_error(io::Error::last_os_error()));
        }
        let f = unsafe { File::from_raw_fd(fd) };
        let m = f.metadata().map_err(io_error)?;
        if m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o077 != 0 {
            return Err(error("unsafe_store"));
        }
        if create {
            f.sync_all().map_err(io_error)?;
        }
        Ok(Self(f, std::cell::Cell::new(None)))
    }
    fn lock(&self, exclusive: bool) -> Result<()> {
        if unsafe {
            libc::flock(
                self.0.as_raw_fd(),
                (if exclusive {
                    libc::LOCK_EX
                } else {
                    libc::LOCK_SH
                }) | libc::LOCK_NB,
            )
        } != 0
        {
            let e = io::Error::last_os_error();
            if matches!(e.raw_os_error(), Some(libc::EWOULDBLOCK)) {
                return Err(error("busy"));
            }
            return Err(io_error(e));
        }
        self.1.set(Some(unsafe { libc::getpid() }));
        Ok(())
    }
    fn entries(&self) -> Result<Vec<String>> {
        // A fresh open description avoids shared directory offsets across repeated scans.
        let dot = c(".")?;
        let fd = unsafe {
            libc::openat(
                self.0.as_raw_fd(),
                dot.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(io_error(io::Error::last_os_error()));
        }
        let stream = unsafe { libc::fdopendir(fd) };
        if stream.is_null() {
            unsafe { libc::close(fd) };
            return Err(error("unavailable"));
        }
        struct Close(*mut libc::DIR);
        impl Drop for Close {
            fn drop(&mut self) {
                unsafe { libc::closedir(self.0) };
            }
        }
        let stream = Close(stream);
        let mut names = Vec::new();
        loop {
            unsafe { *libc::__errno_location() = 0 };
            let entry = unsafe { libc::readdir(stream.0) };
            if entry.is_null() {
                if io::Error::last_os_error().raw_os_error() != Some(0) {
                    return Err(error("unavailable"));
                }
                break;
            }
            let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }
                .to_str()
                .map_err(|_| error("unsafe_store"))?;
            if name != "." && name != ".." {
                names.push(name.to_owned())
            }
        }
        names.sort();
        Ok(names)
    }
    fn file(&self, name: &str, max: u64) -> Result<File> {
        let name = c(name)?;
        let fd = unsafe {
            libc::openat(
                self.0.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(io_error(io::Error::last_os_error()));
        }
        let f = unsafe { File::from_raw_fd(fd) };
        let m = f.metadata().map_err(io_error)?;
        if !m.is_file()
            || m.uid() != unsafe { libc::geteuid() }
            || m.mode() & 0o077 != 0
            || m.nlink() != 1
        {
            return Err(error("unsafe_store"));
        }
        if m.len() > max {
            return Err(error("storage_limit"));
        }
        Ok(f)
    }
    fn bytes(&self, name: &str, max: usize) -> Result<Vec<u8>> {
        let mut f = self.file(name, max as u64)?;
        let mut bytes = Vec::new();
        (&mut f)
            .take((max + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(io_error)?;
        if bytes.len() > max {
            return Err(error("corrupt_store"));
        }
        Ok(bytes)
    }
    fn typed<T: DeserializeOwned>(&self, name: &str) -> Result<T> {
        let bytes = self.bytes(name, MAX_FRAME)?;
        let v = json::decode(&bytes).map_err(|_| error("corrupt_store"))?;
        if v.get("schema_version").and_then(Value::as_u64) != Some(1) {
            return Err(error("unsupported_schema"));
        }
        let parsed = state::parse(v).map_err(|_| error("corrupt_store"))?;
        // Reconcile a previously published write whose acknowledgement was lost.
        self.file(name, MAX_FRAME as u64)?
            .sync_all()
            .map_err(io_error)?;
        self.0.sync_all().map_err(io_error)?;
        Ok(parsed)
    }
    fn publish(&self, name: &str, bytes: &[u8], role: Role) -> Result<()> {
        if bytes.len()
            > if role == Role::Blob {
                32 * 1024 * 1024
            } else {
                MAX_FRAME
            }
        {
            return Err(error("storage_limit"));
        }
        let temp = c(&format!(".tmp-{}", native_id()?))?;
        let fd = unsafe {
            libc::openat(
                self.0.as_raw_fd(),
                temp.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        if fd < 0 {
            return Err(io_error(io::Error::last_os_error()));
        }
        let mut f = unsafe { File::from_raw_fd(fd) };
        if role == Role::Commit || role == Role::Blob {
            fault(Phase::BeforeWrite)?
        }
        f.write_all(bytes).map_err(io_error)?;
        f.sync_all().map_err(io_error)?;
        if role == Role::Commit || role == Role::Blob {
            fault(Phase::BeforePublish)?
        }
        let name = c(name)?;
        // Hardlink publication has a two-name crash window incompatible with S1's
        // hardlink exclusion. Linux no-replacement rename is a single publication.
        let renamed = unsafe {
            libc::syscall(
                libc::SYS_renameat2,
                self.0.as_raw_fd(),
                temp.as_ptr(),
                self.0.as_raw_fd(),
                name.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if renamed != 0 {
            return Err(io_error(io::Error::last_os_error()));
        }
        if role == Role::Commit || role == Role::Blob {
            fault(Phase::AfterPublish)?
        }
        self.0.sync_all().map_err(io_error)?;
        if role == Role::Commit || role == Role::Blob {
            fault(Phase::BeforeAck)?
        }
        Ok(())
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Metadata,
    Commit,
    Blob,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    S2AfterHeader,
    S2BeforeTransition,
    S2AfterIntent,
    S2AfterBlob,
    S3AfterIntent,
    S3AfterFrame,
    S3AfterDone,
    AfterReservation,
    BeforeWrite,
    BeforePublish,
    AfterPublish,
    BeforeAck,
}
#[cfg(not(test))]
fn fault(_: Phase) -> Result<()> {
    Ok(())
}
#[cfg(test)]
thread_local! {static FAULT:std::cell::Cell<Option<(Phase,bool)>>=const{std::cell::Cell::new(None)};}
#[cfg(test)]
fn fault(phase: Phase) -> Result<()> {
    FAULT.with(|f| {
        if let Some((at, crash)) = f.get() {
            if at == phase {
                f.set(None);
                if crash {
                    std::process::exit(93)
                }
                return Err(error("storage_limit"));
            }
        }
        Ok(())
    })
}

struct Context {
    research: Dir,
    _v1: Dir,
    owner: Owner,
    dir: Dir,
}
fn retained_s2(research: &Dir) -> Result<bool> {
    for name in ["s2-snapshots", "s2-intents", "s3", "s3-receipts"] {
        match research.child(name, false) {
            Ok(dir) if !dir.entries()?.is_empty() => return Ok(true),
            Ok(_) => {}
            Err(err) if err.code == "research_not_found" => {}
            Err(err) => return Err(err),
        }
    }
    Ok(false)
}
impl Context {
    fn open(root: &Path, create: bool) -> Result<Self> {
        Self::open_mode(root, create, create)
    }
    fn open_mode(root: &Path, create: bool, exclusive: bool) -> Result<Self> {
        let research = Dir::root(root)?.child("research", create)?;
        let v1 = match research.child("v1", false) {
            Ok(dir) => dir,
            Err(err) if err.code == "research_not_found" => {
                if retained_s2(&research)? {
                    return Err(error("corrupt_store"));
                }
                research.child("v1", create)?
            }
            Err(err) => return Err(err),
        };
        v1.lock(exclusive)?;
        let owner = match v1.typed::<Owner>("owner-v1.json") {
            Ok(o) => o,
            Err(e) if e.code == "research_not_found" => {
                if retained_s2(&research)? {
                    return Err(error("corrupt_store"));
                }
                let entries = v1.entries()?;
                for name in &entries {
                    if temporary_name(name) {
                        v1.file(name, MAX_FRAME as u64)?;
                    }
                }
                if !create {
                    if entries.iter().all(|s| temporary_name(s)) {
                        return Err(error("not_found"));
                    }
                    return Err(error("corrupt_store"));
                }
                if entries.iter().any(|s| !temporary_name(s)) {
                    return Err(error("corrupt_store"));
                }
                let o = Owner {
                    schema_version: 1,
                    owner_id: native_id()?,
                };
                v1.child(&o.owner_id, true)?.child("creates", true)?;
                v1.publish(
                    "owner-v1.json",
                    state::canonical(&o)?.as_bytes(),
                    Role::Metadata,
                )?;
                o
            }
            Err(e) => return Err(e),
        };
        if owner.schema_version != 1 || !state::id(&owner.owner_id) {
            return Err(error("corrupt_store"));
        }
        for name in v1.entries()? {
            if temporary_name(&name) {
                v1.file(&name, MAX_FRAME as u64)?;
            } else if name != "owner-v1.json" && name != owner.owner_id {
                return Err(error("corrupt_store"));
            }
        }
        let dir = match v1.child(&owner.owner_id, false) {
            Ok(d) => d,
            Err(e) if e.code == "research_not_found" => return Err(error("corrupt_store")),
            Err(e) => return Err(e),
        };
        dir.child("creates", false).map_err(|e| {
            if e.code == "research_not_found" {
                error("corrupt_store")
            } else {
                e
            }
        })?;
        // Owner namespace is pinned; no fallback to another identity on corruption.
        Ok(Self {
            research,
            _v1: v1,
            owner,
            dir,
        })
    }
    fn reservation(&self, op: &str) -> Result<Reservation> {
        let creates = self.dir.child("creates", false)?;
        let r: Reservation = creates.typed(&format!("{op}.json"))?;
        if r.schema_version != 1 || r.initial.operation_id != op {
            return Err(error("corrupt_store"));
        }
        let mut st = State::default();
        state::replay(
            &self.owner.owner_id,
            &r.initial.project_id,
            state::canonical(&r.initial)?.as_bytes(),
            &mut st,
        )?;
        Ok(r)
    }
    fn done(&self, op: &str, pid: &str) -> Result<bool> {
        let creates = self.dir.child("creates", false)?;
        match creates.typed::<CreationDone>(&format!("{op}.done")) {
            Ok(d) => {
                if d.schema_version != 1 || d.project_id != pid || d.operation_id != op {
                    return Err(error("corrupt_store"));
                }
                Ok(true)
            }
            Err(e) if e.code == "research_not_found" => Ok(false),
            Err(e) => Err(e),
        }
    }
    fn ready(&self, pid: &str, exclusive: bool) -> Result<(Dir, State, Recovery)> {
        let project = match self.dir.child(pid, false) {
            Ok(project) => project,
            Err(e) if e.code == "research_not_found" => {
                // The durable receipt survives loss of a project's directory.
                return Err(match project_identities(self)?.get(pid) {
                    Some(true) => error("missing_project_data"),
                    Some(false) => error("incomplete_creation"),
                    None => e,
                });
            }
            Err(e) => return Err(e),
        };
        project.lock(exclusive)?;
        let (st, recovery) = load(&project, &self.owner.owner_id, pid).map_err(|e| {
            if e.code == "research_incomplete_creation" {
                match project_identities(self) {
                    Ok(identities) if identities.get(pid) == Some(&false) => e,
                    _ => error("corrupt_store"),
                }
            } else {
                e
            }
        })?;
        let initial = st
            .operations
            .values()
            .find(|c| c.sequence == 1)
            .ok_or_else(|| error("incomplete_creation"))?;
        let reserved = self.reservation(&initial.operation_id).map_err(|e| {
            if e.code == "research_not_found" {
                error("corrupt_store")
            } else {
                e
            }
        })?;
        if reserved.initial != *initial {
            return Err(error("corrupt_store"));
        }
        if !self.done(&initial.operation_id, pid)? {
            return Err(error("incomplete_creation"));
        }
        v2::verify_snapshot(self, pid, &st)?;
        Ok((project, st, recovery))
    }
}
fn clock(state: &State) -> Clock {
    let at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| u64::try_from(d.as_millis()).ok())
        .filter(|n| *n <= SAFE_INTEGER);
    let last = state
        .events
        .iter()
        .filter_map(|e| e.at_ms)
        .chain(state.s2.events.iter().filter_map(|e| e.at_ms))
        .max();
    match at {
        Some(n) if last.map_or(true, |old| n >= old) => Clock {
            at_ms: Some(n),
            reason: None,
        },
        _ => Clock {
            at_ms: None,
            reason: Some("clock unavailable or moved backwards".into()),
        },
    }
}
fn seeds(state: &State) -> Result<Seeds> {
    // Collision checks cover every native ID reserved by all previous commits.
    let mut ids = state.used_ids.clone();
    let mut next = || -> Result<String> {
        for _ in 0..16 {
            let n = native_id()?;
            if ids.insert(n.clone()) {
                return Ok(n);
            }
        }
        Err(error("unavailable"))
    };
    Ok(Seeds {
        entity_id: next()?,
        revision_id: next()?,
        activity_id: next()?,
        iteration_id: next()?,
        event_id: next()?,
        clock: clock(state),
    })
}
fn load(dir: &Dir, owner: &str, pid: &str) -> Result<(State, Recovery)> {
    usage(dir).map_err(|e| {
        if e.code == "research_not_found" {
            error("incomplete_creation")
        } else {
            e
        }
    })?;
    let mut recovery = Recovery { orphan_files: 0 };
    for name in dir.entries()? {
        if name != "commits" && name != "blobs" {
            return Err(error("corrupt_store"));
        }
    }
    let commits = dir.child("commits", false).map_err(|e| {
        if e.code == "research_not_found" {
            error("incomplete_creation")
        } else {
            e
        }
    })?;
    let mut state = State::default();
    let mut seq = 1u64;
    let mut committed_bytes = 0u64;
    for name in commits.entries()? {
        if temporary_name(&name) {
            commits.file(&name, MAX_FRAME as u64)?;
            recovery.orphan_files += 1;
            continue;
        }
        if name != format!("{seq:020}.json") {
            return Err(error("corrupt_store"));
        }
        committed_bytes = committed_bytes
            .checked_add(
                commits
                    .file(&name, MAX_FRAME as u64)?
                    .metadata()
                    .map_err(io_error)?
                    .len(),
            )
            .ok_or_else(|| error("storage_limit"))?;
        if committed_bytes > MAX_PROJECT {
            return Err(error("storage_limit"));
        }
        let bytes = commits.bytes(&name, MAX_FRAME)?;
        let frame = json::decode(&bytes).map_err(|_| error("corrupt_store"))?;
        match frame.get("schema_version").and_then(Value::as_u64) {
            Some(1) if state.s2.transition.is_none() => {
                state::replay(owner, pid, &bytes, &mut state)?
            }
            Some(2) if state.project.is_some() => {
                super::evidence::replay(owner, pid, &bytes, &mut state).map_err(|e| {
                    if e.code == "research_unsupported_schema" {
                        e
                    } else {
                        error("corrupt_store")
                    }
                })?
            }
            _ => return Err(error("unsupported_schema")),
        }
        commits
            .file(&name, MAX_FRAME as u64)?
            .sync_all()
            .map_err(io_error)?;
        seq += 1;
    }
    commits.0.sync_all().map_err(io_error)?;
    let blobs = dir
        .child("blobs", false)
        .map_err(|_| error("corrupt_store"))?;
    for name in blobs.entries()? {
        blobs.file(&name, 32 * 1024 * 1024)?;
        recovery.orphan_files += 1;
    }
    if state.project.is_none() {
        return Err(error("incomplete_creation"));
    }
    Ok((state, recovery))
}
fn usage(dir: &Dir) -> Result<u64> {
    let mut size = 0u64;
    for sub in ["commits", "blobs"] {
        let d = dir.child(sub, false)?;
        for name in d.entries()? {
            let len = d
                .file(
                    &name,
                    if sub == "blobs" {
                        32 * 1024 * 1024
                    } else {
                        MAX_FRAME as u64
                    },
                )?
                .metadata()
                .map_err(io_error)?
                .len();
            size = size
                .checked_add(len)
                .ok_or_else(|| error("storage_limit"))?;
        }
    }
    Ok(size)
}
fn append(dir: &Dir, commit: &Commit) -> Result<()> {
    let bytes = state::canonical(commit)?;
    if usage(dir)?
        .checked_add(bytes.len() as u64)
        .map_or(true, |n| n > MAX_PROJECT)
    {
        return Err(error("storage_limit"));
    }
    dir.child("commits", false)?.publish(
        &format!("{:020}.json", commit.sequence),
        bytes.as_bytes(),
        Role::Commit,
    )
}
fn create(ctx: &Context, m: Mutation) -> Result<MutationResult> {
    let op = m.operation_id();
    let request = state::canonical(&m)?;
    let reservation = match ctx.reservation(op) {
        Ok(r) => {
            if r.initial.request != request {
                return Err(error("idempotency_conflict"));
            }
            r
        }
        Err(e) if e.code == "research_not_found" => {
            let identities = project_identities(ctx)?;
            let mut chosen = None;
            for _ in 0..16 {
                let candidate = native_id()?;
                if identities.contains_key(&candidate) {
                    continue;
                }
                match ctx.dir.child(&candidate, false) {
                    Err(e) if e.code == "research_not_found" => {
                        chosen = Some(candidate);
                        break;
                    }
                    Ok(_) => {}
                    Err(e) => return Err(e),
                }
            }
            let pid = chosen.ok_or_else(|| error("unavailable"))?;
            let st = State::default();
            let initial = state::build(&ctx.owner.owner_id, &pid, &st, &m, seeds(&st)?)?;
            let r = Reservation {
                schema_version: 1,
                initial,
            };
            ctx.dir.child("creates", false)?.publish(
                &format!("{op}.json"),
                state::canonical(&r)?.as_bytes(),
                Role::Metadata,
            )?;
            r
        }
        Err(e) => return Err(e),
    };
    fault(Phase::AfterReservation)?;
    let pid = &reservation.initial.project_id;
    let done = ctx.done(op, pid)?;
    let dir = ctx.dir.child(pid, !done).map_err(|e| {
        if done && e.code == "research_not_found" {
            error("missing_project_data")
        } else {
            e
        }
    })?;
    dir.lock(true)?;
    if !done {
        dir.child("commits", true)?;
        dir.child("blobs", true)?;
    }
    match load(&dir, &ctx.owner.owner_id, pid) {
        Ok((st, _)) => {
            v2::verify_snapshot(ctx, pid, &st)?;
            v2::ensure_no_pending(ctx, pid, &st, None)?;
            let old = st
                .operations
                .get(op)
                .ok_or_else(|| error("corrupt_store"))?;
            if old != &reservation.initial {
                return Err(error("corrupt_store"));
            }
        }
        Err(e) if e.code == "research_incomplete_creation" && !done => {
            // Only a proven empty commit directory can complete pending creation.
            let commits = dir.child("commits", false)?;
            if commits.entries()?.iter().any(|n| !temporary_name(n)) {
                return Err(error("corrupt_store"));
            }
            append(&dir, &reservation.initial)?;
        }
        Err(e) if e.code == "research_incomplete_creation" && done => {
            return Err(error("corrupt_store"));
        }
        Err(e) => return Err(e),
    }
    if !done {
        let d = CreationDone {
            schema_version: 1,
            project_id: pid.clone(),
            operation_id: op.into(),
        };
        ctx.dir.child("creates", false)?.publish(
            &format!("{op}.done"),
            state::canonical(&d)?.as_bytes(),
            Role::Metadata,
        )?;
    }
    Ok(reservation.initial.result)
}
fn mutate(ctx: &Context, m: Mutation) -> Result<MutationResult> {
    let pid = m.project_id().ok_or_else(|| error("invalid_input"))?;
    let (dir, st, _) = ctx.ready(pid, true)?;
    v3::legacy_guard(ctx, pid, &st, m.operation_id())?;
    if st.s2.transition.is_some() {
        return v2::legacy_mutate(ctx, &dir, &st, &m);
    }
    let request = state::canonical(&m)?;
    if let Some(old) = st.operations.get(m.operation_id()) {
        if old.request == request {
            return Ok(old.result.clone());
        }
        return Err(error("idempotency_conflict"));
    }
    let commit = state::build(&ctx.owner.owner_id, pid, &st, &m, seeds(&st)?)?;
    v2::reserve(ctx, pid, &dir, state::canonical(&commit)?.len())?;
    append(&dir, &commit)?;
    Ok(commit.result)
}
// Receipt acknowledgement records identity, not inferred project data health.
fn project_identities(ctx: &Context) -> Result<BTreeMap<String, bool>> {
    let mut projects = BTreeMap::new();
    for name in ctx.dir.child("creates", false)?.entries()? {
        if temporary_name(&name) {
            ctx.dir
                .child("creates", false)?
                .file(&name, MAX_FRAME as u64)?;
            continue;
        }
        if let Some(op) = name.strip_suffix(".json") {
            if !state::id(op) {
                return Err(error("corrupt_store"));
            }
            let r = ctx.reservation(op)?;
            let acknowledged = ctx.done(op, &r.initial.project_id)?;
            if projects
                .insert(r.initial.project_id, acknowledged)
                .is_some()
            {
                return Err(error("corrupt_store"));
            }
        } else if let Some(op) = name.strip_suffix(".done").filter(|op| state::id(op)) {
            let r = ctx.reservation(op).map_err(|e| {
                if e.code == "research_not_found" {
                    error("corrupt_store")
                } else {
                    e
                }
            })?;
            if !ctx.done(op, &r.initial.project_id)? {
                return Err(error("corrupt_store"));
            }
        } else {
            return Err(error("corrupt_store"));
        }
    }
    Ok(projects)
}
fn parse<T: DeserializeOwned>(v: Value) -> Result<T> {
    state::parse(v)
}
#[cfg(test)]
#[path = "tests.rs"]
mod tests;
#[path = "evidence_store.rs"]
mod v2;
#[path = "planning_store.rs"]
mod v3;
pub(super) fn dispatch(root: &Path, cmd: &str, args: Value) -> Result<Value> {
    // Validate the full request before opening/creating any Research directory.
    json::canonical(&args)?;
    if super::planning::handles(cmd) {
        return v3::dispatch(root, cmd, args);
    }
    if v2::handles(cmd, &args) {
        return v2::dispatch(root, cmd, args);
    }
    let mutation = match cmd {
        "research_project_create" => Some(Mutation::Create(parse(args.clone())?)),
        "research_project_update" => Some(Mutation::Update(parse(args.clone())?)),
        "research_question_save" => Some(Mutation::Question(parse(args.clone())?)),
        "research_plan_revision_save" => Some(Mutation::Plan(parse(args.clone())?)),
        _ => None,
    };
    if let Some(m) = mutation {
        state::validate(&m)?;
        let creating = matches!(m, Mutation::Create(_));
        let ctx = Context::open(root, creating)?;
        return state::value(&if creating {
            create(&ctx, m)?
        } else {
            mutate(&ctx, m)?
        });
    }
    match cmd {
        "research_project_list" => {
            let a: ListArgs = parse(args)?;
            state::schema(a.schema_version)?;
            state::page(a.limit)?;
            if a.cursor.as_ref().is_some_and(|s| !state::id(s)) {
                return Err(error("invalid_input"));
            }
            let ctx = match Context::open(root, false) {
                Ok(c) => c,
                Err(e) if e.code == "research_not_found" => {
                    return state::value(&ProjectPage {
                        owner_id: None,
                        items: vec![],
                        next_cursor: None,
                    })
                }
                Err(e) => return Err(e),
            };
            let mut ids: Vec<_> = project_identities(&ctx)?.into_keys().collect();
            for n in ctx.dir.entries()? {
                if n == "creates" {
                    continue;
                }
                if !state::id(&n) {
                    return Err(error("corrupt_store"));
                }
                ids.push(n)
            }
            ids.sort();
            ids.dedup();
            ids.retain(|n| a.cursor.as_ref().map_or(true, |c| n > c));
            let more = ids.len() > a.limit as usize;
            ids.truncate(a.limit as usize);
            let next = if more { ids.last().cloned() } else { None };
            let mut items = Vec::new();
            for pid in ids {
                let entry = match ctx.ready(&pid, false) {
                    Ok((_, st, _)) => ProjectEntry {
                        project_id: pid,
                        status: "available".into(),
                        project: st.project,
                        reason: None,
                    },
                    Err(e) => ProjectEntry {
                        project_id: pid,
                        status: if e.code == "research_incomplete_creation" {
                            "pending_creation"
                        } else {
                            "unavailable"
                        }
                        .into(),
                        project: None,
                        reason: Some(e.code),
                    },
                };
                items.push(entry);
            }
            state::value(&ProjectPage {
                owner_id: Some(ctx.owner.owner_id.clone()),
                items,
                next_cursor: next,
            })
        }
        "research_project_get" => {
            let a: ProjectArgs = parse(args)?;
            state::schema(a.schema_version)?;
            if !state::id(&a.project_id) {
                return Err(error("invalid_input"));
            }
            let ctx = Context::open(root, false)?;
            let (_, st, recovery) = ctx.ready(&a.project_id, false)?;
            state::value(&ProjectView {
                project: st.project.ok_or_else(|| error("corrupt_store"))?,
                recovery,
            })
        }
        "research_record_get" => {
            let a: RecordArgs = parse(args)?;
            state::schema(a.schema_version)?;
            state::reference(&a.reference)?;
            if !state::id(&a.project_id) {
                return Err(error("invalid_input"));
            }
            let ctx = Context::open(root, false)?;
            let (_, st, _) = ctx.ready(&a.project_id, false)?;
            state::value(st.get(&a.reference)?)
        }
        "research_events_page" => {
            let a: EventsArgs = parse(args)?;
            state::schema(a.schema_version)?;
            state::page(a.limit)?;
            if !state::id(&a.project_id) || a.cursor > SAFE_INTEGER {
                return Err(error("invalid_input"));
            }
            let ctx = Context::open(root, false)?;
            let (_, st, _) = ctx.ready(&a.project_id, false)?;
            let mut items: Vec<_> = st
                .events
                .into_iter()
                .filter(|e| e.project_sequence > a.cursor)
                .collect();
            let more = items.len() > a.limit as usize;
            items.truncate(a.limit as usize);
            let next = if more {
                items.last().map(|e| e.project_sequence)
            } else {
                None
            };
            state::value(&EventPage {
                items,
                next_cursor: next,
            })
        }
        "research_operation_status" => {
            let a: OperationArgs = parse(args)?;
            state::schema(a.schema_version)?;
            if !state::id(&a.operation_id) || a.project_id.as_ref().is_some_and(|p| !state::id(p)) {
                return Err(error("invalid_input"));
            }
            let ctx = match Context::open(root, false) {
                Ok(c) => c,
                Err(e) if e.code == "research_not_found" => {
                    return state::value(&OperationStatus {
                        status: "unknown".into(),
                        project_id: a.project_id,
                        result: None,
                    })
                }
                Err(e) => return Err(e),
            };
            let (pid, result) = if let Some(pid) = a.project_id {
                let (_, st, _) = ctx.ready(&pid, false)?;
                if st.s2.operations.contains_key(&a.operation_id)
                    && !st.operations.contains_key(&a.operation_id)
                {
                    return Err(error("unsupported_schema"));
                }
                let result = st.operations.get(&a.operation_id).map(|c| c.result.clone());
                (Some(pid), result)
            } else {
                match ctx.reservation(&a.operation_id) {
                    Ok(r) => {
                        let pid = r.initial.project_id;
                        if !ctx.done(&a.operation_id, &pid)? {
                            return state::value(&OperationStatus {
                                status: "pending".into(),
                                project_id: Some(pid),
                                result: None,
                            });
                        }
                        let (_, st, _) = ctx.ready(&pid, false)?;
                        let result = st.operations.get(&a.operation_id).map(|c| c.result.clone());
                        (Some(pid), result)
                    }
                    Err(e) if e.code == "research_not_found" => (None, None),
                    Err(e) => return Err(e),
                }
            };
            state::value(&OperationStatus {
                status: if result.is_some() {
                    "complete"
                } else {
                    "unknown"
                }
                .into(),
                project_id: pid,
                result,
            })
        }
        _ => Err(error("invalid_input")),
    }
}
