#!/bin/sh
# Fixture tests for the project's shell guards (RFC 032 D5).
#
# Each guard was shown failing once, by hand, in a review request, and
# those demonstrations ran nowhere afterwards. These run them in the gate.
#
# Layout: tools/guard-tests/<guard>/<case>/ holds
#
#   tree/     a minimal repository the guard is pointed at as its root
#   status    the expected exit code
#   stdout    the expected standard output, byte for byte
#   stderr    optional; when present, compared too
#
# where <guard> is `doc-versions` or `wasi-version`, naming
# tools/check-<guard>.sh.
#
# A shell runner rather than a Rust test because of what a failure looks
# like: `diff -u` of expected against actual output is what someone
# debugging a guard wants to read, and assert_eq! on two multi-line
# strings prints both blobs whole and leaves the diff to the reader.
#
# Usage: tools/guard-tests/run.sh
# Exits 0 when every case passes, 1 otherwise. Prints one line per case,
# and a diff under each failure.

set -u

# The guards list the files they scan through `sort`, whose order follows
# the collation locale: under a case-insensitive one `README.md` sorts
# after `docs/`, under C it sorts before. Exit status is unaffected, but
# expected stdout is byte-exact, so the runner fixes the locale rather
# than keeping one fixture per locale. CI's runner defaults to C.UTF-8.
LC_ALL=C
export LC_ALL

here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
root=$(CDPATH= cd -- "$here/../.." && pwd)
scratch=$(mktemp -d "${TMPDIR:-/tmp}/guard-tests.XXXXXX")
trap 'rm -r -- "$scratch"' EXIT

failures=0
cases=0

for case_dir in "$here"/*/*/; do
    case_dir=${case_dir%/}
    [ -d "$case_dir/tree" ] || continue
    case_name=${case_dir#"$here"/}
    guard_name=${case_name%%/*}
    guard="$root/tools/check-$guard_name.sh"
    cases=$((cases + 1))

    out="$scratch/stdout"
    err="$scratch/stderr"
    "$guard" "$case_dir/tree" >"$out" 2>"$err"
    status=$?

    expected_status=$(cat "$case_dir/status")
    problems=""

    if [ "$status" != "$expected_status" ]; then
        problems="${problems}  exit status: expected $expected_status, got $status
"
    fi
    if ! diff -u --label "expected stdout" --label "actual stdout" \
            "$case_dir/stdout" "$out" >"$scratch/diff-out"; then
        problems="${problems}$(cat "$scratch/diff-out")
"
    fi
    if [ -f "$case_dir/stderr" ] && \
       ! diff -u --label "expected stderr" --label "actual stderr" \
            "$case_dir/stderr" "$err" >"$scratch/diff-err"; then
        problems="${problems}$(cat "$scratch/diff-err")
"
    fi

    if [ -z "$problems" ]; then
        echo "PASS  $case_name"
    else
        echo "FAIL  $case_name"
        printf '%s' "$problems"
        failures=$((failures + 1))
    fi
done

if [ "$cases" -eq 0 ]; then
    echo "guard-tests: no cases found under $here" >&2
    exit 1
fi
if [ "$failures" -ne 0 ]; then
    echo "guard-tests: $failures of $cases case(s) failed" >&2
    exit 1
fi
