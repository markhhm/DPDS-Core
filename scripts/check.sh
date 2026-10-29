#!/bin/sh
# SPDX-License-Identifier: Apache-2.0
# The complete check suite. The pre-push hook and CI both run exactly this script.
set -eu
cd "$(git rev-parse --show-toplevel)"
if ! "${PYTHON:-python3}" -c 'import numpy' >/dev/null 2>&1; then
    echo "NumPy is missing or cannot be imported by ${PYTHON:-python3}." >&2
    echo "Use a Python 3.11+ venv; run: python -m pip install -r scripts/trace-requirements.txt" >&2
    echo "Set PYTHON to that environment's interpreter, then rerun scripts/check.sh." >&2
    exit 1
fi
scripts/check-policy.sh
scripts/check-policy-test.sh
. scripts/find-cargo.sh
"$CARGO" fmt --check
"$CARGO" clippy --locked --all-targets -- -D warnings
"$CARGO" test --locked
"$CARGO" build --locked --bin trace_replay
PYTHONDONTWRITEBYTECODE=1 "${PYTHON:-python3}" -m unittest discover -s tests -p 'test_*.py'
# A check that rewrites tracked files (for example a stale Cargo.lock) must fail.
if [ -n "$(git status --porcelain)" ]; then
    echo "Checks modified the working tree:" >&2
    git status --short >&2
    exit 1
fi
echo "check.sh: all checks passed"
