//! SCRAM-SHA-256 (RFC 5802) authentication helpers.
//!
//! This module provides the pure-algorithmic pieces of the
//! SCRAM-SHA-256 SASL mechanism: message construction, parsing of
//! the server-first message, computation of the client proof, and
//! verification of the server signature. The transport-level glue
//! that drives the SMTP `AUTH SCRAM-SHA-256` exchange lives in
//! `client.rs`.
//!
//! # SCRAM-SHA-256 in 60 seconds
//!
//! The mechanism is a four-message dance:
//!
//! ```text
//! client_first  (C → S):  n,,n=user,r=client_nonce
//! server_first  (S → C):  r=server_nonce,s=salt_b64,i=iter_count
//! client_final  (C → S):  c=biws,r=server_nonce,p=proof_b64
//! server_final  (S → C):  v=server_signature_b64
//! ```
//!
//! The client proof is computed without ever sending the password
//! to the server. Both sides derive shared key material via
//! PBKDF2(HMAC-SHA-256, password, salt, `iter_count`); the client's
//! proof is HMAC-derived from this and bound to the entire
//! conversation through the `AuthMessage`. A man-in-the-middle that
//! observed only the SCRAM messages cannot recover the password.
//!
//! # What this module does NOT implement
//!
//! - **Channel binding (`SCRAM-SHA-256-PLUS`).** RFC 5802 §6 defines
//!   a `-PLUS` variant that mixes TLS channel-binding tokens into
//!   the `AuthMessage` to defend against a class of `MITM` attacks
//!   distinct from SCRAM's baseline guarantees. Implementing this
//!   would require pulling per-connection binding tokens out of the
//!   TLS implementation, which is out of scope for the current
//!   `Transport` / `StartTlsCapable` contract. Callers needing
//!   channel binding should pin TLS at the transport layer
//!   (validate certificates, validate hostnames) which is what
//!   the `Transport` trait already requires.
//!
//! - **SCRAM-SHA-1.** RFC 5802 also defines SHA-1 variants. SHA-1
//!   is no longer recommended for new authentication deployments
//!   and is not implemented here.
//!
//! - **`SASLprep` / RFC 4013 normalization.** The username and
//!   password are passed through to the underlying primitives
//!   without Unicode normalization. This is acceptable for ASCII
//!   credentials (the common case) and matches the behaviour of
//!   `validate_login_username` / `validate_login_password`. Callers
//!   with non-ASCII credentials should normalize them before
//!   submission.

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::error::AuthError;
use crate::protocol::{base64_decode, base64_encode};

/// Length in bytes of the SHA-256 output. Used for `ClientKey`,
/// `StoredKey`, `ServerKey`, `ClientSignature`, etc.
pub(crate) const SHA256_LEN: usize = 32;

/// Minimum acceptable PBKDF2 iteration count. RFC 5802 §5.1 does
/// not mandate a floor, but iteration counts below 4096 are
/// considered weak in modern threat models. We accept the
/// server's value but reject obviously-broken configurations.
pub(crate) const MIN_PBKDF2_ITERATIONS: u32 = 4096;

/// Maximum acceptable PBKDF2 iteration count. RFC 5802 §5.1 does
/// not mandate a ceiling either, but a malicious server could
/// supply an absurdly large value to `DoS` the client. 600,000 is
/// roughly 10x the OWASP 2023 recommendation; anything above is
/// almost certainly an attack or a configuration error.
pub(crate) const MAX_PBKDF2_ITERATIONS: u32 = 600_000;

/// Length in bytes of the client nonce we generate. RFC 5802
/// recommends >= 18 random octets; we use 24 for a safety margin.
/// Encoded as base64, this becomes 32 ASCII characters.
pub(crate) const CLIENT_NONCE_LEN: usize = 24;

// -----------------------------------------------------------------------------
// Public-crate API
// -----------------------------------------------------------------------------

/// Generate a base64-encoded random client nonce.
///
/// Uses `getrandom` to source bytes from the OS CSPRNG. Failures
/// (extremely rare — only on platforms without entropy access)
/// surface as `AuthError::Other`.
pub(crate) fn generate_client_nonce() -> Result<String, AuthError> {
    let mut bytes = [0u8; CLIENT_NONCE_LEN];
    getrandom::getrandom(&mut bytes).map_err(|_| AuthError::Other("CSPRNG unavailable"))?;
    // Per RFC 5802 §5.1, the nonce is "a sequence of random printable
    // ASCII characters excluding ','". Base64 satisfies that.
    Ok(base64_encode(&bytes))
}

