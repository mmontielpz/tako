"""TAKO sealed evidence v1: offline Ed25519 authenticity, never execution authority.

No production keys, trust anchors or replay databases are created at import.
Cryptography is an optional, separately declared dependency, not a Rust dependency.
"""
from __future__ import annotations
import hashlib
import json
import os
from pathlib import Path
import re
import sqlite3
import stat
from urllib.parse import quote
from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey, Ed25519PublicKey
from cryptography.hazmat.primitives import serialization

LIMIT = 4 * 1024 * 1024
SAFE_INT = 2**53 - 1
DOMAIN = b"TAKO-SIGNED-EVIDENCE-V1\x00"
ROLES = {"snapshot", "execution", "verification", "isolation"}
HEX = re.compile(r"^[0-9a-f]+$")
ENVELOPE_VERSION = "tako.signed-evidence.v1"
PAYLOAD_VERSION = "tako.sealed-evidence.v1"
TRUST_VERSION = "tako.evidence-trust.v1"

class AuthError(ValueError):
    pass

def require(condition, code):
    if not condition:
        raise AuthError(code)

def exact(obj, names):
    require(isinstance(obj, dict) and set(obj) == set(names.split()), "unexpected_or_missing_fields")

def token(s):
    return isinstance(s, str) and 0 < len(s) <= 256 and s.strip() == s and all(c.isprintable() for c in s)

def integer(n, minimum=0):
    return type(n) is int and minimum <= n <= SAFE_INT

def hex_bytes(s, size):
    require(isinstance(s, str) and len(s) == size * 2 and HEX.fullmatch(s), "invalid_hex_encoding")
    return bytes.fromhex(s)

def canonical(value):
    """TAKO CJ1 profile, deliberately NOT RFC 8785/JCS: no floats or surrogate strings."""
    def visit(v, depth=0):
        require(depth <= 32, "json_nesting_limit")
        if v is None or type(v) is bool:
            return
        if type(v) is int:
            require(-SAFE_INT <= v <= SAFE_INT, "integer_out_of_range")
        elif isinstance(v, str):
            try:
                v.encode("utf-8", errors="strict")
            except UnicodeError as exc:
                raise AuthError("invalid_unicode_scalar") from exc
        elif isinstance(v, list):
            require(len(v) <= 1000, "array_limit")
            for item in v:
                visit(item, depth + 1)
        elif isinstance(v, dict):
            require(len(v) <= 1000 and all(isinstance(k, str) and k.isascii() for k in v), "invalid_object_keys")
            for item in v.values():
                visit(item, depth + 1)
        else:
            raise AuthError("unsupported_json_type")
    visit(value)
    blob = json.dumps(value, sort_keys=True, ensure_ascii=True, separators=(",", ":"), allow_nan=False).encode("ascii")
    require(len(blob) <= LIMIT, "json_size_limit")
    return blob

def parse(blob, canonical_only=False):
    require(isinstance(blob, bytes) and len(blob) <= LIMIT, "json_size_limit")
    def pairs(items):
        result = {}
        for k, v in items:
            require(k not in result, "duplicate_json_key")
            result[k] = v
        return result
    def nonfinite(_):
        raise AuthError("nonfinite_number")
    try:
        value = json.loads(blob, object_pairs_hook=pairs, parse_constant=nonfinite)
        normalized = canonical(value)
    except (UnicodeError, RecursionError, json.JSONDecodeError) as exc:
        raise AuthError("invalid_json") from exc
    require(not canonical_only or normalized == blob, "noncanonical_encoding")
    return value

def sha(blob):
    return hashlib.sha256(blob).hexdigest()

def identity(value):
    exact(value, "backend_id project_id mission_id")
    require(all(token(v) for v in value.values()), "invalid_identity")
    return value

def _parent(path):
    path = Path(path).absolute()
    # Every ancestor is part of the trust boundary, not only the final directory.
    # Root-owned sticky directories (e.g. /tmp) protect owned child entries.
    for directory in (path.parent, *path.parent.parents):
        info = directory.lstat()
        require(stat.S_ISDIR(info.st_mode) and not directory.is_symlink(), "unsafe_private_directory")
        if hasattr(os, "getuid"):
            sticky_root = info.st_uid == 0 and bool(info.st_mode & stat.S_ISVTX)
            require(info.st_uid in {0, os.getuid()} and (not info.st_mode & 0o022 or sticky_root), "unsafe_private_directory")
    return path

