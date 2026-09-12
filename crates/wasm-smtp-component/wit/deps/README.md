# Vendored WIT dependencies

`wasi:io`, `wasi:sockets`, and `wasi:clocks` at **WASI 0.2.4**, copied
unmodified from the WebAssembly WASI 0.2.4 release (as redistributed in
the `wasip2 1.0.1+wasi-0.2.4` crate's `wit/deps/`).

The `wasi` crate that `wasm-smtp-wasi` and this crate's `with:` mapping
resolve against may implement a **later 0.2.x** than these packages
declare: `wasi 0.14` depends on `wasip2` through a caret range, so which
WASI minor a build gets is decided by the consumer's lockfile and is
outside this project's control. That mismatch, and the fix for it — the
annotations, a gate check so drift cannot be silent again, and executing
the component under a host — is tracked in RFC 028.

`wit/smtp.wit` imports these packages; `wit-bindgen` resolves them from
here. `wasi:sockets` depends on `wasi:clocks`, which is why the latter is
present even though the world does not import it directly.

Upstream: <https://github.com/WebAssembly/WASI>, licensed
Apache-2.0 WITH LLVM-exception.

Do not edit these files. To move to another WASI version, replace the
directories wholesale and update the `@version` annotations in
`wit/smtp.wit` and the `with:` mapping in
`crates/wasm-smtp-component/src/lib.rs`.
