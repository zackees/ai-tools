#!/usr/bin/env bash
# Shared shell environment for ci/build, ci/lint, ci/test.
#
# Locates the local-install copy of soldr (./install puts it in
# .venv/{bin,Scripts}/soldr) and prepends that directory to PATH so the
# rest of the script can just call `soldr cargo ...` without worrying
# about which platform we're on.
#
# Source, don't execute: `source ci/env.sh`.

set -euo pipefail

# Resolve repo root regardless of where the wrapper was invoked from.
# `ci/env.sh` is at $repo_root/ci/env.sh, so go one up.
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export REPO_ROOT

# Prefer the local .venv install (matches `./install` without --global).
# Fall back to whatever soldr is already on PATH (matches `./install --global`).
for candidate in \
    "$REPO_ROOT/.venv/Scripts" \
    "$REPO_ROOT/.venv/bin"; do
    if [[ -x "$candidate/soldr" || -x "$candidate/soldr.exe" ]]; then
        export PATH="$candidate:$PATH"
        break
    fi
done

if ! command -v soldr >/dev/null 2>&1; then
    cat >&2 <<EOF
error: soldr not found on PATH and not present in $REPO_ROOT/.venv

Run \`./install\` (local) or \`./install --global\` first.
See https://github.com/zackees/soldr for details.
EOF
    exit 1
fi
