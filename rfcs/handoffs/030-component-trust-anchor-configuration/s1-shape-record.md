# RFC 030 S1 — record of the shape proof

Handoff S1 asks for this to be committed: a record of what was run, with nothing in `crates/` changed. The scratch sources, generated bindings, both stub components, and the run logs are kept with the review request's evidence (`.git-exclude/review-request/evidence/030/s1/`).

Run on 2026-09-13, on `32ba755`.

## The shape under test

A minimal `wasm-smtp:smtp@0.2.0`. Its records and `send-error` are copied from `0.1.0`; the resource is RFC 030 D1 and the variant D2. The world exports `smtp-send` and nothing else.

```wit
variant trust-anchors { bundled, custom(string) }

resource smtp-config {
    create: static func(host: string, port: u16, ehlo-domain: string,
                        tls-mode: tls-mode, trust: trust-anchors)
        -> result<smtp-config, send-error>;
}

send: func(config: borrow<smtp-config>, credentials: smtp-credentials,
           message: smtp-message) -> result<send-result, send-error>;
```

The `0.2.1` copy is identical except for its version and one added method, `host: func() -> string;`.

## S1.2 — each documented generator

The commands are the documentation's (`docs/src/adapters/component-model.md`), pointed at the stub WIT. Each tool was installed into a scratch directory: `npm install @bytecodealliance/jco`, `cargo install --locked wit-bindgen-cli`, and `pip install componentize-py` in a venv.

| Generator | Version | `create` | `send` |
|---|---|---|---|
| `jco types` | 1.33.0 | `class SmtpConfig implements Disposable { static create(host: string, port: number, ehloDomain: string, tlsMode: TlsMode, trust: TrustAnchors): SmtpConfig; [Symbol.dispose](): void; }` | `export function send(config: SmtpConfig, credentials: SmtpCredentials, message: SmtpMessage): SendResult;` |
| `wit-bindgen go` | wit-bindgen-cli 0.62.0 | `SmtpConfigCreate(host string, port uint16, ehloDomain string, tlsMode uint8, trust TrustAnchors)`, returning a `witTypes` result of `*SmtpConfig` or `SendError`; `(*SmtpConfig).Drop()` | `Send(config *SmtpConfig, credentials SmtpCredentials, message SmtpMessage)`, returning a `witTypes` result of `SendResult` or `SendError`; the borrow arrives through `SmtpConfigFromBorrowHandle` |
| `componentize-py … bindings` | 0.25.1 | `class SmtpConfig(Protocol)` with `@classmethod def create(cls, host: str, port: int, ehlo_domain: str, tls_mode: TlsMode, trust: TrustAnchors) -> Self` | `def send(self, config: smtp_send.SmtpConfig, credentials: smtp_send.SmtpCredentials, message: smtp_send.SmtpMessage) -> smtp_send.SendResult`, raising `Err(SendError)` |

`trust-anchors` comes out as `TrustAnchorsBundled | TrustAnchorsCustom` (TypeScript), `MakeTrustAnchorsBundled()` / `MakeTrustAnchorsCustom(string)` (Go), and `TrustAnchors_Bundled | TrustAnchors_Custom` dataclasses (Python). **All three express the shape.**

**A finding, outside this RFC.** `wit-bindgen go` and `componentize-py … bindings` generate **implementation-side** bindings. The Go output is `wasm_export_…` functions that call `SmtpConfigCreate` and `Send`, which the Go author must write. The Python output is a `Protocol` to implement. Neither produces code for *calling* this component from Go or Python, which is how the documentation's "Language bindings" section presents them. Only `jco types` describes a caller's view. This predates RFC 030 and is not a reason to stop S1, since each generator expresses the shape. It is recorded so the documentation's claim can be decided on separately.

## S1.3 — create, send through a borrow, drop

The stub guest is built with `wit-bindgen` 0.62 for `wasm32-wasip2`; its resource logs from its destructor. The host is generated from the `0.2.0` WIT with `wasmtime::component::bindgen!`, wasmtime and wasmtime-wasi `=36.0.14`, the harness's pins.

```
[host] instantiated …/stub-0.2.0.wasm
[guest] smtp-config created (host=smtp.example.com, custom=true)
[host] create -> Ok(resource)
[guest] send via borrowed config (port=2525, custom=true)
[host] send(borrow) -> Ok(SendResult { reply-code: 2525 })
[guest] send via borrowed config (port=2525, custom=true)
[host] send(borrow) again, same resource -> Ok(SendResult { reply-code: 2525 })
[guest] smtp-config dropped (host=smtp.example.com)
[host] resource_drop -> Ok
[host] create(empty host) -> Err(InvalidInput("host is empty"))
[host] OK
```

The borrow reaches the guest's own value (it reads `port` back), survives a second `send`, and `resource_drop` runs the guest destructor exactly once. A rejected `create` returns the error variant rather than a resource.

## S1.4 — compatibility: a 0.2.1 component under the 0.2.0 host

The two artifacts really do differ. Strings in each show `stub-0.2.0.wasm` exporting `wasm-smtp:smtp/smtp-send@0.2.0` with `[static]smtp-config.create` only, and `stub-0.2.1.wasm` exporting `wasm-smtp:smtp/smtp-send@0.2.1` with `[static]smtp-config.create` and `[method]smtp-config.host`.

The same `0.2.0` host binary, run against `stub-0.2.1.wasm`:

```
[host] instantiated …/stub-0.2.1.wasm
[guest] smtp-config created (host=smtp.example.com, custom=true)
[host] create -> Ok(resource)
[guest] send via borrowed config (port=2525, custom=true)
[host] send(borrow) -> Ok(SendResult { reply-code: 2525 })
[guest] send via borrowed config (port=2525, custom=true)
[host] send(borrow) again, same resource -> Ok(SendResult { reply-code: 2525 })
[guest] smtp-config dropped (host=smtp.example.com)
[host] resource_drop -> Ok
[host] create(empty host) -> Err(InvalidInput("host is empty"))
[host] OK
```

**It instantiates and `send` works.** A method added in a compatible minor version does not break a host built against `0.2.0`, which is what RFC 030 D1's "the break is paid once" depends on.

## S1.5 — result

Neither stop condition is met: every documented generator expresses the shape, and the compatibility run passed. S2 proceeds.
