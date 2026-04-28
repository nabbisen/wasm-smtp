//! Tests for the [`crate::session::SessionState`] state machine.

use crate::session::SessionState::{
    Authentication, Closed, Data, Ehlo, Greeting, MailFrom, Quit, RcptTo, StartTls,
};

#[test]
fn forward_progression_is_allowed() {
    assert!(Greeting.can_transition_to(Ehlo));
    assert!(Ehlo.can_transition_to(Authentication));
    assert!(Authentication.can_transition_to(MailFrom));
    assert!(MailFrom.can_transition_to(RcptTo));
    assert!(RcptTo.can_transition_to(Data));
    assert!(Data.can_transition_to(MailFrom));
}

#[test]
fn skipping_authentication_is_allowed() {
    // Unauthenticated submission goes Ehlo -> MailFrom directly.
    assert!(Ehlo.can_transition_to(MailFrom));
}

#[test]
fn starting_a_second_transaction_is_allowed() {
    // After one successful transaction the state is MailFrom; it
    // must be possible to begin another transaction.
    assert!(MailFrom.can_transition_to(MailFrom));
}

#[test]
fn multiple_recipients_stay_in_rcptto() {
    assert!(RcptTo.can_transition_to(RcptTo));
}

#[test]
fn quit_is_allowed_from_every_active_state() {
    for from in [Greeting, Ehlo, Authentication, MailFrom, RcptTo, Data] {
        assert!(from.can_transition_to(Quit), "{from:?} should allow QUIT");
    }
}

#[test]
fn closed_is_reachable_from_every_state() {
    for from in [
        Greeting,
        Ehlo,
        Authentication,
        StartTls,
        MailFrom,
        RcptTo,
        Data,
        Quit,
        Closed,
    ] {
        assert!(from.can_transition_to(Closed), "{from:?} -> Closed");
    }
}

#[test]
fn invalid_transitions_are_rejected() {
    assert!(!Greeting.can_transition_to(Authentication));
    assert!(!Greeting.can_transition_to(MailFrom));
    assert!(!Ehlo.can_transition_to(RcptTo));
    assert!(!Ehlo.can_transition_to(Data));
    assert!(!MailFrom.can_transition_to(Data));
    assert!(!MailFrom.can_transition_to(Authentication));
    assert!(!Data.can_transition_to(RcptTo));
    // Once Closed, the only transition is to Closed itself.
    assert!(!Closed.can_transition_to(Ehlo));
    assert!(!Closed.can_transition_to(MailFrom));
}

#[test]
fn closed_is_the_only_terminal_state() {
    assert!(Closed.is_terminal());
    for s in [
        Greeting,
        Ehlo,
        Authentication,
        StartTls,
        MailFrom,
        RcptTo,
        Data,
        Quit,
    ] {
        assert!(!s.is_terminal(), "{s:?} should not be terminal");
    }
}

// -- STARTTLS transitions (Phase 5) -----------------------------------

#[test]
fn starttls_is_reachable_from_authentication_only() {
    // The caller may upgrade only after EHLO completed.
    assert!(Authentication.can_transition_to(StartTls));
    // Other states must not jump straight into StartTls.
    for from in [Greeting, Ehlo, MailFrom, RcptTo, Data, Quit, Closed] {
        assert!(
            !from.can_transition_to(StartTls),
            "{from:?} should not be able to enter StartTls"
        );
    }
}

#[test]
fn starttls_returns_to_ehlo_after_upgrade() {
    // RFC 3207 §4.2: the client must re-issue EHLO on the secure
    // channel. The state machine models this by passing through
    // Ehlo on the way back.
    assert!(StartTls.can_transition_to(Ehlo));
    // From Ehlo we can resume the normal flow.
    assert!(Ehlo.can_transition_to(Authentication));
}

#[test]
fn starttls_cannot_skip_to_later_states() {
    // After upgrading we must still re-EHLO before talking auth or
    // MAIL FROM. Skipping Ehlo would mean the new (post-TLS)
    // capabilities are unknown.
    for to in [Authentication, MailFrom, RcptTo, Data, Quit] {
        assert!(
            !StartTls.can_transition_to(to),
            "StartTls should not skip directly to {to:?}"
        );
    }
}
