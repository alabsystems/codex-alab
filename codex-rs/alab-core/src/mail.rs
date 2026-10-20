// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A session's aterm mail, as codex-alab's pieces share it (CONTRACT §5 "inbox").
//!
//! A manager's `aterm drive task` posts `kind=task` and types one line into the worker,
//! `Inbox: task @<off>`. A sandboxed codex cannot read its inbox (the model's tools never get
//! aterm's socket), so codex-alab's TUI, which is never sandboxed, reads it for the turn: the
//! typed line stays the trigger (nothing is typed or woken by mail), the turn carries the
//! mail's words fenced as data, and at the turn's end the autopilot marks the row handled and
//! reports back to the sender. Only under a proven claim; anything else leaves the line as
//! typed. Plain data and pure readings: the aterm client does the I/O.

/// The kinds a typed trigger names (`Inbox: <kind> @<off>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriggerKind {
    Task,
    Ask,
}

impl TriggerKind {
    pub fn word(self) -> &'static str {
        match self {
            TriggerKind::Task => "task",
            TriggerKind::Ask => "ask",
        }
    }
}

/// The trigger a submitted turn is: exactly `Inbox: (task|ask) @(\d+)` (surrounding
/// whitespace aside), else `None`.
pub fn trigger(text: &str) -> Option<(TriggerKind, u64)> {
    let rest = text.trim().strip_prefix("Inbox: ")?;
    let (kind, off) = rest.split_once(' ')?;
    let kind = match kind {
        "task" => TriggerKind::Task,
        "ask" => TriggerKind::Ask,
        _ => return None,
    };
    let off = off.strip_prefix('@')?;
    if off.is_empty() || !off.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((kind, off.parse().ok()?))
}

/// One message read from this session's inbox: its row and the body this endpoint holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InboxMail {
    /// The inbox row id (`inbox seen <id>` names it).
    pub id: u64,
    /// The broker offset (`re=` of a reply names it).
    pub off: u64,
    /// The sender (a session `@<sid>`, a principal).
    pub from: String,
    /// `task`, `ask`, …
    pub kind: String,
    /// aterm's verdict on what the content is (`human`, `agent`, `relayed`, `screen`): the
    /// RECEIVER's, never the sender's claim.
    pub trust: String,
    /// The body, as this endpoint holds it.
    pub body: String,
    /// The true size when the delivering bridge cut the body (`truncated=1 len=<n>`).
    pub cut_from: Option<usize>,
    /// The advisory deadline (`dl=`), as sent.
    pub deadline: Option<String>,
}

impl InboxMail {
    /// Whether aterm vouches the words are a sender's own (a person, an agent session), so the
    /// turn may carry them at once; `relayed`, `screen` or anything else waits in the composer
    /// for a person to send.
    pub fn sendable(&self) -> bool {
        matches!(self.trust.as_str(), "human" | "agent")
    }
}

/// A turn the TUI submitted for a mail: matched by its full text when the turn starts, and
/// answered at its end.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MailTurn {
    pub id: u64,
    pub off: u64,
    pub from: String,
    pub kind: String,
    /// The turn's whole text (the mail ahead of the marker, then the typed line).
    pub text: String,
}

/// The receipt a mail turn's end gives (`inbox seen <id> <verdict>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// The turn ran to its end.
    Handled,
    /// It was interrupted or ended at an error.
    Deferred,
}

impl Verdict {
    pub fn word(self) -> &'static str {
        match self {
            Verdict::Handled => "handled",
            Verdict::Deferred => "deferred",
        }
    }
}

/// What the status line's mail badge counts: this session's rows above the handled
/// watermark (display only).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MailBadge {
    pub tasks: u32,
    pub asks: u32,
    /// Any other kind (notes, answers, reports, …), receipts and expiries aside.
    pub other: u32,
}

impl MailBadge {
    /// Count one row of `kind`.
    pub fn count(&mut self, kind: &str) {
        match kind {
            "task" => self.tasks += 1,
            "ask" => self.asks += 1,
            "ack" | "expired" => {}
            _ => self.other += 1,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.tasks == 0 && self.asks == 0 && self.other == 0
    }

    /// The badge's words: `✉ 1 task`, `✉ 2 tasks · 1 ask · 3 more`.
    pub fn text(&self) -> Option<String> {
        if self.is_empty() {
            return None;
        }
        let mut parts = Vec::new();
        for (n, one, many) in [(self.tasks, "task", "tasks"), (self.asks, "ask", "asks")] {
            if n > 0 {
                parts.push(format!("{n} {}", if n == 1 { one } else { many }));
            }
        }
        if self.other > 0 {
            parts.push(if parts.is_empty() {
                format!("{} mail", self.other)
            } else {
                format!("{} more", self.other)
            });
        }
        Some(format!("✉ {}", parts.join(" · ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn only_the_exact_line_is_a_trigger() {
        assert_eq!(trigger("Inbox: task @12"), Some((TriggerKind::Task, 12)));
        assert_eq!(trigger("  Inbox: ask @0\n"), Some((TriggerKind::Ask, 0)));
        for text in [
            "Inbox: task @",
            "Inbox: task 12",
            "Inbox: note @12",
            "Inbox: task @12 please",
            "inbox: task @12",
            "Inbox:  task @12",
            "Inbox: task @+12",
            "Inbox: task @12x",
            "please read Inbox: task @12",
            "Inbox: task @99999999999999999999999",
        ] {
            assert_eq!(trigger(text), None, "{text}");
        }
    }

    #[test]
    fn only_a_senders_own_words_are_sendable() {
        let mail = |trust: &str| InboxMail {
            id: 1,
            off: 2,
            from: "@s-a".to_string(),
            kind: "task".to_string(),
            trust: trust.to_string(),
            body: String::new(),
            cut_from: None,
            deadline: None,
        };
        assert!(mail("human").sendable());
        assert!(mail("agent").sendable());
        for trust in ["relayed", "screen", "", "Human", "unknown"] {
            assert!(!mail(trust).sendable(), "{trust}");
        }
    }

    #[test]
    fn the_badge_counts_what_waits() {
        let mut badge = MailBadge::default();
        assert_eq!(badge.text(), None);
        badge.count("task");
        assert_eq!(badge.text().as_deref(), Some("✉ 1 task"));
        badge.count("task");
        badge.count("ask");
        badge.count("note");
        badge.count("ack");
        badge.count("expired");
        assert_eq!(badge.text().as_deref(), Some("✉ 2 tasks · 1 ask · 1 more"));
        let mut notes = MailBadge::default();
        notes.count("note");
        notes.count("report");
        assert_eq!(notes.text().as_deref(), Some("✉ 2 mail"));
    }
}
