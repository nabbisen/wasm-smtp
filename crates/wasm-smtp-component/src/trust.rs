//! Trust-anchor parsing for the `smtp-config` resource (RFC 030 D3).
//!
//! Turns the caller's `trust-anchors` choice into the root store the WASI
//! adapter connects with, or into an error, before any connection exists.
//! This is the only code in the component that decides who is trusted, so
//! RFC 030 D3's rules are invariants here, not guidance:
//!
//! 1. `custom` **replaces** the bundled roots; nothing is merged.
//! 2. A rejected `custom` is an error. It never falls back to `bundled`.
//! 3. Every certificate block is added with [`RootCertStore::add`], which
//!    returns an error per certificate. The skipping
//!    `RootCertStore::add_parsable_certificates` is not used: it would turn
//!    "trust these three" into "trust whichever of these three parsed".
//! 4. Text outside PEM blocks is ignored, because real bundles carry
//!    comments, but every block must be labelled `CERTIFICATE`. A private
//!    key pasted by mistake fails; it is not skipped.
//! 5. Errors never contain the input: no PEM text, no label read from the
//!    input, no decoded bytes. They name a block ordinal and a fixed
//!    reason, and every message is built in [`TrustError`]'s `Display`.
//! 6. At least one certificate is required.
//! 7. At most [`MAX_BLOCKS`] blocks and [`MAX_INPUT_BYTES`] bytes. The size
//!    is checked before anything is parsed; the count fails as soon as
//!    block 65 is seen.
//!
//! Rule 8, no verification bypass, holds by construction: nothing here can
//! produce anything but a root store.
//!
//! The blocks are scanned here rather than with `rustls-pki-types`'
//! section iterator, which silently skips blocks with a label it does not
//! recognise (rule 4) and whose errors carry input bytes (rule 5). A block
//! is handed to `pki-types` only once its label is known to be
//! `CERTIFICATE` and its END line has been found, and only for base64
//! decoding; its error is mapped to a fixed reason, never formatted.

use core::fmt;

use rustls::RootCertStore;
use rustls_pki_types::CertificateDer;
use rustls_pki_types::pem::PemObject;

/// The most `custom` input accepted, in bytes: 256 KiB (RFC 030 D3 rule 7).
pub(crate) const MAX_INPUT_BYTES: usize = 256 * 1024;

/// The most PEM blocks accepted in `custom` (RFC 030 D3 rule 7).
pub(crate) const MAX_BLOCKS: usize = 64;

const BEGIN: &str = "-----BEGIN ";
const DASHES: &str = "-----";
const CERTIFICATE_LABEL: &str = "CERTIFICATE";
const CERTIFICATE_END: &str = "-----END CERTIFICATE-----";

/// The caller's trust choice, as the WIT `trust-anchors` variant carries it.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Trust<'a> {
    /// The adapter's bundled root set.
    Bundled,
    /// Exactly these certificate authorities, as PEM text.
    Custom(&'a str),
}

/// Why trust input was rejected. Deliberately carries nothing from the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reason {
    TooLarge,
    TooManyBlocks,
    NoCertificates,
    NotACertificate,
    Unterminated,
    MalformedBase64,
    RejectedByRootStore,
}

/// A rejected trust input: a fixed reason, and the 1-based ordinal of the
/// block it concerns when it concerns one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TrustError {
    block: Option<usize>,
    reason: Reason,
}

impl TrustError {
    const fn whole(reason: Reason) -> Self {
        Self {
            block: None,
            reason,
        }
    }

    const fn at(block: usize, reason: Reason) -> Self {
        Self {
            block: Some(block),
            reason,
        }
    }

    /// The reason, for tests and for callers that branch on it.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) const fn reason(self) -> Reason {
        self.reason
    }

    /// The block ordinal, when the error concerns one block.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) const fn block(self) -> Option<usize> {
        self.block
    }
}

