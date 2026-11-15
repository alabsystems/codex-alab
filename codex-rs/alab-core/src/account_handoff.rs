// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Durable account recovery data. A receipt binds one original turn to one
//! destination; it is evidence to reconcile with the host, never authority to
//! execute a turn on its own.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Purpose {
    /// The same account's login renewed in place (`codex-renewal`).
    #[default]
    Login,
    /// Another account at a usage wall (`codex-rotation`).
    WeeklyQuota,
    /// Another already-signed-in account at a lost login whose token simply ended
    /// (`codex-login-rotation`; `alab/design/accounts-lost-login.md`).
    LoginRotation,
}

impl Purpose {
    /// The record field the handoff is kept in: a move to another account is a `rotation`,
    /// told apart from the others there by its [`Self::rule`].
    pub fn marker(self) -> &'static str {
        match self {
            Self::Login => "renewal",
            Self::WeeklyQuota | Self::LoginRotation => "rotation",
        }
    }

    pub fn rule(self) -> &'static str {
        match self {
            Self::Login => "login-back@v1",
            Self::WeeklyQuota => "usage-rotate@v1",
            Self::LoginRotation => "login-rotate@v1",
        }
    }

    /// The purpose a record's `rotation` field holds under `rule`; `None` for any rule but a
    /// move's, which drops the act.
    pub fn of_rotation_rule(rule: &str) -> Option<Purpose> {
        [Self::WeeklyQuota, Self::LoginRotation]
            .into_iter()
            .find(|purpose| purpose.rule() == rule)
    }

    /// Whether the handoff moves the session to another account: its destination is known
    /// only from a host receipt, and is never the source.
    pub fn moves_account(self) -> bool {
        matches!(self, Self::WeeklyQuota | Self::LoginRotation)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Destination {
    pub account: String,
    pub user: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountHandoff {
    pub purpose: Purpose,
    /// Evidence of the original foreground OpenAI subscription request.
    pub openai_scope: bool,
    /// A same-file login recovery has no native restart receipt.
    pub local: bool,
    /// Domain-separated SHA256 identity proofs, never raw account identifiers.
    pub account: String,
    pub user: String,
    pub destination: Option<Destination>,
    pub receipt: Option<String>,
    /// The original turn, reserved before its continuation is admitted.
    pub turn: Option<String>,
    pub accepted: bool,
    /// Original turn starts before this handoff was prepared.
    pub starts: u64,
    pub goal_id: Option<String>,
}

fn hex(word: &str, len: usize) -> bool {
    word.len() == len
        && word
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

impl AccountHandoff {
    pub fn source(&self) -> (&str, &str) {
        (&self.account, &self.user)
    }

    /// The destination becomes known only after native account selection.
    pub fn target(&self) -> Option<(&str, &str)> {
        match self.purpose {
            Purpose::Login if self.destination.is_none() => Some(self.source()),
            Purpose::WeeklyQuota | Purpose::LoginRotation => self
                .destination
                .as_ref()
                .map(|target| (target.account.as_str(), target.user.as_str())),
            Purpose::Login => None,
        }
    }

    pub fn valid_for_origin(&self, origin: &str) -> bool {
        if origin.is_empty()
            || origin.len() > 512
            || origin.chars().any(char::is_control)
            || !self.openai_scope
            || self.starts == 0
            || !hex(&self.account, 64)
            || !hex(&self.user, 64)
            || self.receipt.as_ref().is_some_and(|id| !hex(id, 32))
            || self.turn.as_deref().is_some_and(|turn| turn != origin)
            || (self.accepted && self.turn.is_none())
            || (self.local && (self.purpose != Purpose::Login || self.receipt.is_some()))
            || (self.accepted && !self.local && self.receipt.is_none())
        {
            return false;
        }
        match (&self.purpose, &self.destination) {
            (Purpose::Login, None) => true,
            (Purpose::WeeklyQuota | Purpose::LoginRotation, Some(target)) => {
                self.receipt.is_some()
                    && hex(&target.account, 64)
                    && hex(&target.user, 64)
                    && target.account != self.account
            }
            (Purpose::WeeklyQuota | Purpose::LoginRotation, None) => self.receipt.is_none(),
            (Purpose::Login, Some(_)) => false,
        }
    }

    /// Bind a host response without allowing a receipt or identity substitution.
    /// Both the source and its original native turn are checked by the caller.
    pub fn bind(&mut self, id: &str, account: &str, user: &str, origin: &str) -> bool {
        if !self.valid_for_origin(origin)
            || self.local
            || self.receipt.as_deref().is_some_and(|known| known != id)
            || self.target().is_some_and(|known| known != (account, user))
        {
            return false;
        }
        let mut bound = self.clone();
        bound.receipt = Some(id.into());
        bound.destination = self.purpose.moves_account().then(|| Destination {
            account: account.into(),
            user: user.into(),
        });
        if !bound.valid_for_origin(origin) {
            return false;
        }
        *self = bound;
        true
    }

    /// A host-retired receipt releases only this selection, not the owed task.
    pub fn retire_selection(&mut self) {
        self.receipt = None;
        self.destination = None;
        self.turn = None;
        self.accepted = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handoff(purpose: Purpose) -> AccountHandoff {
        AccountHandoff {
            purpose,
            openai_scope: true,
            local: false,
            account: "a".repeat(64),
            user: "b".repeat(64),
            destination: None,
            receipt: None,
            turn: None,
            accepted: false,
            starts: 1,
            goal_id: Some("original-goal".into()),
        }
    }

    #[test]
    fn quota_receipt_keeps_both_identities_and_rejects_substitution() {
        let mut value = handoff(Purpose::WeeklyQuota);
        let id = "c".repeat(32);
        let target = "d".repeat(64);
        let user = "e".repeat(64);
        assert!(value.valid_for_origin("original-turn"));
        assert_eq!(value.target(), None);
        assert!(!value.bind(&id, &value.account.clone(), &user, "original-turn"));
        assert!(value.bind(&id, &target, &user, "original-turn"));
        assert!(value.bind(&id, &target, &user, "original-turn"));
        let before = value.clone();
        assert!(!value.bind(&"f".repeat(32), &target, &user, "original-turn"));
        assert!(!value.bind(&id, &"f".repeat(64), &user, "original-turn"));
        assert_eq!(value, before);
        value.retire_selection();
        assert_eq!(value, handoff(Purpose::WeeklyQuota));
    }

    #[test]
    fn login_receipt_cannot_change_the_account_or_user() {
        let mut value = handoff(Purpose::Login);
        let id = "c".repeat(32);
        let (account, user) = (value.account.clone(), value.user.clone());
        assert!(!value.bind(&id, &"d".repeat(64), &user, "original-turn"));
        assert!(!value.bind(&id, &account, &"d".repeat(64), "original-turn"));
        assert!(value.bind(&id, &account, &user, "original-turn"));
        value.destination = Some(Destination { account, user });
        assert!(!value.valid_for_origin("original-turn"));
    }

    /// A LOST LOGIN'S MOVE IS A MOVE, NOT A RENEWAL: its destination comes only with a receipt,
    /// is never the source account and never local, and binding cannot substitute the receipt
    /// or the destination. (Its origin turn is the owed act's, which the receipt is matched
    /// against: `account_recovery::Receipt::matches_handoff`.)
    #[test]
    fn a_login_rotation_binds_only_another_accounts_receipt() {
        assert_eq!(Purpose::LoginRotation.marker(), "rotation");
        assert_eq!(Purpose::LoginRotation.rule(), "login-rotate@v1");
        assert!(Purpose::LoginRotation.moves_account() && Purpose::WeeklyQuota.moves_account());
        assert!(!Purpose::Login.moves_account());
        let id = "c".repeat(32);
        let (target, user) = ("d".repeat(64), "e".repeat(64));
        let mut value = handoff(Purpose::LoginRotation);
        assert!(
            value.valid_for_origin("original-turn"),
            "unbound, no receipt"
        );
        assert_eq!(
            value.target(),
            None,
            "no destination before the host selects one"
        );
        assert!(
            !value.bind(&id, &value.account.clone(), &user, "original-turn"),
            "never the source account"
        );
        assert!(!value.bind(&id, "not-hex", &user, "original-turn"));
        assert!(value.bind(&id, &target, &user, "original-turn"));
        assert_eq!(value.target(), Some((target.as_str(), user.as_str())));
        assert!(value.valid_for_origin("original-turn"));
        let before = value.clone();
        assert!(!value.bind(&"f".repeat(32), &target, &user, "original-turn"));
        assert!(!value.bind(&id, &"f".repeat(64), &user, "original-turn"));
        assert_eq!(value, before);
        // A destination without a receipt, a receipt-less destination after a retire, and a
        // local move are refused.
        let mut destination_only = handoff(Purpose::LoginRotation);
        destination_only.destination = before.destination.clone();
        assert!(!destination_only.valid_for_origin("original-turn"));
        let mut local = handoff(Purpose::LoginRotation);
        local.local = true;
        assert!(!local.valid_for_origin("original-turn"));
        assert!(!local.bind(&id, &target, &user, "original-turn"));
        let mut bound_without_target = before;
        bound_without_target.destination = None;
        assert!(
            !bound_without_target.valid_for_origin("original-turn"),
            "a receipt with no destination"
        );
        value.retire_selection();
        assert_eq!(value, handoff(Purpose::LoginRotation));
    }

    #[test]
    fn a_rotation_rule_names_its_purpose_and_no_other_rule_does() {
        assert_eq!(
            Purpose::of_rotation_rule("usage-rotate@v1"),
            Some(Purpose::WeeklyQuota)
        );
        assert_eq!(
            Purpose::of_rotation_rule("login-rotate@v1"),
            Some(Purpose::LoginRotation)
        );
        for rule in ["login-back@v1", "usage-resume@v1", "login-rotate@v2", ""] {
            assert_eq!(Purpose::of_rotation_rule(rule), None, "{rule:?}");
        }
    }

    #[test]
    fn admission_stays_on_the_original_turn_and_requires_receipt_evidence() {
        let mut value = handoff(Purpose::WeeklyQuota);
        value.turn = Some("original-turn".into());
        value.accepted = true;
        assert!(!value.valid_for_origin("original-turn"));
        assert!(!value.bind(
            &"c".repeat(32),
            &"d".repeat(64),
            &"e".repeat(64),
            "original-turn"
        ));
        value.accepted = false;
        assert!(value.bind(
            &"c".repeat(32),
            &"d".repeat(64),
            &"e".repeat(64),
            "original-turn"
        ));
        value.accepted = true;
        assert!(value.valid_for_origin("original-turn"));
        assert!(!value.valid_for_origin(""));
        assert!(!value.valid_for_origin("another-turn"));
        value.local = true;
        assert!(!value.valid_for_origin("original-turn"));
    }
}
