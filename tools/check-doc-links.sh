#!/bin/sh
# Fail if a link in the documentation does not resolve where it is read.
#
# The defect this exists for (RFC 034): documentation is read on GitHub,
# on crates.io, on docs.rs, and in the published book, and its links were
# written for one of those and checked in none. The owner found four
# broken links on crates.io by hand. A link is a constant that has to
# match something outside the file it sits in, which is a check the gate
# should run (RFC 029's conclusion, applied to links).
#
# Two checks, both offline. This reads local files only and never the
# network: a gate that fails because a third-party site is down is not a
# gate.
#
# 1. Published READMEs. For each crate under `crates/` whose manifest is
#    not `publish = false`, take the file its `readme` names (or its
#    `README.md` when `readme` is unset). crates.io rewrites a relative
#    link against the *crate's* directory, not the file's, so every
#    relative link — inline `](…)` and reference definitions `[…]: …` —
#    is resolved from the crate directory and must exist there. Skipped:
#    `http:`, `https:`, `mailto:`, fragment-only links, and anything
#    inside a fenced code block.
#
# 2. docs.rs links. In `README.md`, `crates/*/README.md`,
#    `docs/src/**/*.md`, `crates/*/src/**/*.rs`, and `CHANGELOG.md`, every
#    `https://docs.rs/<crate>/<version>/<ident>/<path>.html[#<anchor>]`
#    whose `<ident>` is a crate in this workspace must name a page rustdoc
#    generates: `target/doc/<ident>/<path>.html`, or for `wasm_smtp_wasi`,
#    whose API exists only on its target, `target/wasm32-wasip2/doc/…`.
#    With an anchor, the page must contain `id="<anchor>"`. A docs.rs link
#    to a crate outside this workspace is skipped, as is a docs.rs link
#    that names no item page (a crate root or a badge). Fenced code blocks
#    in Markdown are skipped here too.
#
# The documentation trees are produced by the two `cargo doc` gate
# commands that run before this one.
#
# Usage: tools/check-doc-links.sh [root]
# Exits 0 and prints nothing when clean; prints `file:line: <problem>` per
# violation and exits 1 otherwise. Exits 2 when a prerequisite is missing,
# such as an absent `target/doc`: that is a failure, not a pass.
#
# POSIX shell, like the other guards: nothing to build, readable without
# Rust. Fixture tests in `tools/guard-tests/doc-links/`.

set -eu

root="${1:-$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)}"
host_doc="$root/target/doc"
wasi_doc="$root/target/wasm32-wasip2/doc"

if [ ! -d "$host_doc" ]; then
    echo "check-doc-links: no $host_doc; build the documentation first" >&2
    exit 2
fi
if [ ! -d "$root/crates" ]; then
    echo "check-doc-links: no crates/ directory at $root" >&2
    exit 2
fi

report="$(mktemp "${TMPDIR:-/tmp}/check-doc-links.XXXXXX")"
trap 'rm -f -- "$report"' EXIT

# Print "<line>\t<target>" for every link in a Markdown or Rust file.
# $1: file; $2: "relative" for relative links, "docsrs" for docs.rs links.
# Fenced code blocks (``` or ~~~) are skipped in Markdown.
links() {
    awk -v mode="$2" -v md="$(case "$1" in *.md) echo 1 ;; *) echo 0 ;; esac)" '
        md && /^[ \t]*(```|~~~)/ { fence = !fence; next }
        fence { next }
        mode == "docsrs" {
            line = $0
            while (match(line, /https:\/\/docs\.rs\/[^ \t)>"`'"'"']+/)) {
                print NR "\t" substr(line, RSTART, RLENGTH)
                line = substr(line, RSTART + RLENGTH)
            }
            next
        }
        mode == "relative" {
            line = $0
            # Reference definitions: [label]: target
            if (match(line, /^[ \t]*\[[^]]+\]:[ \t]*[^ \t]+/)) {
                def = substr(line, RSTART, RLENGTH)
                sub(/^[ \t]*\[[^]]+\]:[ \t]*/, "", def)
                print NR "\t" def
            }
            # Inline links and images: ](target)
            while (match(line, /\]\([^)]*\)/)) {
                t = substr(line, RSTART + 2, RLENGTH - 3)
                print NR "\t" t
                line = substr(line, RSTART + RLENGTH)
            }
        }
    ' "$1"
}

# ── 1. Published READMEs ──────────────────────────────────────────────────

