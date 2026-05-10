# RFC 018 — Component Model and WIT interface

**Status.** Proposed
**Priority.** P2
**Tracks.** Component Model / WIT
**Touches.** `wit/` (new), `docs/src/component-model.md`

## Summary

Design a language-neutral WASM Component Model interface for `wasm-smtp`
so that the SMTP send operation can be used from any language with WIT
bindings (Go, Python, JavaScript/TypeScript, Rust, C/C++, …) without
re-implementing the protocol.

## Motivation

WASI Component Model (part of WASI 0.2+) lets WASM modules expose
typed interfaces via WIT (WebAssembly Interface Types). A `wasm-smtp`
component would expose a single `send` operation; consumers in any
language generate bindings from the WIT and call it directly.

This is the "WASM component as a standard part" vision from the
extension plan: `wasm-smtp` as a portable, language-agnostic SMTP
send component, not just a Rust library.

## Goals

- Define a WIT interface for SMTP send operations.
- Cover the common case: send a plain-text or HTML message to one or
  more recipients with authentication.
- Design the credential handling so that secrets are not serialised
  into the component's persistent state.
- Ensure the WIT interface is compatible with the existing Rust API.
- Provide a draft WIT file that can be iterated on.

## Non-goals

- Generating language bindings for all targets (that is the consumer's
  job via `wit-bindgen`).
- A production-ready component release (this is a design RFC).
- Streaming DATA in the WIT interface (deferred to RFC 019).
- MIME composition (out of scope for this project).

## Design

### WIT world

```wit
package wasm-smtp:smtp@0.1.0;

/// The SMTP submission world. A component implementing this world can
/// be composed into any host that supplies the wasi:sockets and (when
/// wasi-tls is available) wasi:tls imports.
world smtp-client {
    import wasi:sockets/network@0.2.0;
    import wasi:sockets/tcp@0.2.0;
    // import wasi:tls/client@0.1.0;  // future

    export smtp-send;
}
```

### WIT interface: `smtp-send`

```wit
interface smtp-send {
    /// SMTP server connection parameters.
    record smtp-config {
        host: string,
        port: u16,
        /// Domain to use in EHLO. Typically the sender's hostname.
        ehlo-domain: string,
        tls-mode: tls-mode,
    }

    enum tls-mode {
        /// Implicit TLS (port 465). Recommended.
        implicit,
        /// STARTTLS (port 587).
        starttls,
    }

    /// Authentication credentials.
    ///
    /// NOTE: credentials are passed as plain-text function arguments and
    /// are not retained by the component between calls. Callers are
    /// responsible for protecting credentials in their own runtime.
    record smtp-credentials {
        username: string,
        password: string,
    }

    /// A single SMTP message envelope + body.
    record smtp-message {
        from: string,
        to: list<string>,
        /// Fully composed RFC 5322 message body (headers + blank line +
        /// content), CRLF-normalised. Dot-stuffing is handled by the
        /// component.
        raw-message: string,
    }

    /// Outcome of a send operation.
    record send-result {
        /// Server reply code from the final DATA response.
        reply-code: u16,
    }

    /// Error classification.
    variant send-error {
        /// Transport-level failure.
        io(string),
        /// SMTP protocol error (unexpected reply code, malformed reply).
        protocol(string),
        /// Authentication failure.
        auth-rejected,
        /// Input validation failure (empty from, no recipients, etc.).
        invalid-input(string),
    }

    /// Connect, authenticate, send one message, and quit.
    ///
    /// This is the primary entry point. It creates a new TCP connection
    /// for each call; connection reuse is not exposed through the WIT
    /// interface (use the Rust API directly for connection reuse).
    send: func(
        config: smtp-config,
        credentials: smtp-credentials,
        message: smtp-message,
    ) -> result<send-result, send-error>;
}
```

### Credential handling

Credentials are passed as function arguments on each `send` call and
are not persisted in component state. This is the safest design:
there is no credential cache in the component that a compromised host
could read.

The documentation will prominently warn that credentials cross the
host-component boundary as plain WIT strings. Callers who require
higher security should use the Rust API directly (where credentials
remain within a single Rust `async` call frame) rather than the
component interface.

### Connection-per-call model

The WIT `send` function creates a TCP connection, does the SMTP
exchange, and closes the connection. It does not expose connection
reuse (that requires stateful component instances, which is more
complex). For high-throughput use cases, callers should use the Rust
API with its explicit `SmtpClient` lifecycle.

### Relationship to `wasm-smtp-wasi`

The WASM component is built from `wasm-smtp-wasi` + an adapter layer
that maps the WIT function arguments to Rust API calls. The component
itself is a thin shim over the existing Rust implementation.

### Build process

```
wit/smtp.wit          ← WIT interface definition
  │
  └─▶ wit-bindgen → Rust glue (generated, not hand-maintained)
        │
        └─▶ links against wasm-smtp-wasi + wasm-smtp
              │
              └─▶ wasm-smtp-component.wasm  ← distributable
```

`cargo component build` (from the `cargo-component` toolchain) produces
the component WASM.

## Security considerations

- The WIT interface exposes credentials as plain strings. This is
  unavoidable in the current Component Model; WIT has no secret type.
  The documentation must make this clear.
- The component must not log or emit the credentials through any WASI
  logging interface.
- `send-error::auth-rejected` carries no information about which field
  (username or password) was wrong.

## Simplicity and maintainability considerations

The WIT interface is intentionally minimal: one world, one interface,
one function. Extensions (multiple messages per connection, streaming
body, OAUTH2 credentials) can be added as additional functions or
optional parameters in later releases.

## Alternatives considered

**Expose `SmtpClient` as a stateful component resource:** the Component
Model supports `resource` types with constructors and methods. A
`SmtpClient` resource would allow connection reuse. Deferred because
stateful component resources are more complex to implement and compose,
and because the common use case (one message per Worker request) does
not benefit from connection reuse.

**Use WASI-native CLI interface instead of WIT:** simpler but produces
a WASM binary, not a component, which cannot be composed. Rejected.

## Implementation plan

*Not yet implemented.*

1. Create `wit/smtp.wit` with the draft interface.
2. Test WIT validity with `wit-parser` / `wac` tooling.
3. Implement the Rust shim layer that bridges WIT arguments to the
   Rust `SmtpClient` API.
4. Build and test the component with `cargo component`.
5. Publish `docs/src/component-model.md`.

Target: v0.16.0 or later, after RFC 016 and RFC 017 are implemented.

## Acceptance criteria

- `wit/smtp.wit` is valid WIT parseable by current `wit-parser`.
- `wit-bindgen` can generate Rust bindings from the WIT.
- A test host (wasmtime CLI or a test harness) can call the `send`
  function and verify the correct SMTP commands are issued.
- The documentation warns about credential handling.

## Open questions

1. Should the WIT interface use `resource smtp-client` for connection
   reuse, deferred or in scope for the first release?
2. Is `cargo-component` the right build tool, or should we use
   `wasm-tools component` + a manual WIT implementation?
3. Should XOAUTH2 be exposed in the WIT credentials type (as an
   alternative to username/password), given that XOAUTH2 tokens are
   already short-lived and therefore safer to pass as WIT arguments?