/// Build the `client-first-message` SCRAM frame.
///
/// Format: `n,,n=<username>,r=<client_nonce>`
///
/// The leading `n,,` is the GS2 header signalling "no channel
/// binding, no authzid". The `<username>` is sent verbatim — RFC
/// 5802 §5.1 requires `SASLprep`, but for ASCII credentials this is
/// a no-op. The `,` and `=` characters in the username must be
/// escaped to `=2C` and `=3D` respectively.
pub(crate) fn build_client_first(username: &str, client_nonce: &str) -> String {
    let escaped_user = escape_saslname(username);
    format!("n,,n={escaped_user},r={client_nonce}")
}

/// Server-first message parsed into its three components.
#[derive(Debug)]
pub(crate) struct ServerFirst {
    /// The server's nonce. Per RFC 5802 §5.1, this MUST start with
    /// the client nonce we sent; we verify that on parse.
    pub(crate) nonce: String,
    /// Salt, base64-decoded.
    pub(crate) salt: Vec<u8>,
    /// PBKDF2 iteration count.
    pub(crate) iterations: u32,
}

/// Parse the server-first message and verify it is well-formed.
///
/// Format: `r=<nonce>,s=<salt_b64>,i=<iterations>` (extensions
/// allowed but ignored). The `nonce` MUST start with `client_nonce`
/// — this prevents replay attacks. The iteration count MUST fall
/// within the defended-floor / defended-ceiling range.
pub(crate) fn parse_server_first(
    server_first: &str,
    client_nonce: &str,
) -> Result<ServerFirst, AuthError> {
    // Each attribute is `key=value`, separated by ','. Mandatory
    // keys: r, s, i. Extensions (m=, ...) precede these and signal
    // optional features we don't support; per RFC 5802 §5.1, we
    // MUST fail if we see an `m=` we don't understand.
    let mut nonce: Option<&str> = None;
    let mut salt_b64: Option<&str> = None;
    let mut iterations: Option<u32> = None;

    for attr in server_first.split(',') {
        let (key, value) = attr
            .split_once('=')
            .ok_or(AuthError::Other("malformed server-first message"))?;

        match key {
            "r" => nonce = Some(value),
            "s" => salt_b64 = Some(value),
            "i" => {
                iterations = Some(
                    value
                        .parse::<u32>()
                        .map_err(|_| AuthError::Other("server iteration count not a u32"))?,
                );
            }
            "m" => {
                // RFC 5802 §5.1: "If the server sends the
                // server-first-message with the optional extension
                // 'm', the client MUST fail authentication if it
                // does not support the extension."
                return Err(AuthError::Other(
                    "server requested an unsupported SCRAM extension",
                ));
            }
            _ => {
                // Unknown extensions after the mandatory attributes
                // are ignored per the spec's forward-compat rules.
            }
        }
    }

    let nonce = nonce.ok_or(AuthError::Other("server-first missing r="))?;
    let salt_b64 = salt_b64.ok_or(AuthError::Other("server-first missing s="))?;
    let iterations = iterations.ok_or(AuthError::Other("server-first missing i="))?;

    // Replay defense: the server's nonce MUST extend our client nonce.
    if !nonce.starts_with(client_nonce) {
        return Err(AuthError::Other(
            "server nonce does not start with client nonce",
        ));
    }

    if !(MIN_PBKDF2_ITERATIONS..=MAX_PBKDF2_ITERATIONS).contains(&iterations) {
        return Err(AuthError::Other(
            "server iteration count outside acceptable range",
        ));
    }

    let salt = base64_decode(salt_b64)
        .map_err(|_| AuthError::Other("server-first salt is not valid base64"))?;

    Ok(ServerFirst {
        nonce: nonce.to_string(),
        salt,
        iterations,
    })
}

/// Result of the cryptographic exchange: the bytes to send back to
/// the server, plus the expected server signature for later
/// verification.
#[derive(Debug)]
pub(crate) struct ClientFinal {
    /// The fully-formatted `client-final-message` to send.
    pub(crate) message: String,
    /// The expected `ServerSignature` (HMAC of `AuthMessage` with
    /// `ServerKey`). When the server's `server-final` arrives, we
    /// constant-time-compare the verifier to this value.
    pub(crate) expected_server_signature: [u8; SHA256_LEN],
}

