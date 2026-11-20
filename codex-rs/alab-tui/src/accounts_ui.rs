// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! One account pane's text and ownership of its latest requested snapshot.

use std::sync::Arc;
use std::sync::Weak;

use crate::interface::ConnectionState;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Destination {
    pub label: String,
    pub masked_email: Option<String>,
    pub id: String,
    pub revision: String,
    pub selection_ref: String,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub text: String,
    /// Retained observations to display once this roster's actions are no longer current.
    pub cached_text: Option<String>,
    pub shared_mask: bool,
    /// Present only while the displayed ambiguous roster can open its native address sheet.
    pub address_revision: Option<Arc<str>>,
    pub destinations: Vec<Destination>,
}

impl Snapshot {
    fn withdraw_actions(&mut self) {
        self.destinations.clear();
        self.address_revision = None;
        if let Some(cached) = self.cached_text.take() {
            self.text = cached;
        }
    }
}

#[derive(Debug)]
pub enum PickerAction {
    Open,
    ShowAddresses(Weak<str>),
    /// Only the corresponding picker row keeps this destination alive.
    Select(Weak<Destination>),
}

#[derive(Clone, Debug)]
pub struct SnapshotToken(Arc<()>);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AddressResult {
    Opened,
    Unsupported,
    Stale,
    Unavailable,
}

#[derive(Clone, Debug)]
pub enum Update {
    Snapshot {
        token: SnapshotToken,
        result: Result<Snapshot, String>,
    },
    Addresses {
        revision: Weak<str>,
        result: AddressResult,
    },
    /// What an account move says while it is under way: shown only in its own open pane.
    Observation(String),
    /// An account move's last word, one of its fixed sentences, which name no account: shown in
    /// its own open pane as an observation, or, where that pane is gone (closed, replaced, or
    /// another thread shown), said once in history, so a move that did not happen is never
    /// left unsaid.
    Outcome(String),
}

#[derive(Debug)]
pub struct View {
    notice: Option<String>,
    snapshot: Option<Snapshot>,
    current: Option<SnapshotToken>,
    opening_addresses: bool,
}

impl View {
    pub fn new(text: String) -> Self {
        Self {
            notice: Some(text),
            snapshot: None,
            current: None,
            opening_addresses: false,
        }
    }

