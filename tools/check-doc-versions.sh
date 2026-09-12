#!/bin/sh
# Fail if a dependency version string in the documentation disagrees with
# the workspace manifest.
#
# The defect this exists for: every "add this to your Cargo.toml" snippet
# carried a hard-coded version, and they went stale at each release. Twice
# the owner found one by hand. A constant that has to match a release is
# something a machine should check (RFC 029 D2, D4).
#
# Two things are compared:
#
#   - any `wasm-smtp*` version, against `[workspace.package] version`;
#   - any `mail-builder` version, against `[workspace.dependencies]`,
#     because the composing chapter's advice depends on what we build.
#
# Only `major.minor` is compared, so a patch release does not invalidate
# the documentation.
#
# What is scanned: `README.md`, `docs/src/**/*.md`, `crates/*/README.md`,
# and `tools/*/README.md` — everything a reader copies a dependency line
# out of. Two things are left out on purpose, and neither is an oversight:
#
#   - `CHANGELOG.md`, whose version strings are history. "moves to 0.5"
#     in the 0.17.0 entry is true forever and must not be "corrected".
#   - `rfcs/`, for the same reason: an RFC records what was decided at the
#     version it was decided.
#
# The tokio chapter keeps one TOML block of dependency lines, comparing
# four feature configurations, where every other chapter moved to
# `cargo add`. That block is intentional live coverage: it reads better
# as one block, and it is the input that keeps this check's own-crate
# branch exercised against the real book between releases.
#
# The fixture tests in `tools/guard-tests/` exercise every branch.
#
# Usage: tools/check-doc-versions.sh [root]
# Exits 0 and prints nothing when clean; prints `file:line: found …,
# expected …` per violation and exits 1 otherwise.
#
# POSIX shell rather than a Rust binary on purpose: the gate should be
# able to run this without building anything, and it stays readable to
# someone who is not a Rust programmer.

set -eu

root="${1:-$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)}"
manifest="$root/Cargo.toml"

if [ ! -f "$manifest" ]; then
    echo "check-doc-versions: no Cargo.toml at $root" >&2
    exit 2
fi

# `[workspace.package] version`, reduced to major.minor.
expected_own=$(awk '
    /^\[/ { section = $0 }
    section == "[workspace.package]" && /^version[ \t]*=/ {
        if (match($0, /"[0-9]+\.[0-9]+/)) {
            print substr($0, RSTART + 1, RLENGTH - 1)
            exit
        }
    }
' "$manifest")

# `mail-builder` in `[workspace.dependencies]`, reduced to major.minor.
expected_mail_builder=$(awk '
    /^\[/ { section = $0 }
    section == "[workspace.dependencies]" && /^mail-builder[ \t]*=/ {
        if (match($0, /version[ \t]*=[ \t]*"[0-9]+\.[0-9]+/)) {
            v = substr($0, RSTART, RLENGTH)
            if (match(v, /[0-9]+\.[0-9]+/)) {
                print substr(v, RSTART, RLENGTH)
            }
            exit
        }
    }
' "$manifest")

if [ -z "$expected_own" ]; then
    echo "check-doc-versions: could not read [workspace.package] version" >&2
    exit 2
fi
if [ -z "$expected_mail_builder" ]; then
    echo "check-doc-versions: could not read mail-builder from [workspace.dependencies]" >&2
    exit 2
fi

# README, the book, and each crate's own README: everything a reader
# copies a dependency line out of.
files=$(
    {
        [ -f "$root/README.md" ] && echo "$root/README.md"
        find "$root/docs/src" -name '*.md' -type f 2>/dev/null
        find "$root/crates" -maxdepth 2 -name 'README.md' -type f 2>/dev/null
        find "$root/tools" -maxdepth 2 -name 'README.md' -type f 2>/dev/null
    } | sort -u
)

violations=0
for file in $files; do
    # Report repo-relative paths: the absolute prefix is noise, and a
    # relative path is what an editor or a CI annotation wants.
    rel=${file#"$root/"}
    output=$(
        awk -v own="$expected_own" -v mb="$expected_mail_builder" -v path="$rel" '
            {
                line = $0
                # A dependency line names the crate, then an "=", then a
                # version either directly or inside an inline table. The
                # leading "#" of a commented-out alternative is fine: a
                # reader uncomments it.
                if (match(line, /(wasm-smtp[a-zA-Z0-9_-]*|mail-builder)[ \t]*=/)) {
                    name = substr(line, RSTART, RLENGTH)
                    sub(/[ \t]*=$/, "", name)
                    rest = substr(line, RSTART + RLENGTH)

                    if (match(rest, /"[0-9]+\.[0-9]+(\.[0-9]+)?"/)) {
                        found_full = substr(rest, RSTART + 1, RLENGTH - 2)
                        # Compare on major.minor only.
                        if (match(found_full, /^[0-9]+\.[0-9]+/)) {
                            found = substr(found_full, RSTART, RLENGTH)
                        } else {
                            found = found_full
                        }

                        expected = (name == "mail-builder") ? mb : own
                        if (found != expected) {
                            printf "%s:%d: found \"%s\", expected \"%s\"\n", \
                                path, NR, found_full, expected
                        }
                    }
                }
            }
        ' "$file"
    )
    if [ -n "$output" ]; then
        echo "$output"
        violations=$((violations + 1))
    fi
done

[ "$violations" -eq 0 ] || exit 1
