# SPDX-License-Identifier: Apache-2.0
# Sourced by check scripts. Resolves cargo from $CARGO, then PATH, then the
# rustup default $HOME/.cargo/bin; exports CARGO and puts its directory first
# on PATH (Meson and rustup proxies call cargo/rustc by name). Fails loudly.
if [ -n "${CARGO:-}" ]; then
    :
elif command -v cargo >/dev/null 2>&1; then
    CARGO=$(command -v cargo)
elif [ -x "$HOME/.cargo/bin/cargo" ]; then
    CARGO="$HOME/.cargo/bin/cargo"
else
    echo "cargo not found: set CARGO, put cargo on PATH, or install Rust with rustup." >&2
    exit 1
fi
if ! "$CARGO" --version >/dev/null 2>&1; then
    echo "CARGO=$CARGO is not a working cargo executable." >&2
    exit 1
fi
PATH="$(dirname "$CARGO"):$PATH"
export CARGO PATH
