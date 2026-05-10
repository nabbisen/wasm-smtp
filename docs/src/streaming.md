# Streaming DATA

`send_mail_stream` sends a message body from any `MessageBody` source
in bounded memory — O(chunk size) rather than O(body size). This is
useful for large messages and memory-constrained runtimes (Cloudflare
Workers, WASI components).

## MessageBody trait

Implement `MessageBody` for any async byte source:

```rust
use wasm_smtp::message_body::MessageBody;
use wasm_smtp::IoError;

struct FileBody {
    data: Vec<u8>,
    pos: usize,
}

impl MessageBody for FileBody {
    async fn read_chunk(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        if self.pos >= self.data.len() {
            return Ok(0); // EOF
        }
        let n = buf.len().min(self.data.len() - self.pos);
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}
```

Return `Ok(0)` to signal end of body. Return `Err(IoError)` to abort
the DATA phase.

## Built-in implementations

| Type | Source |
|------|--------|
| `SliceBody<'a>` | `&[u8]` |
| `StrBody<'a>` | `&str` |

## Usage

```rust
use wasm_smtp::message_body::StrBody;

let body = "Subject: hello\r\n\r\nworld\r\n";
client.send_mail_stream(
    "from@example.com",
    &["to@example.com"],
    &mut StrBody::new(body),
).await?;
```

## Body requirements

The body must be a fully composed RFC 5322 message (headers + blank
line + content) with **CRLF** (`\r\n`) line endings. Dot-stuffing and
the end-of-data terminator (`\r\n.\r\n`) are applied automatically.

## Chunk size

`send_mail_stream` uses an 8 KB internal buffer. This is not
configurable in the current release.

## Policy note

`check_message_size` is called with `usize::MAX` because the total
body size is unknown in advance. Use `send_mail_bytes` when precise
size enforcement is needed.

## DotStufferState

The streaming dot-stuffer is exposed as `wasm_smtp::DotStufferState`
for advanced use cases. `process_chunk(&[u8]) -> Vec<u8>` handles
chunk-boundary dot insertion correctly; `finish() -> Vec<u8>` produces
the end-of-DATA terminator.