impl fmt::Display for TrustError {
    /// Every message a caller can see is built here, and only here, from a
    /// block ordinal and one of these fixed strings (RFC 030 D3 rule 5).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let reason = match self.reason {
            Reason::TooLarge => "input exceeds 256 KiB",
            Reason::TooManyBlocks => "more than 64 PEM blocks",
            Reason::NoCertificates => "no certificate blocks",
            Reason::NotACertificate => "not a certificate",
            Reason::Unterminated => "block has no matching END line",
            Reason::MalformedBase64 => "malformed base64",
            Reason::RejectedByRootStore => "rejected by the root store",
        };
        match self.block {
            Some(n) => write!(f, "trust anchors: block {n}: {reason}"),
            None => write!(f, "trust anchors: {reason}"),
        }
    }
}

/// Resolve a trust choice. `Ok(None)` means the adapter's bundled roots;
/// `Ok(Some(store))` holds exactly the caller's certificates.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn resolve(trust: Trust<'_>) -> Result<Option<RootCertStore>, TrustError> {
    match trust {
        Trust::Bundled => Ok(None),
        Trust::Custom(pem) => parse_custom(pem).map(Some),
    }
}

/// Parse `custom` PEM into a root store holding every certificate in it,
/// or reject the whole input.
pub(crate) fn parse_custom(pem: &str) -> Result<RootCertStore, TrustError> {
    // Rule 7: the size bound comes before any parsing at all.
    if pem.len() > MAX_INPUT_BYTES {
        return Err(TrustError::whole(Reason::TooLarge));
    }

    // Built locally and returned only on full success, so no caller can
    // ever hold a store that contains some of the input (rule 3).
    let mut store = RootCertStore::empty();
    let mut blocks = 0usize;
    let mut offset = 0usize;
    let mut lines = pem.split_inclusive('\n');

    while let Some(line) = lines.next() {
        let start = offset;
        offset += line.len();
        let Some(label) = begin_label(line) else {
            // Text outside a block: a comment in a bundle (rule 4).
            continue;
        };

        blocks += 1;
        if blocks > MAX_BLOCKS {
            return Err(TrustError::at(blocks, Reason::TooManyBlocks));
        }
        if label != CERTIFICATE_LABEL {
            return Err(TrustError::at(blocks, Reason::NotACertificate));
        }

        // Find this block's END line. Another BEGIN first means the block
        // was never closed.
        let mut end = None;
        for body_line in lines.by_ref() {
            offset += body_line.len();
            let trimmed = body_line.trim_end();
            if trimmed == CERTIFICATE_END {
                end = Some(offset);
                break;
            }
            if trimmed.starts_with(BEGIN) {
                return Err(TrustError::at(blocks, Reason::Unterminated));
            }
        }
        let Some(end) = end else {
            return Err(TrustError::at(blocks, Reason::Unterminated));
        };

        let block = &pem.as_bytes()[start..end];
        let Ok(der) = CertificateDer::from_pem_slice(block) else {
            return Err(TrustError::at(blocks, Reason::MalformedBase64));
        };
        if store.add(der).is_err() {
            return Err(TrustError::at(blocks, Reason::RejectedByRootStore));
        }
    }

    // Rule 6: a trust store that trusts nothing is a misconfiguration.
    if blocks == 0 {
        return Err(TrustError::whole(Reason::NoCertificates));
    }
    Ok(store)
}

/// The label of a `-----BEGIN <label>-----` line, if this is one.
fn begin_label(line: &str) -> Option<&str> {
    line.trim_end().strip_prefix(BEGIN)?.strip_suffix(DASHES)
}

#[cfg(test)]
mod tests {
    use super::{MAX_BLOCKS, MAX_INPUT_BYTES, Reason, Trust, TrustError, parse_custom, resolve};

    /// A freshly generated self-signed certificate and its private key, as
    /// PEM. Generated per test run; nothing is committed.
    fn cert_and_key() -> (String, String) {
        let certified =
            rcgen::generate_simple_self_signed(vec!["ca.example".to_owned()]).expect("rcgen");
        (certified.cert.pem(), certified.signing_key.serialize_pem())
    }

