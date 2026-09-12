# Vendored WIT dependencies

`wasi:io`, `wasi:sockets`, and `wasi:clocks` at **WASI 0.2.4**, copied
unmodified from the WebAssembly WASI 0.2.4 release (as redistributed in
the `wasip2 1.0.1+wasi-0.2.4` crate's `wit/deps/`). That is the version
the `wasi 0.14` crate implements, which `wasm-smtp-wasi` already uses.

`wit/smtp.wit` imports these packages; `wit-bindgen` resolves them from
here. `wasi:sockets` depends on `wasi:clocks`, which is why the latter is
present even though the world does not import it directly.

Upstream: <https://github.com/WebAssembly/WASI>, licensed
Apache-2.0 WITH LLVM-exception.

Do not edit these files. To move to another WASI version, replace the
directories wholesale and update the `@version` annotations in
`wit/smtp.wit` and the `with:` mapping in
`crates/wasm-smtp-component/src/lib.rs`.