    pub fn display_text(&self, connected_email: Option<&str>) -> String {
        let mut sections = Vec::new();
        if self.current.is_some() && self.snapshot.is_some() {
            sections.push("Refreshing subscription accounts…");
        } else if self.opening_addresses {
            sections.push("Opening account addresses in aterm…");
        } else if let Some(notice) = &self.notice {
            sections.push(notice.as_str());
        }
        let identity = connected_email.map(|email| format!("Connected server account: {email}"));
        if let Some(identity) = &identity {
            sections.push(identity.as_str());
        } else if self
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.shared_mask)
        {
            sections.push(match crate::interface::connection_state() {
                ConnectionState::Connected => "Use /status for this session's account details.",
                ConnectionState::Reconnecting => {
                    "Reconnecting to the app server. The connected email is unavailable."
                }
                ConnectionState::Offline => {
                    "The app server is offline. The connected email is unavailable."
                }
            });
        }
        if let Some(snapshot) = &self.snapshot {
            sections.push(&snapshot.text);
        }
        sections.join("\n\n")
    }

    pub fn destinations(&self) -> &[Destination] {
        self.snapshot
            .as_ref()
            .map_or(&[], |snapshot| snapshot.destinations.as_slice())
    }

    pub fn address_action(&self) -> Option<Weak<str>> {
        if self.opening_addresses {
            return None;
        }
        self.snapshot
            .as_ref()?
            .address_revision
            .as_ref()
            .map(Arc::downgrade)
    }

    pub fn begin_addresses(&mut self, requested: &Weak<str>) -> Option<Arc<str>> {
        if !self.address_action()?.ptr_eq(requested) {
            return None;
        }
        self.opening_addresses = true;
        requested.upgrade()
    }

    pub fn awaiting_addresses(&self, requested: &Weak<str>) -> bool {
        self.opening_addresses
            && self
                .snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.address_revision.as_ref())
                .is_some_and(|revision| requested.ptr_eq(&Arc::downgrade(revision)))
    }

    /// Reserve before spawning so scheduling cannot reorder refresh ownership.
    pub fn begin_snapshot(&mut self) -> SnapshotToken {
        let token = SnapshotToken(Arc::new(()));
        self.current = Some(token.clone());
        self.opening_addresses = false;
        if let Some(snapshot) = &mut self.snapshot {
            snapshot.withdraw_actions();
        }
        token
    }

    /// Accept a snapshot once, or let an account move's observation supersede pending reads.
    pub fn apply(&mut self, update: Update) -> bool {
        match update {
            Update::Snapshot { token, result } => {
                if !self
                    .current
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(&current.0, &token.0))
                {
                    return false;
                }
                self.current = None;
                match result {
                    Ok(snapshot) => {
                        self.snapshot = Some(snapshot);
                        self.notice = None;
                    }
                    Err(error) => {
                        self.notice = Some(if self.snapshot.is_some() {
                            format!(
                                "{error}\n\nShowing previously loaded account information; it may be out of date."
                            )
                        } else {
                            error
                        });
                    }
                }
            }
            Update::Addresses { revision, result } => {
                if !self.awaiting_addresses(&revision) {
                    return false;
                }
                self.opening_addresses = false;
                let notice = match result {
                    AddressResult::Opened => "Account addresses opened in aterm.",
                    AddressResult::Unsupported => {
                        "This aterm version cannot show account addresses. Use the slot-label lookup below."
                    }
                    AddressResult::Stale => {
                        if let Some(snapshot) = &mut self.snapshot {
                            snapshot.withdraw_actions();
                        }
                        "The account list changed. Refresh the accounts pane for current addresses and switch choices."
                    }
                    AddressResult::Unavailable => {
                        "aterm could not open the account addresses. Refresh the accounts pane and try again."
                    }
                };
                self.notice = Some(notice.to_owned());
            }
            Update::Observation(text) | Update::Outcome(text) => {
                self.opening_addresses = false;
                self.current = None;
                self.snapshot = None;
                self.notice = Some(text);
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ambiguous_roster() -> Snapshot {
        Snapshot {
            text: "two accounts".into(),
            cached_text: None,
            shared_mask: true,
            address_revision: Some(Arc::from("revision")),
            destinations: Vec::new(),
        }
    }

    #[test]
    fn a_stale_native_refusal_withdraws_roster_actions_until_refresh() {
        let mut view = View::new(String::new());
        let mut roster = ambiguous_roster();
        roster.cached_text = Some(roster.text.clone());
        roster
            .text
            .push_str("\n/accounts use spare\n/accounts auto");
        roster.destinations.push(Destination {
            label: "spare".into(),
            masked_email: None,
            id: "id".into(),
            selection_ref: "d".repeat(64),
            revision: "revision".into(),
        });
        let token = view.begin_snapshot();
        assert!(view.apply(Update::Snapshot {
            token,
            result: Ok(roster.clone())
        }));
        let key = view.address_action().unwrap();
        assert!(view.begin_addresses(&key).is_some());
        assert!(view.apply(Update::Addresses {
            revision: key,
            result: AddressResult::Stale
        }));
        assert!(view.address_action().is_none());
        assert!(view.destinations().is_empty());
        let shown = view.display_text(None);
        assert!(shown.contains("two accounts"));
        assert!(!shown.contains("/accounts use") && !shown.contains("/accounts auto"));
        assert!(
            shown.contains("account list changed") && shown.contains("Refresh the accounts pane")
        );
        let token = view.begin_snapshot();
        assert!(view.apply(Update::Snapshot {
            token,
            result: Ok(roster)
        }));
        assert!(view.address_action().is_some());
        assert_eq!(view.destinations()[0].label, "spare");
        assert!(view.display_text(None).contains("/accounts use spare"));
    }

    #[test]
    fn refresh_revokes_address_keys_and_late_replies_even_when_the_revision_is_unchanged() {
        let mut view = View::new(String::new());
        let token = view.begin_snapshot();
        assert!(view.address_action().is_none());
        assert!(view.apply(Update::Snapshot {
            token,
            result: Ok(ambiguous_roster())
        }));
        let key = view.address_action().unwrap();
        let revision = view.begin_addresses(&key).unwrap();
        assert_eq!(&*revision, "revision");
        assert!(view.begin_addresses(&key).is_none());
        assert!(view.awaiting_addresses(&key));
        let late = Update::Addresses {
            revision: key.clone(),
            result: AddressResult::Unavailable,
        };
        let token = view.begin_snapshot();
        assert!(!view.awaiting_addresses(&key));
        assert!(!view.apply(late.clone()));
        assert!(view.apply(Update::Snapshot {
            token,
            result: Err("refresh failed".into())
        }));
        assert!(view.address_action().is_none());
        assert!(view.begin_addresses(&key).is_none());
        let token = view.begin_snapshot();
        assert!(view.apply(Update::Snapshot {
            token,
            result: Ok(ambiguous_roster())
        }));
        assert!(view.begin_addresses(&key).is_none());
        let key = view.address_action().unwrap();
        assert_eq!(view.begin_addresses(&key).as_deref(), Some("revision"));
        assert!(!view.apply(late));
        assert!(view.apply(Update::Addresses {
            revision: key.clone(),
            result: AddressResult::Opened
        }));
        assert!(view.display_text(Some("connected")).contains("opened"));
        assert!(view.address_action().is_some());
        assert!(view.begin_addresses(&key).is_some());
        assert!(view.apply(Update::Observation("moving".into())));
        assert!(view.address_action().is_none());
        assert!(!view.awaiting_addresses(&key));
    }
}
