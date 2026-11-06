// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Durable account recovery data. A receipt binds one original turn to one
//! destination; it is evidence to reconcile with the host, never authority to
//! execute a turn on its own.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Purpose {
    #[default]
    Login,
    WeeklyQuota,
}

impl Purpose {
    pub fn marker(self) -> &'static str {
        match self {
            Self::Login => "renewal",
            Self::WeeklyQuota => "rotation",
        }
    }

    pub fn rule(self) -> &'static str {
        match self {
            Self::Login => "login-back@v1",
            Self::WeeklyQuota => "usage-rotate@v1",
        }
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
            Purpose::WeeklyQuota => self
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
            (Purpose::WeeklyQuota, Some(target)) => {
                self.receipt.is_some()
                    && hex(&target.account, 64)
                    && hex(&target.user, 64)
                    && target.account != self.account
            }
            (Purpose::WeeklyQuota, None) => self.receipt.is_none(),
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
        bound.destination = match self.purpose {
            Purpose::Login => None,
            Purpose::WeeklyQuota => Some(Destination {
                account: account.into(),
                user: user.into(),
            }),
        };
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
