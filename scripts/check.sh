#!/usr/bin/env bash
# Repository hygiene checks. Documentation-stage only; no application tests exist yet.
set -u
cd "$(dirname "$0")/.."
fail=0
err() { echo "FAIL: $*"; fail=1; }

# 1. Required files
for f in README.md AGENTS.md CLAUDE.md CONTRIBUTING.md CODE_OF_CONDUCT.md SECURITY.md CHANGELOG.md \
         CURRENT_STATE.md .gitignore .github/PULL_REQUEST_TEMPLATE.md pyproject.toml \
         docs/contracts/backend-evidence/v1/conformance-example.json \
         tools/cli.py tools/doctor.py tools/sota_integrity.py tools/sota_discovery/sota.py tools/sota_discovery/engine.py tools/sota_ai_native/workflow.py \
         docs/product/PRODUCT_VISION.md docs/product/MVP_SCOPE.md \
         docs/architecture/ARCHITECTURE_PRINCIPLES.md docs/architecture/OPEN_SOURCE_BOUNDARY.md \
         docs/decisions/0001-license.md LICENSE NOTICE \
         docs/architecture/DESKTOP_STACK_DECISION.md docs/architecture/CORE_CONTRACTS.md docs/architecture/FIRST_WORKFLOW.md \
         docs/product/UX_SPEC.md docs/product/COMPANION_SPEC.md docs/product/DESIGN_SYSTEM.md docs/product/ROADMAP.md \
         docs/architecture/SECURITY_LIMITATIONS.md docs/architecture/ISOLATION_DECISION.md \
         bridge/src/lib.rs bridge/tests/bridge.rs ui/package.json ui/package-lock.json ui/src/App.tsx ui/src/companion.ts \
         src-tauri/tauri.conf.json src-tauri/src/main.rs src-tauri/capabilities/default.json Cargo.toml Cargo.lock src/lib.rs tests/engine.rs; do
  [ -f "$f" ] || err "missing $f"
done

# 2. LICENSE must be the Apache-2.0 text; NOTICE must exist
[ -f LICENSE ] && grep -q "Apache License" LICENSE && grep -q "Version 2.0, January 2004" LICENSE || err "LICENSE missing or not Apache-2.0"
[ -f NOTICE ] || err "missing NOTICE"

# 2b. Unresolved placeholders (warning only; must be resolved before publication)
grep -n "\[INSERT CONTACT METHOD\]" CODE_OF_CONDUCT.md >/dev/null && echo "WARN: CODE_OF_CONDUCT.md has no reporting contact yet (publication blocker)"

# 3. Local markdown links resolve
while IFS= read -r md; do
  dir=$(dirname "$md")
  grep -oE '\]\(([^)#]+)(#[^)]*)?\)' "$md" | sed -E 's/^\]\(([^)#]+).*/\1/' | while read -r link; do
    case "$link" in http*|mailto:*) continue ;; esac
    [ -e "$dir/$link" ] || { echo "FAIL: broken link in $md -> $link"; exit 1; }
  done || fail=1
done < <(git ls-files -co --exclude-standard '*.md')

# 4. Secret-looking patterns in tracked/untracked files (this script excluded)
pat='(AKIA[0-9A-Z]{16}|gh[pousr]_[A-Za-z0-9]{30,}|sk-[A-Za-z0-9]{20,}|-----BEGIN [A-Z ]*PRIVATE KEY-----)'
if git ls-files -co --exclude-standard | grep -v '^scripts/check.sh$' | xargs grep -EIn "$pat" 2>/dev/null; then
  err "possible secret found"
fi

[ $fail -eq 0 ] && echo "ALL CHECKS PASSED" || { echo "CHECKS FAILED"; exit 1; }
