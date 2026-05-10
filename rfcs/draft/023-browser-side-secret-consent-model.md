# RFC 023 — Browser-side secret and consent model

**Status.** Draft
**Priority.** P3
**Tracks.** Security / Browser
**Touches.** `docs/src/browser-security.md`, `crates/wasm-smtp-direct-sockets/` (future)

## Summary

Define the minimum security requirements for handling SMTP credentials
in a browser context, specifically when using `wasm-smtp-direct-sockets`
(RFC 022) from an Isolated Web App or other high-trust browser context.

## Motivation

SMTP credentials in a browser are at higher risk than in a server-side
context. Browsers run untrusted third-party code (extensions, iframes,
injected scripts). Even in an IWA's hardened CSP environment, the
credential lifecycle must be designed defensively to avoid:

- Credential exfiltration via XSS or compromised extensions.
- Credentials leaking into browser developer tools.
- Credentials persisting in browser storage after the session ends.
- Silent (no-user-consent) SMTP connections.

## Goals

- Define the minimum credential lifetime rule: credentials exist only
  in JS memory for the duration of the SMTP call.
- Prohibit credential persistence in any browser storage API.
- Require user-initiated (not automatic) SMTP connections.
- Define the audit trail the IWA should maintain (without the credential).
- Warn about phishing / exfiltration risks in the adapter documentation.

## Non-goals

- Implementing a credential manager.
- Designing a browser extension.
- Prescribing specific UI patterns (that's the application's job).
- Full application security architecture for IWAs.

## Design

### Credential lifetime rule

SMTP credentials (username, password, OAuth token) must:

1. Be collected from the user immediately before the SMTP call.
2. Be passed to `send_mail` / `login` as function arguments.
3. Not be assigned to any module-level or class-level variable.
4. Not be stored in `localStorage`, `sessionStorage`, `IndexedDB`,
   `CacheStorage`, cookies, or any other browser-managed persistence.
5. Be overwritten (in languages that support it) or allowed to be
   garbage-collected immediately after the call completes.

In practice, in JavaScript / TypeScript:

```typescript
async function sendEmail(form: EmailFormData): Promise<void> {
    // Credentials are local variables, not stored anywhere.
    const password = form.elements['password'].value;
    const username = form.elements['username'].value;

    await smtpSend(config, { username, password }, message);

    // Do not retain username / password beyond this function.
    // JS GC will collect them; there is no manual zeroing in JS.
}
```

### No persistent credential storage

The adapter documentation must explicitly state:

> Storing SMTP credentials in `localStorage`, `sessionStorage`, or
> `IndexedDB` is **prohibited**. These storage APIs are accessible to
> JavaScript running in the same origin, including browser extensions
> with host permissions. A compromised extension or XSS attack can
> silently exfiltrate stored credentials.
>
> If users object to entering credentials on every send, the recommended
> pattern is OAuth 2.0 / XOAUTH2: the IWA obtains a short-lived access
> token via an OAuth flow (which persists the refresh token securely via
> the OS credential store, not browser storage) and uses the token for
> SMTP.

### User-initiated connections

SMTP connections from `wasm-smtp-direct-sockets` must be initiated by
an explicit user action (button click, form submit). The adapter must
not automatically open TCP connections on page load.

This is enforced by design: the `DirectSocketsTransport` constructor
requires calling `navigator.openTCPSocket`, which requires a user
gesture in the browser (Chrome enforces this for Direct Sockets API
access in some contexts).

### Audit event in the IWA

The IWA should record an audit event for each SMTP send (timestamp,
recipient count, outcome) in a local log visible to the user. The
credential itself must not appear in the log. This mirrors the
server-side audit model in RFC 012.

### Phishing and exfiltration warnings

The adapter crate documentation includes a threat model section:

- **Malicious extension:** a browser extension with host permissions can
  read DOM values (including password fields before or after the user
  types) and intercept function calls. IWA's strict CSP limits extension
  access, but extensions with the right permissions can still interfere.
  Mitigation: use hardware security keys or OS-level credential managers
  where available.

- **XSS:** injected script can call the adapter's exported functions.
  IWA's strict CSP prevents most XSS; any user-supplied content rendered
  in the IWA must still be sanitised.

- **Man-in-the-middle:** mitigated by TLS (required; no plaintext mode).

- **Compromised SMTP server:** SCRAM-SHA-256 prevents the server from
  learning the raw password. For maximum protection, use SCRAM when
  the server supports it.

## Security considerations

This RFC is entirely security. The key properties:

1. **No persistence** — credentials exist only for the call duration.
2. **User consent** — connections are user-initiated.
3. **TLS required** — credentials are never sent in plaintext.
4. **Audit trail** — outcome (not credential) is recorded.
5. **Threat model disclosed** — users of the adapter understand the risks.

## Simplicity and maintainability considerations

The security requirements are documentation constraints, not code
constraints. The adapter itself cannot enforce "do not call localStorage"
— that is the IWA developer's responsibility. The documentation is the
mechanism.

## Alternatives considered

**Implement credential storage with encryption:** rejected. Encrypted
credential storage in the browser is complex (key management, the key
itself must be stored somewhere) and provides false assurance. No
persistent storage is simpler and more defensible.

**Require OS-level credential manager integration:** desirable but
platform-specific. An IWA on ChromeOS has access to the ChromeOS
keychain; on other platforms, options vary. This is an application-level
concern beyond the adapter's scope.

## Implementation plan

*Not yet implemented. Companion to RFC 022 (Direct Sockets adapter).*

When RFC 022 moves from Draft to Proposed, this RFC moves simultaneously.
They are implemented together.

## Acceptance criteria

*Applicable when the adapter is eventually implemented:*

- The adapter crate-level doc comment includes the credential lifetime
  rule and the prohibition on browser storage.
- The adapter crate includes a `docs/src/browser-security.md` that covers
  the threat model.
- No function in the adapter API accepts or returns a credential as a
  persistent type.

## Open questions

1. Should the adapter emit a compile-time or runtime warning if the
   application passes credentials as `static` variables? This would
   require some instrumentation but could catch common misuse.
2. Is OAuth 2.0 / XOAUTH2 the right recommendation for "avoid re-entering
   password"? The OAuth flow requires a backend redirect endpoint, which
   is awkward for a fully client-side IWA.
3. Should RFC 022 and RFC 023 be merged into a single RFC? They were
   separated to mirror the extension plan's structure but the content
   is tightly coupled.
