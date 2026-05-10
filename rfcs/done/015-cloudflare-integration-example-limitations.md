# RFC 015 — Cloudflare integration example and runtime limitations

**Status.** Implemented (v0.3.0)
**Priority.** P2
**Tracks.** Docs / Adapter / Integration
**Touches.** `docs/src/cloudflare-adapter.md`, `docs/src/usage.md`

## Summary

Document the minimal Cloudflare Workers integration example and the
complete set of runtime constraints that `wasm-smtp-cloudflare` users
must be aware of.

## Motivation

A working code example removes the most common friction point for new
users. An explicit limitations section prevents runtime surprises (port
25 failures, private network errors, request-scope violations) from
appearing as bugs in the library.

## Goals

- Provide a minimal, copy-pasteable Workers handler that sends one email.
- Document all Cloudflare-specific constraints in one place.
- Explain how to retrieve SMTP credentials from Workers Secrets.
- Explain connection lifecycle requirements.

## Non-goals

- Building a full application template.
- Documenting Wrangler CLI in detail.
- Providing a production SMTP server setup guide.

## Design

### Minimal Workers handler

```rust
use worker::{event, Context, Env, Request, Response, Result};
use wasm_smtp_cloudflare::connect_smtps;

#[event(fetch)]
async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    let host   = env.secret("SMTP_HOST")?.to_string();
    let port: u16 = env.var("SMTP_PORT")?.to_string().parse().unwrap_or(465);
    let user   = env.secret("SMTP_USER")?.to_string();
    let pass   = env.secret("SMTP_PASS")?.to_string();

    let mut client = connect_smtps(&host, port, "worker.example.com")
        .await
        .map_err(|e| worker::Error::RustError(e.to_string()))?;

    client.login(&user, &pass).await
        .map_err(|e| worker::Error::RustError(e.to_string()))?;

    client.send_mail(
        &user,
        &["recipient@example.org"],
        "From: sender@example.com\r\n\
         To: recipient@example.org\r\n\
         Subject: Hello from a Worker\r\n\
         \r\n\
         This message was sent via wasm-smtp-cloudflare.\r\n",
    ).await.map_err(|e| worker::Error::RustError(e.to_string()))?;

    client.quit().await.ok();
    Response::ok("Sent")
}
```

### Credential retrieval

SMTP credentials are stored as Workers Secrets (never in `vars` or
committed to source). The example above uses `env.secret(...)`.

### Known limitations

| Constraint | Detail |
|---|---|
| Port 25 blocked | Cloudflare blocks outbound port 25. Use 465 (implicit TLS) or 587 (STARTTLS). |
| Private networks blocked | Connections to RFC 1918 addresses, localhost, and Cloudflare's own IP ranges will fail. Only publicly reachable SMTP submission endpoints are supported. |
| Request-scoped connections | A `CloudflareTransport` must not be stored in a global variable and reused across requests. Create a new connection per request (or per email-send operation within a request). |
| No connection pooling | Workers' request isolation prevents connection pooling. Each request pays the TCP + TLS handshake cost. For high-volume sending, consider a dedicated sending service behind the Worker. |
| No port 25 MX delivery | `wasm-smtp-cloudflare` is a submission client, not an MTA. It does not resolve MX records and connect to port 25 of the recipient's mail server. |
| 10 ms CPU limit | Cloudflare Workers have a 10 ms CPU-time limit on the free plan (paid plans have 50 ms / 30 s wall-clock). The TLS handshake and SMTP exchange count toward this limit. |

### Local testing with `wrangler dev`

`wrangler dev` runs the Worker locally with a simulated Workers runtime.
SMTP connections made from `wrangler dev` to a local SMTP server (e.g.,
Mailpit, MailHog) work as long as the local server is on a
publicly-accessible port (not localhost from Cloudflare's perspective in
some networking configurations). For deterministic testing, use the mock
transport from `wasm-smtp`'s test suite directly in unit tests.

## Security considerations

- Never put SMTP credentials in `vars` (plaintext in `wrangler.toml`).
  Use `env.secret(...)` which pulls from Workers Secrets.
- The example calls `client.quit().await.ok()` — the `.ok()` suppresses
  QUIT errors since they are non-fatal. A missing QUIT does not cause
  message loss; it only causes the server to time out the connection.

## Simplicity and maintainability considerations

The minimal example is intentionally short. It does not demonstrate
error conversion helpers, structured logging, or retry logic; those
are application concerns documented in `docs/src/usage.md`.

## Alternatives considered

None. Documentation does not have meaningful design alternatives beyond
scope (what to include).

## Implementation plan

*Documented as part of Phase 3 (v0.3.x). Updated for STARTTLS in Phase 5.*

## Acceptance criteria

- The minimal example in `docs/src/cloudflare-adapter.md` compiles
  against the current crate API without modification.
- All six constraints in the table above are documented.
- The credential-retrieval pattern uses `env.secret`, not `env.var`.

## Open questions

None. Documented and stable.