def read_local(path, private=False):
    path = _parent(path)
    try:
        require(not path.is_symlink(), "unsafe_local_file")
        flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_NONBLOCK", 0) | getattr(os, "O_BINARY", 0)
        fd = os.open(path, flags)
        with os.fdopen(fd, "rb") as stream:
            info = os.fstat(stream.fileno())
            require(stat.S_ISREG(info.st_mode) and info.st_nlink == 1, "unsafe_local_file")
            if hasattr(os, "getuid"):
                require(info.st_uid in {0, os.getuid()}, "unsafe_local_file")
                require(not info.st_mode & (0o077 if private else 0o022), "unsafe_local_permissions")
            blob = stream.read(LIMIT + 1)
            require(len(blob) <= LIMIT, "local_file_too_large")
            return blob
    except OSError as exc:
        raise AuthError("local_file_unavailable") from exc

def write_new(path, blob):
    path = _parent(path)
    try:
        require(not path.is_symlink(), "unsafe_local_file")
        flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_BINARY", 0)
        fd = os.open(path, flags, 0o600)
        with os.fdopen(fd, "wb") as stream:
            stream.write(blob)
            stream.flush()
            os.fsync(stream.fileno())
        if hasattr(os, "O_DIRECTORY"):
            parent_fd = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY | getattr(os, "O_NOFOLLOW", 0))
            try:
                os.fsync(parent_fd)
            finally:
                os.close(parent_fd)
    except OSError as exc:
        raise AuthError("exclusive_write_failed") from exc

def generate_key(path, key_id, issuer_id, role):
    require(token(key_id) and token(issuer_id) and role in ROLES, "invalid_key_identity")
    key = Ed25519PrivateKey.generate()
    public = key.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw).hex()
    secret = key.private_bytes(serialization.Encoding.Raw, serialization.PrivateFormat.Raw, serialization.NoEncryption()).hex()
    credential = {"schema_version": "tako.signing-key.v1", "key_id": key_id, "issuer_id": issuer_id,
                  "role": role, "private_key_hex": secret}
    write_new(path, canonical(credential))
    # No automatic enrollment and no secret output.
    return {"key_id": key_id, "issuer_id": issuer_id, "role": role, "public_key_hex": public}

def _content_binding(content, role):
    if role == "snapshot":
        exact(content, "schema_version identity revision observed_at_ms expires_at_ms availability reason lifecycle execution artifacts verification isolation acceptance")
        require(content["schema_version"] == "tako.backend-evidence.v1" and content["availability"] == "available"
                and content["reason"] is None and content["acceptance"] == "not_supported", "not_an_available_v1_snapshot")
        x = content["execution"]
        require(isinstance(x, dict), "execution_missing")
        binding = (identity(content["identity"]), content["revision"], x.get("execution_id"), x.get("attempt"), x.get("subject_sha256"))
    else:
        exact(content, "schema_version identity revision execution_id attempt subject_sha256 issuer_id claim")
        require(content["schema_version"] == "tako.backend-artifact.v1" and isinstance(content["claim"], dict)
                and content["claim"].get("kind") == role, "artifact_kind_mismatch")
        binding = (identity(content["identity"]), content["revision"], content["execution_id"], content["attempt"], content["subject_sha256"])
    require(token(binding[1]) and token(binding[2]) and integer(binding[3], 1), "invalid_content_binding")
    hex_bytes(binding[4], 32)
    return binding

def seal(content_bytes, key_path, sequence, issued_at_ms, expires_at_ms):
    """Sign existing records only. Does not create outcomes, checks, timestamps or IDs."""
    credential = parse(read_local(key_path, private=True))
    exact(credential, "schema_version key_id issuer_id role private_key_hex")
    require(credential["schema_version"] == "tako.signing-key.v1" and token(credential["role"]) and credential["role"] in ROLES
            and token(credential["key_id"]) and token(credential["issuer_id"]), "invalid_signing_credential")
    content = parse(content_bytes)
    role = credential["role"]
    bound, revision, execution_id, attempt, subject = _content_binding(content, role)
    if role != "snapshot":
        require(content["issuer_id"] == credential["issuer_id"], "signer_does_not_match_issuer")
    require(integer(sequence, 1) and integer(issued_at_ms) and integer(expires_at_ms)
            and 0 < expires_at_ms - issued_at_ms <= 300_000, "invalid_issuance_window")
    if role == "snapshot":
        require(content["observed_at_ms"] == issued_at_ms and expires_at_ms <= content["expires_at_ms"], "snapshot_issuance_mismatch")
    payload = {"schema_version": PAYLOAD_VERSION, "kind": role, "identity": bound, "revision": revision,
        "execution_id": execution_id, "attempt": attempt, "sequence": sequence, "subject_sha256": subject,
        "issued_at_ms": issued_at_ms, "expires_at_ms": expires_at_ms,
        "content_sha256": sha(canonical(content)), "content": content}
    unsigned = {"schema_version": ENVELOPE_VERSION, "key_id": credential["key_id"], "payload": payload}
    key = Ed25519PrivateKey.from_private_bytes(hex_bytes(credential["private_key_hex"], 32))
    return canonical({**unsigned, "signature_hex": key.sign(DOMAIN + canonical(unsigned)).hex()})