/// Compute the client-final message and the expected server
/// signature.
///
/// Implements the cryptographic core of RFC 5802 §3:
///
/// ```text
/// SaltedPassword  = PBKDF2(HMAC-SHA-256, password, salt, iter, 32)
/// ClientKey       = HMAC-SHA-256(SaltedPassword, "Client Key")
/// StoredKey       = SHA-256(ClientKey)
/// AuthMessage     = client_first_bare + "," + server_first + "," + client_final_no_proof
/// ClientSignature = HMAC-SHA-256(StoredKey, AuthMessage)
/// ClientProof     = ClientKey XOR ClientSignature
/// ServerKey       = HMAC-SHA-256(SaltedPassword, "Server Key")
/// ServerSignature = HMAC-SHA-256(ServerKey, AuthMessage)
/// ```
pub(crate) fn compute_client_final(
    username: &str,
    password: &str,
    client_nonce: &str,
    server_first: &ServerFirst,
    server_first_raw: &str,
) -> ClientFinal {
    // SaltedPassword = PBKDF2-HMAC-SHA-256(password, salt, iter, 32)
    let mut salted_password = [0u8; SHA256_LEN];
    pbkdf2::pbkdf2::<Hmac<Sha256>>(
        password.as_bytes(),
        &server_first.salt,
        server_first.iterations,
        &mut salted_password,
    )
    .expect("PBKDF2 with valid output length never fails");

    // ClientKey = HMAC(SaltedPassword, "Client Key")
    let client_key = hmac_sha256(&salted_password, b"Client Key");

    // StoredKey = SHA-256(ClientKey)
    let stored_key: [u8; SHA256_LEN] = Sha256::digest(client_key).into();

    // ServerKey = HMAC(SaltedPassword, "Server Key")
    let server_key = hmac_sha256(&salted_password, b"Server Key");

    // Build client_final_no_proof = "c=biws,r=<server_nonce>"
    // ("biws" is base64("n,,") — the GS2 header from client-first)
    let client_final_no_proof = format!("c=biws,r={}", server_first.nonce);

    // AuthMessage = client_first_bare + "," + server_first + "," + client_final_no_proof
    // client_first_bare = the message minus the GS2 header.
    let client_first_bare = format!("n={},r={}", escape_saslname(username), client_nonce);
    let auth_message = format!("{client_first_bare},{server_first_raw},{client_final_no_proof}");

    // ClientSignature = HMAC(StoredKey, AuthMessage)
    let client_signature = hmac_sha256(&stored_key, auth_message.as_bytes());

    // ClientProof = ClientKey XOR ClientSignature
    let mut client_proof = [0u8; SHA256_LEN];
    for i in 0..SHA256_LEN {
        client_proof[i] = client_key[i] ^ client_signature[i];
    }

    // ServerSignature = HMAC(ServerKey, AuthMessage)
    let expected_server_signature = hmac_sha256(&server_key, auth_message.as_bytes());

    let message = format!("{client_final_no_proof},p={}", base64_encode(&client_proof));

    ClientFinal {
        message,
        expected_server_signature,
    }
}

/// Verify the `server-final` reply against the expected
/// `ServerSignature` we computed earlier.
///
/// Format: `v=<server_signature_b64>` (or `e=<error_string>` if the
/// server is rejecting us).
///
/// The comparison is constant-time to prevent timing side channels
/// even though the server signature is not strictly secret —
/// defence-in-depth at no real cost.
pub(crate) fn verify_server_final(
    server_final: &str,
    expected: &[u8; SHA256_LEN],
) -> Result<(), AuthError> {
    // First, look for an explicit error attribute.
    for attr in server_final.split(',') {
        if let Some(error) = attr.strip_prefix("e=") {
            // The server gave us a SCRAM error; surface it as a
            // generic auth failure with the wire token preserved.
            // Common values: "invalid-proof", "channel-binding-not-supported".
            // We don't try to enumerate them; opaque is fine here.
            let _ = error;
            return Err(AuthError::Other("server rejected SCRAM exchange"));
        }
    }

    // Find v= attribute.
    let v_b64 = server_final
        .split(',')
        .find_map(|attr| attr.strip_prefix("v="))
        .ok_or(AuthError::Other("server-final missing v="))?;

    let v = base64_decode(v_b64)
        .map_err(|_| AuthError::Other("server-final v= is not valid base64"))?;

    if v.len() != SHA256_LEN {
        return Err(AuthError::Other("server-final v= is not 32 bytes"));
    }

    // Constant-time comparison.
    if v.ct_eq(expected.as_slice()).into() {
        Ok(())
    } else {
        Err(AuthError::Other("server signature did not verify"))
    }
}

// -----------------------------------------------------------------------------
// Internal helpers
// -----------------------------------------------------------------------------

/// HMAC-SHA-256 returning a fixed-size 32-byte array.
fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; SHA256_LEN] {
    let mut mac =
        <Hmac<Sha256> as Mac>::new_from_slice(key).expect("HMAC accepts arbitrary key length");
    mac.update(message);
    mac.finalize().into_bytes().into()
}

/// `SASLname` escape: `,` → `=2C`, `=` → `=3D`.
///
/// Per RFC 5802 §5.1, the user's username travels through SCRAM
/// without further encoding, but `,` and `=` carry structural
/// meaning in the SCRAM message format and must be escaped.
fn escape_saslname(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        match ch {
            ',' => out.push_str("=2C"),
            '=' => out.push_str("=3D"),
            other => out.push(other),
        }
    }
    out
}
