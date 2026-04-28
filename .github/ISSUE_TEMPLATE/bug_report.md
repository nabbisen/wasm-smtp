---
name: Bug report
about: Report incorrect SMTP behavior, panics, or unexpected errors
title: '[bug] '
labels: bug
assignees: ''
---

## Summary

A short description of the problem.

## Affected crate and version

- Crate: `wasm-smtp` / `wasm-smtp-cloudflare`
- Version or commit hash:
- Rust version (`rustc --version`):

## Reproducer

The smallest piece of code, scripted server reply, or `cargo test`
invocation that demonstrates the issue.

```rust
// ...
```

## Expected behavior

What you expected to happen.

## Actual behavior

What actually happened. Include the full error chain
(`{:#}` formatting), backtraces, and any relevant log output.

## Server context (if relevant)

- SMTP server software and version (e.g. Postfix 3.7, Sendmail, an
  ESP's submission endpoint):
- Port (465 implicit TLS expected):
- Any non-standard server behavior you are aware of:

## Additional notes

Anything else that might help — recent changes, related issues,
hypotheses, etc.