def load_policy(path):
    policy = parse(read_local(path))
    exact(policy, "schema_version policy_id state_id generation backend_id keys")
    require(policy["schema_version"] == TRUST_VERSION and all(token(policy[k]) for k in ["policy_id", "state_id", "backend_id"])
            and integer(policy["generation"], 1) and isinstance(policy["keys"], list) and policy["keys"], "invalid_trust_policy")
    ids, material, issuers = set(), {}, {}
    for key in policy["keys"]:
        exact(key, "key_id issuer_id role public_key_hex project_ids not_before_ms not_after_ms revoked")
        require(token(key["key_id"]) and token(key["issuer_id"]) and token(key["role"]) and key["role"] in ROLES
                and key["key_id"] not in ids and type(key["revoked"]) is bool, "invalid_trust_key")
        require(isinstance(key["project_ids"], list) and key["project_ids"] and all(token(p) for p in key["project_ids"])
                and len(set(key["project_ids"])) == len(key["project_ids"]), "invalid_project_scope")
        require(integer(key["not_before_ms"]) and integer(key["not_after_ms"])
                and key["not_before_ms"] < key["not_after_ms"], "invalid_key_lifetime")
        hex_bytes(key["public_key_hex"], 32)
        ids.add(key["key_id"])
        material.setdefault(key["public_key_hex"], set()).add(key["role"])
        issuers.setdefault(key["issuer_id"], set()).add(key["role"])
    # Multiple keys/labels cannot manufacture role independence from one signer.
    for roles in list(material.values()) + list(issuers.values()):
        require(not ({"verification", "isolation"} & roles and {"snapshot", "execution"} & roles), "nonindependent_trust_roles")
    return policy

def _connect(path):
    # mode=rw never creates state on verification. A lost database is a hard failure.
    read_local(path, private=True)
    conn = None
    try:
        conn = sqlite3.connect("file:" + quote(str(Path(path).absolute()), safe="/") + "?mode=rw", uri=True,
                               isolation_level=None, timeout=3)
        require(conn.execute("PRAGMA journal_mode").fetchone()[0] == "delete", "unexpected_journal_mode")
        conn.execute("PRAGMA synchronous=EXTRA")
        conn.execute("PRAGMA trusted_schema=OFF")
        require(conn.execute("PRAGMA synchronous").fetchone()[0] == 3, "durability_unavailable")
        return conn
    except (sqlite3.Error, AuthError) as exc:
        if conn is not None:
            conn.close()
        if isinstance(exc, AuthError):
            raise
        raise AuthError("replay_state_unavailable") from exc

def init_state(path, policy_path):
    policy = load_policy(policy_path)
    write_new(path, b"")
    conn = _connect(path)
    try:
        conn.execute("BEGIN IMMEDIATE")
        conn.execute("CREATE TABLE metadata (id INTEGER PRIMARY KEY CHECK(id=1), version TEXT NOT NULL, policy_id TEXT NOT NULL, state_id TEXT NOT NULL, generation INTEGER NOT NULL, policy_hash TEXT NOT NULL, last_now_ms INTEGER NOT NULL)")
        conn.execute("CREATE TABLE seen (backend TEXT NOT NULL, project TEXT NOT NULL, mission TEXT NOT NULL, sequence INTEGER NOT NULL, PRIMARY KEY(backend,project,mission))")
        conn.execute("CREATE TABLE revisions (backend TEXT NOT NULL, project TEXT NOT NULL, mission TEXT NOT NULL, revision TEXT NOT NULL, content_hash TEXT NOT NULL, PRIMARY KEY(backend,project,mission,revision))")
        conn.execute("INSERT INTO metadata VALUES (1,?,?,?,?,?,?)", ("tako.replay-state.v1", policy["policy_id"], policy["state_id"], policy["generation"], sha(canonical(policy)), 0))
        conn.commit()
    finally:
        conn.close()

