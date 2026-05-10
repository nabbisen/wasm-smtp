# Component Model interface

`wasm-smtp-component` exports the `smtp-send` WIT interface defined in
`wit/smtp.wit`, enabling any language with WIT tooling to send email
without writing Rust.

## WIT interface (abbreviated)

```wit
package wasm-smtp:smtp@0.1.0;

interface smtp-send {
    send: func(
        config: smtp-config,
        credentials: smtp-credentials,
        message: smtp-message,
    ) -> result<send-result, send-error>;
}
```

See `wit/smtp.wit` for the complete interface including all record and
variant type definitions.

## Building the component

Prerequisites:

```sh
cargo install cargo-component
rustup target add wasm32-wasip2
```

Build:

```sh
cargo component build --target wasm32-wasip2 -p wasm-smtp-component
# Output: target/wasm32-wasip2/debug/wasm_smtp_component.wasm
```

## Language bindings

### TypeScript / JavaScript (via jco)

```sh
npm install -g @bytecodealliance/jco
jco types wit/smtp.wit -o ./smtp-types
```

### Go (via wit-bindgen)

```sh
wit-bindgen go wit/smtp.wit --out-dir ./smtp_bindings
```

### Python (via componentize-py)

```sh
pip install componentize-py
componentize-py --wit-path wit/smtp.wit bindings .
```

## Calling from TypeScript

```typescript
import { send } from './smtp-types/smtp-send.js';

const result = send(
  { host: 'smtp.example.com', port: 465,
    ehloDomain: 'client.example.com', tlsMode: 'implicit' },
  { username: 'user@example.com', password: 'secret' },
  {
    from: 'user@example.com',
    to: ['recipient@example.org'],
    rawMessage:
      'From: user@example.com\r\nTo: recipient@example.org\r\n' +
      'Subject: Hello\r\n\r\nBody.\r\n',
  }
);
```

## Credential security model

Credentials are passed as plain WIT strings on each `send` call and
are **not** retained by the component between calls. The component
does not log, store, or expose credentials in any way.

The host runtime controls what network access the component has.
Ensure the runtime is configured to restrict outbound TCP to trusted
SMTP servers.

## Rust unit tests (no WASM runtime required)

```sh
cargo test -p wasm-smtp-component
```

The test suite runs on native hosts using hand-written type stubs that
mirror the WIT-generated types, so no WASM toolchain is needed for
`cargo test`.
