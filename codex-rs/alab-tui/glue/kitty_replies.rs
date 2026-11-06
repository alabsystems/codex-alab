// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! ALAB(pet-perf) glue, compiled inside codex-tui as `tui::event_stream::alab_kitty_replies`
//! (one `#[path]` line in `tui/src/tui/event_stream.rs`, where the unix `CrosstermEventSource`
//! reads through it): aterm's answer to a kitty graphics command, taken off the input.
//!
//! The ambient pet's placements ask aterm to answer a failure (`q=1`, `glue/pet_kitty.rs`):
//! `ESC _ G i=<id>,p=1;ENOENT:no such image ESC \` once aterm no longer holds a frame (its
//! restore after a seamless update carries no kitty images). crossterm has no parser for an
//! APC string and delivers one as keys: Alt+`_`, `G`, the reply's characters, Alt+`\`.
//! [`KittyReplies`] holds a candidate from Alt+`_` while it reads like a reply, drops a whole
//! reply (`codex_alab_tui::pets::kitty::reply_lost`, which has the frames sent again), and
//! replays anything else unchanged, as upstream's `windows_key_sequence.rs` does: a person's
//! Alt+`_` waits for the next key, and at most [`REPLY_TIMEOUT`]. Off (not the codex binary
//! inside aterm) every event passes as it came.

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;
use std::time::Duration;

use codex_alab_tui::pets::kitty;
use crossterm::event::Event;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use tokio::time::Sleep;
use tokio_stream::Stream;

use super::EventResult;

/// How long a candidate waits for the rest of a reply, which aterm writes in one piece.
const REPLY_TIMEOUT: Duration = Duration::from_millis(/*millis*/ 50);
/// The longest reply text held: aterm's are under 100 characters.
const MAX_REPLY: usize = 256;

pub(super) struct KittyReplies<S> {
    inner: S,
    on: bool,
    /// The candidate's keys, replayed unchanged when it is no reply.
    pending: Vec<KeyEvent>,
    /// The candidate's text after `G`.
    body: String,
    /// The candidate's last key was a bare Escape: the reply's `ESC \` split across reads.
    escape: bool,
    deadline: Option<Pin<Box<Sleep>>>,
    ready: VecDeque<EventResult>,
}

impl<S> KittyReplies<S> {
    pub(super) fn new(inner: S) -> Self {
        Self::with(kitty::on(), inner)
    }

    fn with(on: bool, inner: S) -> Self {
        Self {
            inner,
            on,
            pending: Vec::new(),
            body: String::new(),
            escape: false,
            deadline: None,
            ready: VecDeque::new(),
        }
    }

    fn flush(&mut self) {
        self.ready
            .extend(self.pending.drain(..).map(|key| Ok(Event::Key(key))));
        self.body.clear();
        self.escape = false;
        self.deadline = None;
    }

    fn accept(&mut self, event: Event) {
        if let Event::Key(key) = event
            && self.hold(key)
        {
            return;
        }
        let broke = !self.pending.is_empty();
        self.flush();
        // The key that broke a candidate may start the next one.
        if broke
            && let Event::Key(key) = event
            && self.hold(key)
        {
            return;
        }
        self.ready.push_back(Ok(event));
    }

    /// Whether `key` belongs to the candidate (a whole reply's last key drops it).
    fn hold(&mut self, key: KeyEvent) -> bool {
        if key.kind != KeyEventKind::Press {
            return false;
        }
        let plain = (key.modifiers - KeyModifiers::SHIFT).is_empty();
        match (self.pending.len(), key.code) {
            (0, KeyCode::Char('_')) if key.modifiers == KeyModifiers::ALT => {
                self.deadline = Some(Box::pin(tokio::time::sleep(REPLY_TIMEOUT)));
            }
            (0, _) => return false,
            (1, KeyCode::Char('G')) if plain => {}
            (1, _) => return false,
            (_, KeyCode::Char('\\'))
                if key.modifiers == KeyModifiers::ALT
                    || (self.escape && key.modifiers.is_empty()) =>
            {
                if kitty::reply_lost(&self.body).is_none() {
                    return false;
                }
                self.pending.clear();
                self.body.clear();
                self.escape = false;
                self.deadline = None;
                return true;
            }
            (_, KeyCode::Esc) if key.modifiers.is_empty() && !self.escape => {
                self.escape = true;
            }
            (_, KeyCode::Char(ch))
                if plain
                    && !self.escape
                    && (' '..='~').contains(&ch)
                    && self.body.len() < MAX_REPLY =>
            {
                self.body.push(ch);
            }
            _ => return false,
        }
        self.pending.push(key);
        true
    }
}