def _metadata(conn, policy, now_ms):
    row = conn.execute("SELECT version,policy_id,state_id,generation,policy_hash,last_now_ms FROM metadata WHERE id=1").fetchone()
    require(row is not None and row[:3] == ("tako.replay-state.v1", policy["policy_id"], policy["state_id"]), "wrong_replay_state")
    require(integer(now_ms) and now_ms >= row[5], "clock_rollback")
    require(policy["generation"] >= row[3], "trust_policy_rollback")
    require(policy["generation"] != row[3] or sha(canonical(policy)) == row[4], "trust_policy_changed_without_generation")
    return row

def _pin_policy(state_path, policy, now_ms):
    conn = _connect(state_path)
    try:
        conn.execute("BEGIN IMMEDIATE")
        _metadata(conn, policy, now_ms)
        conn.execute("UPDATE metadata SET generation=?,policy_hash=? WHERE id=1", (policy["generation"], sha(canonical(policy))))
        conn.commit()
    except sqlite3.Error as exc:
        raise AuthError("replay_state_unavailable") from exc
    finally:
        conn.close()

def _verify_envelope(blob, role, policy, expected, now_ms):
    env = parse(blob, canonical_only=True)
    exact(env, "schema_version key_id payload signature_hex")
    require(env["schema_version"] == ENVELOPE_VERSION and token(env["key_id"]), "invalid_signed_envelope")
    p = env["payload"]
    exact(p, "schema_version kind identity revision execution_id attempt sequence subject_sha256 issued_at_ms expires_at_ms content_sha256 content")
    require(p["schema_version"] == PAYLOAD_VERSION and p["kind"] == role and identity(p["identity"]) == expected, "signed_identity_or_role_mismatch")
    require(integer(p["attempt"], 1) and token(p["execution_id"]) and token(p["revision"])
            and integer(p["sequence"], 1) and integer(p["issued_at_ms"]) and integer(p["expires_at_ms"])
            and 0 < p["expires_at_ms"] - p["issued_at_ms"] <= 300_000
            and p["issued_at_ms"] <= now_ms < p["expires_at_ms"], "expired_or_future_envelope")
    require(_content_binding(p["content"], role) == (expected, p["revision"], p["execution_id"], p["attempt"], p["subject_sha256"]), "signed_content_binding_mismatch")
    require(sha(canonical(p["content"])) == p["content_sha256"], "signed_content_digest_mismatch")
    if role == "snapshot":
        require(p["content"]["observed_at_ms"] == p["issued_at_ms"]
                and p["expires_at_ms"] <= p["content"]["expires_at_ms"], "snapshot_issuance_mismatch")
    keys = [k for k in policy["keys"] if k["key_id"] == env["key_id"]]
    require(len(keys) == 1, "unknown_signing_key")
    key = keys[0]
    require(not key["revoked"], "revoked_signing_key")
    require(key["role"] == role and expected["backend_id"] == policy["backend_id"]
            and expected["project_id"] in key["project_ids"], "unauthorized_signing_scope")
    require(key["not_before_ms"] <= p["issued_at_ms"] <= now_ms < key["not_after_ms"]
            and p["expires_at_ms"] <= key["not_after_ms"], "expired_or_not_yet_valid_key")
    if role != "snapshot":
        require(p["content"]["issuer_id"] == key["issuer_id"], "trusted_issuer_mismatch")
    unsigned = {k: env[k] for k in ["schema_version", "key_id", "payload"]}
    try:
        Ed25519PublicKey.from_public_bytes(hex_bytes(key["public_key_hex"], 32)).verify(
            hex_bytes(env["signature_hex"], 64), DOMAIN + canonical(unsigned))
    except InvalidSignature as exc:
        raise AuthError("invalid_signature") from exc
    return p, key

