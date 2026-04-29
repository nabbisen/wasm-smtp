//! Tests for the `ConnectOptions` builder API.

use crate::ConnectOptions;

#[test]
fn default_options_have_no_overrides() {
    let opts = ConnectOptions::new();
    // We can't introspect private fields, but we can round-trip
    // through Debug to assert structural defaults.
    let dbg = format!("{opts:?}");
    assert!(dbg.contains("server_name: None"));
    assert!(dbg.contains("root_store: None"));
    assert!(dbg.contains("alpn: []"));
}

#[test]
fn with_server_name_sets_the_field() {
    let opts = ConnectOptions::new().with_server_name("override.example.com");
    let dbg = format!("{opts:?}");
    assert!(dbg.contains("override.example.com"));
}

#[test]
fn with_alpn_records_protocols() {
    let opts = ConnectOptions::new().with_alpn(&[b"smtp", b"http/1.1"]);
    // `Vec<Vec<u8>>` Debug-formats as `[[115, 109, 116, 112], ...]` —
    // bytes shown numerically. Rather than fight that, just confirm
    // the structure is non-empty and the right shape.
    let dbg = format!("{opts:?}");
    // The Vec has two entries, so we expect at least one ',' between them
    // somewhere inside `alpn: [...]`.
    assert!(
        dbg.contains("alpn: [[") || dbg.contains("alpn: [ ["),
        "alpn list should be visible in Debug: {dbg}"
    );
    // First byte of "smtp" is 's' = 115. Confirm a representative
    // numeric byte value is present.
    assert!(
        dbg.contains("115"),
        "alpn bytes should include 115 ('s'): {dbg}"
    );
}

#[test]
fn builder_methods_chain() {
    // Just confirm the builder methods compose without taking
    // ownership in surprising ways.
    let _opts = ConnectOptions::new()
        .with_server_name("a.example.com")
        .with_alpn(&[b"smtp"]);
}

#[test]
fn options_are_clone_and_default() {
    // Both traits matter for ergonomic call-site use.
    let a = ConnectOptions::default();
    let b = a.clone();
    let _ = (a, b);
}
