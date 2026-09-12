#!/bin/sh
# Fail if the component's declared WASI minor disagrees with the WASI
# version the build actually links.
#
# ## The defect this exists for
#
# RFC 024 D8 vendored `wit/deps/` at WASI 0.2.4 and annotated the world
# `@0.2.4`, on the reasoning that 0.2.4 "is the version implemented by
# the `wasi` 0.14 crate". That read the wrong crate's version metadata.
# `wasi 0.14.7+wasi-0.2.4` is a facade: it re-exports `wasip2`, and it
# is `wasip2`'s build metadata — `1.0.4+wasi-0.2.12` — that says which
# WASI the bindings, and therefore the artifact's imports, belong to.
# The declared contract was two years of releases out of step with the
# artifact and nothing said so, because nothing ever compared them
# (RFC 028).
#
# So the authoritative source here is `wasip2`, never `wasi`.
#
# ## What is compared
#
# The `+wasi-X.Y.Z` metadata on `wasip2` in `Cargo.lock`, against every
# place the component repeats that number:
#
#   - `package wasi:<pkg>@X.Y.Z;` in `crates/wasm-smtp-component/wit/deps/*.wit`
#   - `import wasi:<pkg>/<iface>@X.Y.Z;` in `.../wit/smtp.wit`
#   - the `"wasi:<pkg>/<iface>@X.Y.Z"` keys of the `with:` map in
#     `.../src/lib.rs`
#
# All three have to move together, and a `cargo update` that bumps
# `wasip2` moves none of them. That is the failure this is for: it is
# meant to fire on the update, name both versions, and send whoever ran
# it to re-vendor `wit/deps/` from the new `wasip2`.
#
# ## What this does not claim
#
# Not that the artifact imports only this minor. It does not: the Rust
# standard library contributes `wasi:*@0.2.3` interfaces of its own,
# outside this project's control. The declared minor documents the host
# requirement, and a host satisfies these imports by semver
# compatibility — `tools/component-smoke` proves that on every gate run
# by instantiating the artifact under a host serving a different 0.2.x.
# What is guarded is the weaker, checkable thing: that the number we
# write down is the number our own bindings come from.
#
# Usage: tools/check-wasi-version.sh [root]
# Exits 0 and prints nothing when clean; prints `file:line: found …,
# expected …` per violation and exits 1 otherwise. Exits 2 if it cannot
# establish the expected version, which is a failure, not a pass.
#
# POSIX shell, for the reasons tools/check-doc-versions.sh gives: the
# gate runs it without building anything and it stays readable.

set -eu

root="${1:-$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)}"
lock="$root/Cargo.lock"
crate="$root/crates/wasm-smtp-component"

if [ ! -f "$lock" ]; then
    echo "check-wasi-version: no Cargo.lock at $root" >&2
    exit 2
fi

# The `+wasi-X.Y.Z` build metadata on the `wasip2` package. Deliberately
# not `wasi`: see the header.
expected=$(awk '
    /^\[\[package\]\]/ { name = "" }
    /^name = "wasip2"/ { name = "wasip2" }
    name == "wasip2" && /^version = / {
        if (match($0, /\+wasi-[0-9]+\.[0-9]+\.[0-9]+/)) {
            print substr($0, RSTART + 6, RLENGTH - 6)
            exit
        }
    }
' "$lock")

if [ -z "$expected" ]; then
    echo "check-wasi-version: no wasip2 package with +wasi-X.Y.Z metadata" \
         "in Cargo.lock." >&2
    echo "  The component's WASI bindings reach it through the \`wasi\`" \
         "facade crate. If that" >&2
    echo "  is no longer true, this check needs rewriting against" \
         "whatever replaced it — do" >&2
    echo "  not fall back to \`wasi\`'s own version, which is the" \
         "mistake this guards." >&2
    exit 2
fi

status=0

report() {
    echo "$1:$2: found $3, expected $expected"
    status=1
}

# `package wasi:<pkg>@X.Y.Z;` in the vendored deps.
for f in "$crate"/wit/deps/*.wit; do
    [ -e "$f" ] || continue
    while IFS= read -r hit; do
        line=${hit%%:*}
        found=${hit#*:}
        [ "$found" = "$expected" ] || report "${f#"$root"/}" "$line" "$found"
    done <<EOF
$(grep -n '^package wasi:' "$f" \
    | sed -n 's/^\([0-9]*\):package wasi:[a-z-]*@\([0-9.]*\);.*$/\1:\2/p')
EOF
done

# `import wasi:<pkg>/<iface>@X.Y.Z;` in the world.
while IFS= read -r hit; do
    [ -n "$hit" ] || continue
    line=${hit%%:*}
    found=${hit#*:}
    [ "$found" = "$expected" ] || \
        report "crates/wasm-smtp-component/wit/smtp.wit" "$line" "$found"
done <<EOF
$(grep -n 'import wasi:' "$crate/wit/smtp.wit" \
    | sed -n 's/^\([0-9]*\):.*import wasi:[a-z-]*\/[a-z-]*@\([0-9.]*\);.*$/\1:\2/p')
EOF

# The `with:` map keys in the crate source.
while IFS= read -r hit; do
    [ -n "$hit" ] || continue
    line=${hit%%:*}
    found=${hit#*:}
    [ "$found" = "$expected" ] || \
        report "crates/wasm-smtp-component/src/lib.rs" "$line" "$found"
done <<EOF
$(grep -n '"wasi:' "$crate/src/lib.rs" \
    | sed -n 's/^\([0-9]*\):.*"wasi:[a-z-]*\/[a-z-]*@\([0-9.]*\)".*$/\1:\2/p')
EOF

exit "$status"