    fn reason_of(input: &str) -> (Option<usize>, Reason) {
        let err = parse_custom(input).expect_err("input should be rejected");
        (err.block(), err.reason())
    }

    #[test]
    fn one_valid_certificate_is_the_whole_store() {
        let (cert, _) = cert_and_key();
        let store = parse_custom(&cert).expect("valid certificate");
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn comments_between_blocks_are_ignored() {
        let (a, _) = cert_and_key();
        let (b, _) = cert_and_key();
        let bundle = format!(
            "# Corporate root, rotated 2026\n{a}\nIssuer: Example Intermediate\n\n{b}# end of bundle\n"
        );
        let store = parse_custom(&bundle).expect("bundle with comments");
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn crlf_line_endings_are_accepted() {
        let (cert, _) = cert_and_key();
        let crlf = cert.replace('\n', "\r\n");
        assert_eq!(parse_custom(&crlf).expect("CRLF certificate").len(), 1);
    }

    #[test]
    fn a_private_key_block_alone_is_rejected() {
        let (_, key) = cert_and_key();
        assert_eq!(reason_of(&key), (Some(1), Reason::NotACertificate));
    }

    #[test]
    fn a_private_key_after_a_valid_certificate_fails_the_whole_input() {
        let (cert, key) = cert_and_key();
        let input = format!("{cert}{key}");
        assert_eq!(reason_of(&input), (Some(2), Reason::NotACertificate));
    }

    #[test]
    fn an_unrecognised_label_is_rejected_not_skipped() {
        let (cert, _) = cert_and_key();
        let relabelled = cert.replace("CERTIFICATE", "TRUSTED CERTIFICATE");
        assert_eq!(reason_of(&relabelled), (Some(1), Reason::NotACertificate));
    }

    #[test]
    fn valid_base64_that_is_not_a_certificate_is_rejected_by_the_root_store() {
        // "this is not DER, it is only valid base64" in base64.
        let input = "-----BEGIN CERTIFICATE-----\n\
                     dGhpcyBpcyBub3QgREVSLCBpdCBpcyBvbmx5IHZhbGlkIGJhc2U2NA==\n\
                     -----END CERTIFICATE-----\n";
        assert_eq!(reason_of(input), (Some(1), Reason::RejectedByRootStore));
    }

    #[test]
    fn a_malformed_certificate_after_a_valid_one_fails_the_whole_input() {
        let (cert, _) = cert_and_key();
        let input = format!(
            "{cert}-----BEGIN CERTIFICATE-----\nbm90IGEgY2VydGlmaWNhdGU=\n-----END CERTIFICATE-----\n"
        );
        assert_eq!(reason_of(&input), (Some(2), Reason::RejectedByRootStore));
    }

    #[test]
    fn malformed_base64_is_rejected() {
        let input = "-----BEGIN CERTIFICATE-----\n!!! not base64 !!!\n-----END CERTIFICATE-----\n";
        assert_eq!(reason_of(input), (Some(1), Reason::MalformedBase64));
    }

    #[test]
    fn a_block_without_its_end_line_is_rejected() {
        let (cert, _) = cert_and_key();
        let truncated = cert.replace("-----END CERTIFICATE-----", "");
        assert_eq!(reason_of(&truncated), (Some(1), Reason::Unterminated));
        let nested = cert.replacen(
            "-----END CERTIFICATE-----",
            "-----BEGIN CERTIFICATE-----",
            1,
        );
        assert_eq!(reason_of(&nested), (Some(1), Reason::Unterminated));
    }

    #[test]
    fn no_blocks_is_rejected() {
        assert_eq!(reason_of(""), (None, Reason::NoCertificates));
        assert_eq!(
            reason_of("# only a comment, no certificates\n"),
            (None, Reason::NoCertificates)
        );
    }

    #[test]
    fn sixty_four_certificates_are_accepted_and_sixty_five_are_not() {
        let (cert, _) = cert_and_key();
        let sixty_four = cert.repeat(MAX_BLOCKS);
        assert!(
            sixty_four.len() <= MAX_INPUT_BYTES,
            "the count test must not hit the size limit"
        );
        assert!(parse_custom(&sixty_four).is_ok());
        let sixty_five = cert.repeat(MAX_BLOCKS + 1);
        assert_eq!(reason_of(&sixty_five), (Some(65), Reason::TooManyBlocks));
    }

    #[test]
    fn the_size_limit_is_checked_before_parsing() {
        let (cert, _) = cert_and_key();
        // Exactly the limit is allowed through to parsing: comment text and
        // one certificate, padded to 256 KiB.
        let padding = MAX_INPUT_BYTES - cert.len();
        let at_limit = format!("{}{cert}", "#".repeat(padding - 1) + "\n");
        assert_eq!(at_limit.len(), MAX_INPUT_BYTES);
        assert!(parse_custom(&at_limit).is_ok());
        // One byte more fails as too large, although it holds a valid
        // certificate: nothing was parsed.
        let over = format!("#{at_limit}");
        assert_eq!(over.len(), MAX_INPUT_BYTES + 1);
        assert_eq!(reason_of(&over), (None, Reason::TooLarge));
    }

    #[test]
    fn bundled_needs_no_parsing() {
        assert!(matches!(resolve(Trust::Bundled), Ok(None)));
        let (cert, _) = cert_and_key();
        assert!(matches!(resolve(Trust::Custom(&cert)), Ok(Some(store)) if store.len() == 1));
    }

    #[test]
    fn a_rejected_custom_never_resolves_to_bundled() {
        let (_, key) = cert_and_key();
        assert!(resolve(Trust::Custom(&key)).is_err());
        assert!(resolve(Trust::Custom("")).is_err());
    }

    /// The input may be a private key pasted by mistake, so no error may
    /// contain any part of it (RFC 030 D3 rule 5).
    #[test]
    fn errors_never_echo_the_input() {
        let (cert, key) = cert_and_key();
        let body: String = key.lines().filter(|l| !l.starts_with("-----")).collect();
        assert!(
            body.len() >= 64,
            "the key must have a real body to search for"
        );

        for input in [
            key.clone(),
            format!("{cert}{key}"),
            format!("# note\n{key}"),
        ] {
            let message = parse_custom(&input)
                .expect_err("a key is not trust input")
                .to_string();
            for line in key.lines().filter(|l| !l.trim().is_empty()) {
                assert!(
                    !message.contains(line),
                    "error echoes a PEM line: {message}"
                );
            }
            for window in body.as_bytes().windows(16) {
                let window = core::str::from_utf8(window).expect("base64 is ASCII");
                assert!(
                    !message.contains(window),
                    "error echoes key material: {message}"
                );
            }
            assert!(
                !message.contains("PRIVATE KEY"),
                "error echoes the label: {message}"
            );
        }
    }

    #[test]
    fn every_reason_formats_without_input() {
        // The format table, exercised, so a change to it is visible in review.
        let cases = [
            (
                TrustError::whole(Reason::TooLarge),
                "trust anchors: input exceeds 256 KiB",
            ),
            (
                TrustError::at(65, Reason::TooManyBlocks),
                "trust anchors: block 65: more than 64 PEM blocks",
            ),
            (
                TrustError::whole(Reason::NoCertificates),
                "trust anchors: no certificate blocks",
            ),
            (
                TrustError::at(2, Reason::NotACertificate),
                "trust anchors: block 2: not a certificate",
            ),
            (
                TrustError::at(1, Reason::Unterminated),
                "trust anchors: block 1: block has no matching END line",
            ),
            (
                TrustError::at(1, Reason::MalformedBase64),
                "trust anchors: block 1: malformed base64",
            ),
            (
                TrustError::at(3, Reason::RejectedByRootStore),
                "trust anchors: block 3: rejected by the root store",
            ),
        ];
        for (err, text) in cases {
            assert_eq!(err.to_string(), text);
        }
    }
}
