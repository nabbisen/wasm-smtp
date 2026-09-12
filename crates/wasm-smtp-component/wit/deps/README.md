# Vendored WIT dependencies

`wasi:io`, `wasi:sockets`, and `wasi:clocks` at **WASI 0.2.12**, copied
unmodified from the WebAssembly WASI 0.2.12 release (as redistributed in
the `wasip2 1.0.4+wasi-0.2.12` crate's `wit/deps/`). `wasi:sockets`
depends on `wasi:io` and `wasi:clocks`, which is why the third is here
even though the world does not import it directly.

## Why 0.2.12, and what it does and does not mean

These files, and the `@0.2.12` annotations in `wit/smtp.wit`, are
**documentation of the host requirement**. They do not decide what the
built component imports.

The `with:` mapping in `../../src/lib.rs` points every WASI interface at
the `wasi` crate's own bindings, so this WIT generates no import bindings
at all — only the `smtp-send` export. The artifact's import section is
assembled by the linker from every Rust crate that declares one, and it
contains **two** WASI minors: 0.2.12 from the `wasi` crate this adapter
uses, and 0.2.3 from the Rust standard library's own WASI support. That
was verified in the artifact, and re-vendoring these files from 0.2.4 to
0.2.12 changed the artifact's imports not at all (RFC 028 S2).

So the world names the minor that the majority of the imports carry, and
is accurate about the requirement — a WASI 0.2 host — without claiming to
enumerate the artifact exactly, which it cannot: the `std` contribution
is outside this project's control, as is which `wasip2` a consumer's
lockfile resolves through `wasi`'s caret range.

Stating a single minor is not ideal, and the alternative was considered:
WIT does not permit an unversioned `import wasi:sockets/tcp;` when the
vendored package declares a version — the resolver looks for a package
literally named `wasi:sockets` and does not find `wasi:sockets@0.2.12`.
Tested, not assumed (RFC 028 S2).

What makes this safe in practice is that a host satisfies these imports
by semver compatibility, not by exact match: the component instantiates
under a host providing a different 0.2.x, which `tools/component-smoke`
proves on every gate run by doing exactly that.

## Maintenance

Do not edit these files. To move to another WASI version, replace them
wholesale from the matching `wasip2` crate and update, together:

- the `@version` annotations in `wit/smtp.wit`,
- the `with:` mapping keys in `../../src/lib.rs`,
- this file.

Upstream: <https://github.com/WebAssembly/WASI>, licensed
Apache-2.0 WITH LLVM-exception.
