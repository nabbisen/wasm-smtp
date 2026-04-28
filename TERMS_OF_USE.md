# Terms of Use

`wasm-smtp` is a library for sending mail by SMTP from constrained
runtimes (initially Cloudflare Workers). The Apache License governs the
copyright and patent terms; this document defines the additional rules
of acceptable use that authors of this project consider non-negotiable.

## Prohibited uses

You **must not** use this library, in whole or in part, to:

- Send unsolicited bulk email ("spam"), regardless of the legal status
  of such activity in your jurisdiction.
- Send mail with forged or misleading envelope-sender, header-sender, or
  identity information ("spoofing", "impersonation").
- Send mail in volumes or at frequencies that exceed the operating
  policy of the SMTP server you are submitting to, whether that policy
  is published or has been communicated to you privately.
- Circumvent rate limits, abuse-prevention measures, or authentication
  requirements of any SMTP server or hosting platform.
- Send mail on behalf of a third party without that party's clear,
  prior, and revocable consent.

## Operator responsibility

You are solely responsible for:

- Confirming that your intended use complies with the operating policy
  of the SMTP server you are submitting to.
- Confirming that your intended use complies with applicable law in
  every jurisdiction in which you, your recipients, your SMTP server
  operator, or the runtime host (e.g. Cloudflare) operates.
- Honoring opt-out requests, list-hygiene obligations, and any other
  duties imposed on the sender of email by applicable law.

## Scope

These terms apply to use of the unmodified library, to derivative works,
and to any integration that embeds the library at any layer. They do
not enlarge or restrict the rights granted by the Apache License; they
state the conditions under which the authors are willing to consider a
use of the library to be a use the authors endorse.

If you cannot or will not abide by these terms, you should not use this
library.