for manifest in "$root"/crates/*/Cargo.toml; do
    [ -f "$manifest" ] || continue
    crate_dir=$(dirname -- "$manifest")
    # `publish = false` in [package] means crates.io never renders it.
    publish_false=$(awk '
        /^\[/ { section = $0 }
        section == "[package]" && /^publish[ \t]*=[ \t]*false/ { print "yes"; exit }
    ' "$manifest")
    [ -z "$publish_false" ] || continue

    readme=$(awk '
        /^\[/ { section = $0 }
        section == "[package]" && /^readme[ \t]*=/ {
            if (match($0, /"[^"]*"/)) { print substr($0, RSTART + 1, RLENGTH - 2); exit }
        }
    ' "$manifest")
    [ -n "$readme" ] || readme="README.md"
    readme_path="$crate_dir/$readme"
    [ -f "$readme_path" ] || continue
    # Report the README by its path from the root, whichever crate it is.
    readme_rel=$(CDPATH= cd -- "$(dirname -- "$readme_path")" && pwd)/$(basename -- "$readme_path")
    readme_rel=${readme_rel#"$root/"}

    links "$readme_path" relative | while IFS="$(printf '\t')" read -r lineno target; do
        # Strip <…>, a title after a space, and a #fragment.
        target=${target#<}; target=${target%>}
        target=${target%% *}
        case "$target" in
            ''|'#'*|http:*|https:*|mailto:*) continue ;;
        esac
        path=${target%%#*}
        [ -n "$path" ] || continue
        if [ ! -e "$crate_dir/$path" ]; then
            printf '%s:%s: relative link "%s" does not resolve from the crate directory, where crates.io resolves it\n' \
                "$readme_rel" "$lineno" "$target" >> "$report"
        fi
    done
done

# ── 2. docs.rs links ──────────────────────────────────────────────────────

# Library identifiers of this workspace's crates.
idents=$(for m in "$root"/crates/*/Cargo.toml; do
    awk '/^\[/ { s = $0 } s == "[package]" && /^name[ \t]*=/ {
        if (match($0, /"[^"]*"/)) { n = substr($0, RSTART + 1, RLENGTH - 2); gsub(/-/, "_", n); print n; exit }
    }' "$m"
done)

files=$(
    {
        [ -f "$root/README.md" ] && echo "$root/README.md"
        [ -f "$root/CHANGELOG.md" ] && echo "$root/CHANGELOG.md"
        find "$root/crates" -mindepth 2 -maxdepth 2 -name README.md -type f 2>/dev/null
        find "$root/docs/src" -name '*.md' -type f 2>/dev/null
        find "$root/crates" -path '*/src/*' -name '*.rs' -type f 2>/dev/null
    } | sort -u
)

for file in $files; do
    rel=${file#"$root/"}
    links "$file" docsrs | while IFS="$(printf '\t')" read -r lineno url; do
        # https://docs.rs/<crate>/<version>/<ident>/<path>.html[#anchor]
        rest=${url#https://docs.rs/}
        crate=${rest%%/*}; rest=${rest#"$crate"}; rest=${rest#/}
        version=${rest%%/*}; rest=${rest#"$version"}; rest=${rest#/}
        ident=${rest%%/*}; rest=${rest#"$ident"}; rest=${rest#/}
        page=${rest%%#*}
        case "$rest" in *'#'*) anchor=${rest#*#} ;; *) anchor= ;; esac
        # Only item pages of this workspace's crates are checked.
        case "$page" in *.html) ;; *) continue ;; esac
        [ -n "$ident" ] || continue
        printf '%s\n' "$idents" | grep -qx -- "$ident" || continue

        if [ "$ident" = wasm_smtp_wasi ]; then
            if [ ! -d "$wasi_doc" ]; then
                echo "check-doc-links: no $wasi_doc, needed for $rel:$lineno; build the wasm32-wasip2 documentation first" >&2
                echo 2 > "$report.exit"
                continue
            fi
            doc="$wasi_doc"
        else
            doc="$host_doc"
        fi
        target="$doc/$ident/$page"
        if [ ! -f "$target" ]; then
            printf '%s:%s: docs.rs page "%s/%s" is not generated by rustdoc\n' \
                "$rel" "$lineno" "$ident" "$page" >> "$report"
        elif [ -n "$anchor" ] && ! grep -qF "id=\"$anchor\"" "$target"; then
            printf '%s:%s: anchor "#%s" not found in docs.rs page "%s/%s"\n' \
                "$rel" "$lineno" "$anchor" "$ident" "$page" >> "$report"
        fi
    done
done

if [ -f "$report.exit" ]; then
    rm -f -- "$report.exit"
    exit 2
fi
if [ -s "$report" ]; then
    sort -u "$report"
    exit 1
fi
