#!/bin/sh
# SPDX-License-Identifier: Apache-2.0
# Open-source policy checks for the repository in the current directory.
#
# OS1: every tracked file carries "SPDX-License-Identifier:" in its first five
#      lines, except comment-less data formats (*.json, *.csv, *.lock, LICENSE)
#      and the upstream files listed in scripts/spdx-exempt.txt (files inherited
#      from the upstream initial commit, which never had a header).
# OS2: no tracked file contains environment-specific values: lab hostnames,
#      internal session/role names, personal or shared-mount paths, private or
#      locally configured IPv4 addresses. A hit is allowed only by an exact
#      "path:fixed text" line in scripts/env-allow.txt.
#      Only this script and the allowlist itself are excluded from the scan.
set -eu
cd "$(git rev-parse --show-toplevel)"
status=0
for f in scripts/spdx-exempt.txt scripts/env-allow.txt; do [ -f "$f" ] || { echo "missing $f" >&2; exit 1; }; done

# shortcut: paths with whitespace are not supported; none exist, switch to -z if needed.
files=$(git ls-files -s | awk '$1 != "160000" {print $4}')
for f in $files; do
    case "$f" in *.json | *.csv | *.lock | LICENSE) continue ;; esac
    grep -qxF "$f" scripts/spdx-exempt.txt && continue
    if ! head -n 5 "$f" | grep -q 'SPDX-License-Identifier:'; then
        echo "OS1 missing SPDX-License-Identifier header: $f" >&2
        status=1
    fi
done

local_words=${HNG_POLICY_WORDS_FILE:-}
if [ -n "$local_words" ] && { [ ! -f "$local_words" ] || [ ! -r "$local_words" ]; }; then
    echo "Cannot read HNG_POLICY_WORDS_FILE: $local_words" >&2
    exit 1
fi
other='/(home|Users|mnt)/[^[:space:]/]|(~|\$HOME|\$\{HOME\})/'
other="$other|(^|[^0-9.])(10\.[0-9]{1,3}|172\.(1[6-9]|2[0-9]|3[01])|192\.168)\.[0-9]{1,3}\.[0-9]{1,3}([^0-9]|$)"
hits=$({
    if [ -n "$local_words" ]; then git grep -n -I -i -w -F -f "$local_words" -- . ':!scripts/check-policy.sh' ':!scripts/env-allow.txt' || true; fi
    git grep -n -I -E "$other" -- . ':!scripts/check-policy.sh' ':!scripts/env-allow.txt' || true
} | sort -u)
if [ -n "$hits" ]; then
    printf '%s\n' "$hits" | awk -v allow=scripts/env-allow.txt '
        BEGIN { while ((getline l < allow) > 0) if (l !~ /^#/ && l != "") a[++n] = l }
        {
            i = index($0, ":"); path = substr($0, 1, i - 1); rest = substr($0, i + 1)
            text = substr(rest, index(rest, ":") + 1); ok = 0
            for (k = 1; k <= n; k++) {
                j = index(a[k], ":")
                if (substr(a[k], 1, j - 1) == path && index(text, substr(a[k], j + 1))) ok = 1
            }
            if (!ok) { print "OS2 environment-specific value: " $0 > "/dev/stderr"; bad = 1 }
        }
        END { exit bad }' || status=1
fi
exit "$status"