impl<S: Stream<Item = EventResult> + Unpin> Stream for KittyReplies<S> {
    type Item = EventResult;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        if !this.on {
            return Pin::new(&mut this.inner).poll_next(cx);
        }
        loop {
            if let Some(event) = this.ready.pop_front() {
                return Poll::Ready(Some(event));
            }
            match Pin::new(&mut this.inner).poll_next(cx) {
                Poll::Ready(Some(Ok(event))) => this.accept(event),
                Poll::Ready(Some(Err(error))) => {
                    this.flush();
                    this.ready.push_back(Err(error));
                }
                Poll::Ready(None) => {
                    this.flush();
                    return Poll::Ready(this.ready.pop_front());
                }
                Poll::Pending => {
                    if let Some(deadline) = &mut this.deadline
                        && deadline.as_mut().poll(cx).is_ready()
                    {
                        this.flush();
                        continue;
                    }
                    return Poll::Pending;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::FutureExt;
    use pretty_assertions::assert_eq;
    use tokio_stream::StreamExt;
    use tokio_stream::wrappers::UnboundedReceiverStream;

    /// `text` as crossterm's unix parser delivers it: `ESC x` is Alt+`x`, an uppercase letter
    /// carries Shift.
    fn keys(text: &str) -> Vec<Event> {
        let mut out = Vec::new();
        let mut chars = text.chars();
        while let Some(ch) = chars.next() {
            let key = match ch {
                '\u{1b}' => match chars.next() {
                    Some(next) => KeyEvent::new(KeyCode::Char(next), KeyModifiers::ALT),
                    None => KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
                },
                ch if ch.is_uppercase() => KeyEvent::new(KeyCode::Char(ch), KeyModifiers::SHIFT),
                ch => KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
            };
            out.push(Event::Key(key));
        }
        out
    }

    async fn read(on: bool, events: Vec<Event>) -> Vec<Event> {
        KittyReplies::with(on, tokio_stream::iter(events.into_iter().map(Ok)))
            .map(Result::unwrap)
            .collect()
            .await
    }

    fn enoent() -> String {
        let id = kitty::frame_id(5).unwrap();
        format!("\u{1b}_Gi={id},p=1;ENOENT:no such image\u{1b}\\")
    }

    #[tokio::test]
    async fn an_enoent_reply_is_dropped_and_has_the_frames_sent_again() {
        let before = kitty::epoch();
        let events = [keys("a"), keys(&enoent()), keys("b")].concat();
        assert_eq!(read(/*on*/ true, events).await, keys("ab"));
        assert!(kitty::epoch() > before);
    }

    #[tokio::test]
    async fn a_reply_whose_escape_came_alone_is_dropped_too() {
        let reply = enoent();
        let (body, _) = reply.split_at(reply.len() - 2);
        let mut events = keys(body);
        events.push(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
        events.extend(keys("\\"));
        assert_eq!(read(/*on*/ true, events).await, Vec::new());
    }

    #[tokio::test]
    async fn a_persons_keys_are_replayed_unchanged() {
        for text in [
            "\u{1b}_hi",
            "\u{1b}_\u{1b}_x",
            "\u{1b}_Gxy\u{1b}\\",
            "\u{1b}_G\u{1b}x",
            "\u{1b}_Gi=1;OK\r",
        ] {
            let events = keys(text);
            assert_eq!(read(/*on*/ true, events.clone()).await, events, "{text:?}");
        }
        let mut with_enter = keys("\u{1b}_Gi=");
        with_enter.push(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        )));
        assert_eq!(read(/*on*/ true, with_enter.clone()).await, with_enter);
        let mut with_paste = keys("\u{1b}_");
        with_paste.push(Event::Paste("x".to_string()));
        assert_eq!(read(/*on*/ true, with_paste.clone()).await, with_paste);
    }

    #[tokio::test]
    async fn off_every_key_passes() {
        let events = keys(&enoent());
        assert_eq!(read(/*on*/ false, events.clone()).await, events);
    }

    #[tokio::test(start_paused = true)]
    async fn a_lone_alt_underscore_waits_only_until_the_deadline() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let mut stream = KittyReplies::with(/*on*/ true, UnboundedReceiverStream::new(rx));
        let start = keys("\u{1b}_").remove(0);
        tx.send(Ok(start.clone())).unwrap();
        assert!(stream.next().now_or_never().is_none());
        tokio::time::advance(REPLY_TIMEOUT).await;
        assert_eq!(
            stream.next().now_or_never().unwrap().unwrap().unwrap(),
            start
        );
    }
}
