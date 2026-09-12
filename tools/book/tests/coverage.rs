//! Every book chapter with Rust code is compiled by this crate.
//!
//! A chapter added to `docs/src/` and not listed in `src/lib.rs` would
//! escape the doctests silently, so this walks the book and fails naming
//! each one. Not feature-gated: it runs in `cargo test --workspace` too.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

const INCLUDE_PREFIX: &str = "include_str!(\"../../../docs/src/";

#[test]
fn every_chapter_with_rust_code_is_compiled() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let docs = manifest.join("../../docs/src");
    let lib = fs::read_to_string(manifest.join("src/lib.rs")).expect("read src/lib.rs");

    let included: BTreeSet<String> = lib
        .split(INCLUDE_PREFIX)
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .map(str::to_owned)
        .collect();

    let mut with_rust = BTreeSet::new();
    collect(&docs, &docs, &mut with_rust);
    assert!(
        !with_rust.is_empty(),
        "found no chapters with Rust code under {}; is the path right?",
        docs.display()
    );

    let missing: Vec<_> = with_rust.difference(&included).collect();
    assert!(
        missing.is_empty(),
        "chapters with Rust code blocks that tools/book does not compile: {missing:?}\n\
         add a `#[doc = include_str!(..)]` item for each to tools/book/src/lib.rs"
    );

    let stale: Vec<_> = included
        .iter()
        .filter(|rel| !docs.join(rel).is_file())
        .collect();
    assert!(
        stale.is_empty(),
        "tools/book/src/lib.rs includes chapters that do not exist: {stale:?}"
    );
}

/// Every `.md` file under `dir` holding a fence whose info string starts
/// with `rust`, as a path relative to `root` with `/` separators.
fn collect(root: &Path, dir: &Path, out: &mut BTreeSet<String>) {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .expect("read docs directory")
        .map(|e| e.expect("read directory entry").path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect(root, &path, out);
        } else if path.extension().is_some_and(|e| e == "md") {
            let text = fs::read_to_string(&path).expect("read chapter");
            if text.lines().any(|l| l.trim_start().starts_with("```rust")) {
                let rel = path.strip_prefix(root).expect("chapter under root");
                let rel = rel
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/");
                out.insert(rel);
            }
        }
    }
}
