#!/bin/sh
# SPDX-License-Identifier: Apache-2.0
# Self-test: generic rules and an optional private word list must remain effective.
set -eu
for name in $(git rev-parse --local-env-vars); do unset "$name"; done
unset HNG_POLICY_WORDS_FILE
policy="$(cd "$(dirname "$0")" && pwd)/check-policy.sh"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
fails=0
printf '%s\n' fictional-host fictional-worker > "$work/words.txt"
owner=captainhung096@gmail.com
metadata_commit() {
    printf '%b\n' "$3" > "$work/message"
    GIT_AUTHOR_NAME=Mark GIT_AUTHOR_EMAIL="$1" \
    GIT_COMMITTER_NAME=Mark GIT_COMMITTER_EMAIL="$2" \
        git -c commit.gpgsign=false commit -q --allow-empty -F "$work/message"
}
fresh() {
    rm -rf "$work/repo" && mkdir -p "$work/repo/scripts" && cd "$work/repo"
    git init -q .
    metadata_commit upstream@example.invalid upstream@example.invalid "upstream baseline"
    git update-ref refs/remotes/origin/main HEAD
    printf '# SPDX-License-Identifier: Apache-2.0\n' > scripts/spdx-exempt.txt
    printf '# SPDX-License-Identifier: Apache-2.0\n' > scripts/env-allow.txt
    printf '// SPDX-License-Identifier: Apache-2.0\nfn main() {}\n' > ok.rs
    printf '{"synthetic": true}\n' > data.json
    unset HNG_POLICY_WORDS_FILE
}
expect() {
    if "$policy" >"$work/out" 2>&1; then got=pass; else got=fail; fi
    if [ "$got" = "$1" ]; then echo "ok: $2"; else echo "FAILED: $2 (got $got)"; cat "$work/out"; fails=1; fi
}
bad_line() { fresh; printf '<!-- SPDX-License-Identifier: Apache-2.0 -->\n%s\n' "$1" > doc.md; git add -A; expect fail "$2"; }
fresh; git add -A; expect pass "clean tree without a local list"
fresh; export HNG_POLICY_WORDS_FILE="$work/words.txt"; git add -A; expect pass "clean tree with a local list"
fresh; printf 'fn main() {}\n' > new.rs; git add -A; expect fail "missing SPDX header"
fresh; printf 'fn main() {}\n' > old.rs; echo old.rs >> scripts/spdx-exempt.txt; git add -A; expect pass "listed upstream exemption"
fresh; printf 'x\n' > old.rs; git add -A; printf 'y\n' > untracked.rs; expect fail "tracked file without header despite untracked noise"
fresh; printf '<!-- SPDX-License-Identifier: Apache-2.0 -->\nfictional-host\n' > doc.md; git add -A; expect pass "private words are skipped when unset"
export HNG_POLICY_WORDS_FILE="$work/words.txt"; expect fail "local hostname is rejected"
fresh; export HNG_POLICY_WORDS_FILE="$work/words.txt"; printf '<!-- SPDX-License-Identifier: Apache-2.0 -->\nFICTIONAL-WORKER\n' > doc.md; git add -A; expect fail "local role is case-insensitive"
fresh; export HNG_POLICY_WORDS_FILE="$work/words.txt"; printf '<!-- SPDX-License-Identifier: Apache-2.0 -->\nfictional-hosted\n' > doc.md; git add -A; expect pass "local word inside a longer word"
fresh; export HNG_POLICY_WORDS_FILE="$work/missing.txt"; git add -A; expect fail "configured missing list is an error"
fresh; export HNG_POLICY_WORDS_FILE="$work"; git add -A; expect fail "configured directory is an error"
fresh; : > "$work/empty.txt"; export HNG_POLICY_WORDS_FILE="$work/empty.txt"; git add -A; expect pass "empty local list adds no words"
bad_line "path /ho""me/alice/x" "home directory path"
bad_line "path /Us""ers/alice/x" "desktop home directory path"
bad_line "/m""nt/shared/outbox" "shared mount path"
bad_line "prefix ~""/opt/dpdk" "tilde personal path"
bad_line "prefix \$HO""ME/opt/dpdk" "HOME personal path"
bad_line "addr 192.""168.1.20" "private IPv4 address"
bad_line "addr 10.""0.0.5" "10/8 IPv4 address"
fresh; printf '<!-- SPDX-License-Identifier: Apache-2.0 -->\nversion 110.0.0.5 and 2.10.4\n' > doc.md; git add -A; expect pass "non-address numbers"
fresh; export HNG_POLICY_WORDS_FILE="$work/words.txt"; printf '<!-- SPDX-License-Identifier: Apache-2.0 -->\nthe fictional-host station\n' > doc.md
printf 'doc.md:the fictional-host station\n' >> scripts/env-allow.txt; git add -A; expect pass "allowlisted line"
printf 'another fictional-host\n' >> doc.md; git add -A; expect fail "allowlist entry covers only its own text"

fresh; git add -A; metadata_commit "$owner" "$owner" "owner change"; expect pass "owner metadata"
fresh; git add -A; metadata_commit upstream@example.invalid upstream@example.invalid "upstream change"
git update-ref refs/remotes/origin/main HEAD; expect pass "upstream metadata is outside the checked range"
fresh; git add -A; metadata_commit other@example.invalid "$owner" "owner change"; expect fail "wrong author email"
fresh; git add -A; metadata_commit "$owner" other@example.invalid "owner change"; expect fail "wrong committer email"
fresh; git add -A; metadata_commit "$owner" noreply@github.com "web merge"; expect pass "GitHub committer exception"
fresh; git add -A; metadata_commit "$owner" "$owner" "owner change\n\ncO-aUtHoReD-bY: Someone <human@example.invalid>"; expect fail "all co-author trailers are rejected"
fresh; git add -A; metadata_commit "$owner" "$owner" "owner change\n\nTool-Session: reference"; expect fail "generic session signature"
fresh; git add -A; metadata_commit "$owner" "$owner" "owner change\n\nGenerated with [Tool]"; expect fail "generated signature"
fresh; git add -A; metadata_commit "$owner" "$owner" "owner change\n\n🤖 Generated with [Tool]"; expect fail "prefixed generated signature"
fresh; git add -A; metadata_commit "$owner" "$owner" "data was generated with a fixed seed"; expect pass "ordinary generated prose"
fresh; git add -A; git update-ref -d refs/remotes/origin/main; expect fail "missing upstream baseline"
fresh; git add -A; git rev-parse HEAD > .git/shallow; expect fail "shallow history"
exit "$fails"