def verify_bundle(snapshot_blob, artifact_blobs, policy_path, state_path, expected, now_ms):
    """Authenticate a complete bundle and atomically consume its per-mission sequence.

    This returns an authenticity receipt, NOT a v1 semantic verification or sandbox
    verdict. No product Authority, remote acceptance or provider is activated.
    """
    expected = identity(expected)
    policy = load_policy(policy_path)
    _pin_policy(state_path, policy, now_ms)
    snapshot, snapshot_key = _verify_envelope(snapshot_blob, "snapshot", policy, expected, now_ms)
    record = snapshot["content"]
    require(isinstance(record["verification"], dict) and isinstance(record["isolation"], dict), "invalid_result_records")
    refs = record["artifacts"]
    require(isinstance(refs, list) and len(refs) <= 1000 and isinstance(artifact_blobs, dict), "invalid_artifact_set")
    require(len(artifact_blobs) == len(refs), "missing_or_extra_artifact_envelopes")
    seen, kinds, total = set(), {}, len(snapshot_blob)
    for ref in refs:
        exact(ref, "artifact_id mission_id execution_id kind sha256 issuer_id")
        require(token(ref["artifact_id"]) and ref["artifact_id"] not in seen and token(ref["kind"]) and ref["kind"] in ROLES - {"snapshot"}, "invalid_artifact_reference")
        seen.add(ref["artifact_id"])
        require(ref["artifact_id"] in artifact_blobs, "artifact_envelope_missing")
        raw = artifact_blobs[ref["artifact_id"]]
        total += len(raw)
        require(total <= LIMIT, "bundle_size_limit")
        item, key = _verify_envelope(raw, ref["kind"], policy, expected, now_ms)
        for field in ["revision", "execution_id", "attempt", "sequence", "subject_sha256", "issued_at_ms", "expires_at_ms"]:
            require(item[field] == snapshot[field], "cross_attempt_revision_or_sequence")
        doc = item["content"]
        require(ref["mission_id"] == expected["mission_id"] and ref["execution_id"] == snapshot["execution_id"]
                and ref["issuer_id"] == key["issuer_id"] and sha(canonical(doc)) == ref["sha256"], "artifact_reference_mismatch")
        claim = doc["claim"]
        if ref["kind"] in {"execution", "verification"}:
            exact(claim, "kind record")
            require(claim["record"] == record[ref["kind"]], "artifact_claim_mismatch")
        else:
            # Authenticity of an isolation assertion is NOT proof of enforcement.
            require(claim.get("level") == record["isolation"].get("claimed_level"), "isolation_binding_mismatch")
        kinds[ref["artifact_id"]] = (ref["kind"], key["issuer_id"])
    x, verification = record["execution"], record["verification"]
    require(isinstance(verification, dict) and isinstance(record["isolation"], dict), "invalid_result_records")
    require(kinds.get(x.get("provenance_id")) == ("execution", x.get("runner_id")), "execution_provenance_missing")
    if verification.get("status") in {"passed", "failed"}:
        verifier = kinds.get(verification.get("evidence_id"))
        require(verifier is not None and verifier[0] == "verification"
                and verifier[1] not in {x.get("runner_id"), expected["backend_id"], snapshot_key["issuer_id"]}, "independent_verifier_missing")
        require(verification.get("subject_sha256") == snapshot["subject_sha256"], "verification_subject_mismatch")
    if record["isolation"].get("evidence_id") is not None:
        iso = kinds.get(record["isolation"]["evidence_id"])
        require(iso is not None and iso[0] == "isolation", "isolation_artifact_missing")
    conn = _connect(state_path)
    try:
        conn.execute("BEGIN IMMEDIATE")
        metadata = _metadata(conn, policy, now_ms)
        require(metadata[3] == policy["generation"] and metadata[4] == sha(canonical(policy)), "trust_policy_changed_during_verification")
        scope = (expected["backend_id"], expected["project_id"], expected["mission_id"])
        old = conn.execute("SELECT sequence FROM seen WHERE backend=? AND project=? AND mission=?", scope).fetchone()
        require(old is None or snapshot["sequence"] > old[0], "replayed_or_rolled_back_sequence")
        prior = conn.execute("SELECT content_hash FROM revisions WHERE backend=? AND project=? AND mission=? AND revision=?", (*scope, snapshot["revision"])).fetchone()
        require(prior is None or prior[0] == snapshot["content_sha256"], "immutable_revision_substituted")
        conn.execute("INSERT INTO seen VALUES (?,?,?,?) ON CONFLICT(backend,project,mission) DO UPDATE SET sequence=excluded.sequence", (*scope, snapshot["sequence"]))
        conn.execute("INSERT OR IGNORE INTO revisions VALUES (?,?,?,?,?)", (*scope, snapshot["revision"], snapshot["content_sha256"]))
        conn.execute("UPDATE metadata SET last_now_ms=? WHERE id=1", (now_ms,))
        conn.commit()
    except sqlite3.Error as exc:
        raise AuthError("replay_state_unavailable") from exc
    finally:
        conn.close()
    return {"authenticity": "authenticated", "identity": expected, "execution_id": snapshot["execution_id"],
        "attempt": snapshot["attempt"], "revision": snapshot["revision"], "sequence": snapshot["sequence"],
        "verification": "authenticated_claims_only", "isolation": "unverified", "remote_acceptance_enabled": False,
        "l1_badge_enabled": False, "provider_execution_enabled": False}
