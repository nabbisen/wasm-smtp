# wasm-smtp-test

Test transport and fixtures for [`wasm-smtp`](https://crates.io/crates/wasm-smtp).

**For development and testing only.** Not for production use.

## What this crate provides

- [`MockTransport`] — a synchronous, scripted SMTP transport that replays
  pre-programmed server responses and records client output.
- [`block_on`] — drives a `wasm-smtp` future to completion without an async
  executor, so tests can use `#[test]` instead of `#[tokio::test]`.
- [`flatten`] — concatenates byte slices for building multi-line responses.
- [`UpgradeBehavior`] — configures whether `upgrade_to_tls()` succeeds or
  fails, for testing STARTTLS paths.

## License

Apache-2.0. See [LICENSE](../../LICENSE).
