#!/bin/sh
# SPDX-License-Identifier: Apache-2.0
# The complete check suite. The pre-push hook and CI both run exactly this script.
set -eu
cd "$(git rev-parse --show-toplevel)"
scripts/check-policy.sh
scripts/check-policy-test.sh
. scripts/find-cargo.sh
"$CARGO" fmt --check
"$CARGO" clippy --locked --all-targets -- -D warnings
"$CARGO" test --locked
# A check that rewrites tracked files (for example a stale Cargo.lock) must fail.
if [ -n "$(git status --porcelain)" ]; then
    echo "Checks modified the working tree:" >&2
    git status --short >&2
    exit 1
fi
echo "check.sh: all checks passed"
