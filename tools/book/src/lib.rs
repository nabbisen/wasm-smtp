//! Compiles the Rust code blocks in the book under `docs/src/`.
//!
//! Each item below includes one chapter as documentation, so rustdoc runs
//! every `rust` fence in it as a doctest: a renamed function, a changed
//! signature, or an example that no longer compiles fails the gate instead
//! of reaching a reader (RFC 032 D2).
//!
//! The items exist only under `doctest` and the non-default `book`
//! feature. Run them with
//!
//! ```text
//! cargo test --locked -p wasm-smtp-book --features book
//! ```
//!
//! Fences marked `ignore` are skipped, and each chapter says why beside
//! the fence: code that only exists on `wasm32-wasip2`, or a listing of
//! signatures with no bodies. `tests/coverage.rs` fails if a chapter with
//! Rust code is missing from this list.

#[cfg(all(doctest, feature = "book"))]
#[doc = include_str!("../../../docs/src/adapters/cloudflare.md")]
pub struct AdaptersCloudflare;

#[cfg(all(doctest, feature = "book"))]
#[doc = include_str!("../../../docs/src/adapters/tokio.md")]
pub struct AdaptersTokio;

#[cfg(all(doctest, feature = "book"))]
#[doc = include_str!("../../../docs/src/adapters/wasi.md")]
pub struct AdaptersWasi;

#[cfg(all(doctest, feature = "book"))]
#[doc = include_str!("../../../docs/src/concepts/errors.md")]
pub struct ConceptsErrors;

#[cfg(all(doctest, feature = "book"))]
#[doc = include_str!("../../../docs/src/core/composing-messages.md")]
pub struct CoreComposingMessages;

#[cfg(all(doctest, feature = "book"))]
#[doc = include_str!("../../../docs/src/core/connection-reuse.md")]
pub struct CoreConnectionReuse;

#[cfg(all(doctest, feature = "book"))]
#[doc = include_str!("../../../docs/src/core/core.md")]
pub struct CoreCore;

#[cfg(all(doctest, feature = "book"))]
#[doc = include_str!("../../../docs/src/core/policy-audit.md")]
pub struct CorePolicyAudit;

#[cfg(all(doctest, feature = "book"))]
#[doc = include_str!("../../../docs/src/core/streaming.md")]
pub struct CoreStreaming;

#[cfg(all(doctest, feature = "book"))]
#[doc = include_str!("../../../docs/src/core/usage.md")]
pub struct CoreUsage;

#[cfg(all(doctest, feature = "book"))]
#[doc = include_str!("../../../docs/src/reference/examples.md")]
pub struct ReferenceExamples;
