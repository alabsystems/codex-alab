// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0 AND MIT
// Sequence framing/quarantine derives from crossterm ed1cdab335221515706178d68495bba2aed1924f
// src/event/source/unix/mio.rs; Copyright (c) 2019 Timon, ../LICENSE.crossterm.
use crate::InternalEvent;
use crate::invalid;
use crate::parse::parse_event;
use codex_alab_core::goal_clock;
use crossterm::event::Event;
use crossterm::event::InputDiscardStatus;
use serde::Deserialize;
use serde::Serialize;
use std::collections::VecDeque;
use std::io;
use std::time::Duration;
const BRACKETED_PASTE_START: &[u8] = b"\x1b[200~";
const BRACKETED_PASTE_END: &[u8] = b"\x1b[201~";
const ESCAPE_TIMEOUT: Duration = Duration::from_millis(20);
const MAX_CHECKPOINT: usize = 64 * 1024 * 1024;
// Stop draining the terminal while completed events await their consumer.
// An in-progress sequence must still be read to completion.
const READY_HIGH_WATER: usize = 4096;

#[derive(Default)]
pub struct Parser {
    buffer: Vec<u8>,
    ready: VecDeque<Ready>,
    escape: Option<goal_clock::Stamp>,
    discarded_sequence: Option<DiscardedSequence>,
}
struct Ready {
    event: InternalEvent,
    wire: Frame,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
enum Frame {
    Input { bytes: Vec<u8>, raw_mode: bool },
    Resize { columns: u16, rows: u16 },
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    version: u32,
    buffer: Vec<u8>,
    ready: Vec<Frame>,
    #[serde(deserialize_with = "required")]
    escape: Option<(u8, u64, u32)>,
    #[serde(deserialize_with = "required")]
    discarded_sequence: Option<DiscardedSequence>,
}
#[derive(Serialize)]
struct SnapshotRef<'a> {
    version: u32,
    buffer: &'a [u8],
    ready: Vec<&'a Frame>,
    escape: Option<(u8, u64, u32)>,
    discarded_sequence: Option<DiscardedSequence>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
enum DiscardedSequence {
    PasteStart(usize),
    PasteBody(usize),
    OscBody,
    OscEscape,
    Csi,
    X10Mouse(usize),
    Ss3,
}
fn required<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(d)
}
impl Parser {
    pub fn discard_buffered_input(&mut self) -> InputDiscardStatus {
        if self.discarded_sequence.is_none() {
            if self.buffer.starts_with(BRACKETED_PASTE_START) {
                let matched = (1..BRACKETED_PASTE_END.len())
                    .rev()
                    .find(|&matched| self.buffer.ends_with(&BRACKETED_PASTE_END[..matched]))
                    .unwrap_or(0);
                self.discarded_sequence = Some(DiscardedSequence::PasteBody(matched));
            } else if !self.buffer.is_empty() && BRACKETED_PASTE_START.starts_with(&self.buffer) {
                self.discarded_sequence = Some(DiscardedSequence::PasteStart(self.buffer.len()));
            } else if self.buffer.starts_with(b"\x1b]") {
                self.discarded_sequence = Some(if self.buffer.ends_with(b"\x1b") {
                    DiscardedSequence::OscEscape
                } else {
                    DiscardedSequence::OscBody
                });
            } else if self.buffer.starts_with(b"\x1b[M") {
                self.discarded_sequence = Some(DiscardedSequence::X10Mouse(6 - self.buffer.len()));
            } else if self.buffer.starts_with(b"\x1b[") {
                self.discarded_sequence = Some(DiscardedSequence::Csi);
            } else if self.buffer.starts_with(b"\x1bO") {
                self.discarded_sequence = Some(DiscardedSequence::Ss3);
            }
        }
        self.buffer.clear();
        self.ready.clear();
        self.escape = None;
        match self.discarded_sequence {
            Some(DiscardedSequence::PasteStart(_) | DiscardedSequence::PasteBody(_)) => {
                InputDiscardStatus::BracketedPasteInProgress
            }
            Some(_) => InputDiscardStatus::ControlSequenceInProgress,
            None => InputDiscardStatus::Complete,
        }
    }

    pub fn feed(&mut self, buffer: &[u8], raw_mode: bool) -> io::Result<()> {
        if buffer.is_empty() {
            return Ok(());
        }
        self.escape = None;
        for byte in buffer {
            if let Some(discarded_sequence) = self.discarded_sequence {
                self.discarded_sequence = match discarded_sequence {
                    DiscardedSequence::PasteStart(matched)
                        if *byte == BRACKETED_PASTE_START[matched] =>
                    {
                        if matched + 1 == BRACKETED_PASTE_START.len() {
                            Some(DiscardedSequence::PasteBody(0))
                        } else {
                            Some(DiscardedSequence::PasteStart(matched + 1))
                        }
                    }
                    DiscardedSequence::PasteStart(_) if *byte == BRACKETED_PASTE_START[0] => {
                        Some(DiscardedSequence::PasteStart(1))
                    }
                    DiscardedSequence::PasteStart(1) if *byte == b']' => {
                        Some(DiscardedSequence::OscBody)
                    }
                    DiscardedSequence::PasteStart(1) if *byte == b'O' => {
                        Some(DiscardedSequence::Ss3)
                    }
                    DiscardedSequence::PasteStart(2) if *byte == b'M' => {
                        Some(DiscardedSequence::X10Mouse(3))
                    }
                    DiscardedSequence::PasteStart(2) if *byte == b'[' => {
                        Some(DiscardedSequence::Csi)
                    }
                    DiscardedSequence::PasteStart(matched)
                        if matched >= 2 && !(0x40..=0x7e).contains(byte) =>
                    {
                        Some(DiscardedSequence::Csi)
                    }
                    DiscardedSequence::PasteStart(_) => None,
                    DiscardedSequence::PasteBody(matched)
                        if *byte == BRACKETED_PASTE_END[matched] =>
                    {
                        if matched + 1 == BRACKETED_PASTE_END.len() {
                            None
                        } else {
                            Some(DiscardedSequence::PasteBody(matched + 1))
                        }
                    }
                    DiscardedSequence::PasteBody(_) if *byte == BRACKETED_PASTE_END[0] => {
                        Some(DiscardedSequence::PasteBody(1))
                    }
                    DiscardedSequence::PasteBody(_) => Some(DiscardedSequence::PasteBody(0)),
                    DiscardedSequence::OscBody | DiscardedSequence::OscEscape
                        if *byte == b'\x07' =>
                    {
                        None
                    }
                    DiscardedSequence::OscEscape if *byte == b'\\' => None,
                    DiscardedSequence::OscBody | DiscardedSequence::OscEscape
                        if *byte == b'\x1b' =>
                    {
                        Some(DiscardedSequence::OscEscape)
                    }
                    DiscardedSequence::OscBody | DiscardedSequence::OscEscape => {
                        Some(DiscardedSequence::OscBody)
                    }
                    DiscardedSequence::Csi | DiscardedSequence::Ss3 if *byte == b'\x1b' => {
                        Some(DiscardedSequence::PasteStart(1))
                    }
                    DiscardedSequence::Csi | DiscardedSequence::Ss3
                        if (0x40..=0x7e).contains(byte) =>
                    {
                        None
                    }
                    DiscardedSequence::Csi => Some(DiscardedSequence::Csi),
                    DiscardedSequence::X10Mouse(remaining) if remaining > 1 => {
                        Some(DiscardedSequence::X10Mouse(remaining - 1))
                    }
                    DiscardedSequence::X10Mouse(_) => None,
                    DiscardedSequence::Ss3 => Some(DiscardedSequence::Ss3),
                };
                continue;
            }
            let more = true;

            self.buffer.push(*byte);

            match parse_event(&self.buffer, more, raw_mode) {
                Ok(Some(ie)) => {
                    self.ready.push_back(Ready {
                        event: ie,
                        wire: Frame::Input {
                            bytes: self.buffer.clone(),
                            raw_mode,
                        },
                    });
                    self.buffer.clear();
                }
                Ok(None) => {
                    let completed_osc = self.buffer.starts_with(b"\x1b]")
                        && (*byte == b'\x07'
                            || (*byte == b'\\'
                                && self.buffer.get(self.buffer.len().saturating_sub(2))
                                    == Some(&b'\x1b')));
                    let completed_csi = self.buffer.len() > 2
                        && self.buffer.starts_with(b"\x1b[")
                        && (0x40..=0x7e).contains(byte)
                        && !self.buffer.starts_with(BRACKETED_PASTE_START)
                        && !self.buffer.starts_with(b"\x1b[[")
                        && !self.buffer.starts_with(b"\x1b[M");
                    if completed_osc || completed_csi {
                        self.buffer.clear();
                    }
                }
                Err(_) => {
                    // Event can't be parsed (not enough parameters, parameter is not a number, ...).
                    // Clear the buffer and continue with another sequence.
                    self.buffer.clear();
                }
            }
        }
        if self.buffer.as_slice() == b"\x1b" {
            self.escape =
                Some(goal_clock::checkpoint(&goal_clock::Instant::now()).map_err(crate::invalid)?);
        }
        Ok(())
    }
    pub fn pop(&mut self) -> Option<InternalEvent> {
        self.ready.pop_front().map(|ready| ready.event)
    }
    pub fn pop_where(&mut self, wanted: impl Fn(&InternalEvent) -> bool) -> Option<InternalEvent> {
        let index = self.ready.iter().position(|ready| wanted(&ready.event))?;
        self.ready.remove(index).map(|ready| ready.event)
    }
    pub fn has_where(&self, wanted: impl Fn(&InternalEvent) -> bool) -> bool {
        self.ready.iter().any(|ready| wanted(&ready.event))
    }
    pub fn has_events(&self) -> bool {
        !self.ready.is_empty()
    }
    pub(crate) fn input_backpressured(&self) -> bool {
        self.ready.len() >= READY_HIGH_WATER
    }
    pub fn resize(&mut self, columns: u16, rows: u16) {
        // A resize storm must not grow the queue while terminal reads are held.
        // Preserve ordering across keys, but only the latest consecutive size.
        if self
            .ready
            .back()
            .is_some_and(|ready| matches!(ready.wire, Frame::Resize { .. }))
        {
            self.ready.pop_back();
        }
        self.ready.push_back(Ready {
            event: InternalEvent::Event(Event::Resize(columns, rows)),
            wire: Frame::Resize { columns, rows },
        });
    }
    pub(crate) fn escape_remaining(&self) -> io::Result<Option<Duration>> {
        let Some(stamp) = self.escape else {
            return Ok(None);
        };
        let mut baseline = goal_clock::Instant::now();
        goal_clock::adopt(&mut baseline, stamp).map_err(invalid)?;
        Ok(Some(ESCAPE_TIMEOUT.saturating_sub(baseline.elapsed())))
    }
    pub fn flush_escape(&mut self) -> io::Result<()> {
        let Some(stamp) = self.escape else {
            return Ok(());
        };
        let mut baseline = goal_clock::Instant::now();
        goal_clock::adopt(&mut baseline, stamp).map_err(invalid)?;
        if baseline.elapsed() < ESCAPE_TIMEOUT {
            return Ok(());
        }
        if self.buffer.as_slice() != b"\x1b" {
            return Err(invalid("escape clock without a pending Escape"));
        }
        let event = parse_event(&self.buffer, false, true)?
            .ok_or_else(|| invalid("pending Escape is incomplete"))?;
        self.ready.push_back(Ready {
            event,
            wire: Frame::Input {
                bytes: std::mem::take(&mut self.buffer),
                raw_mode: true,
            },
        });
        self.escape = None;
        Ok(())
    }
    pub fn checkpoint(&self) -> io::Result<Vec<u8>> {
        // Bound serialization's worst-case byte expansion before allocating.
        let estimate = self.ready.iter().fold(
            self.buffer.len().saturating_mul(4).saturating_add(512),
            |total, ready| {
                let cost = match &ready.wire {
                    Frame::Input { bytes, .. } => bytes.len().saturating_mul(4).saturating_add(128),
                    Frame::Resize { .. } => 128,
                };
                total.saturating_add(cost)
            },
        );
        if estimate > MAX_CHECKPOINT {
            return Err(invalid("oversized input checkpoint"));
        }
        let snapshot = SnapshotRef {
            version: 1,
            buffer: &self.buffer,
            ready: self.ready.iter().map(|ready| &ready.wire).collect(),
            escape: self.escape.map(goal_clock::Stamp::parts),
            discarded_sequence: self.discarded_sequence,
        };
        let bytes = serde_json::to_vec(&snapshot)?;
        if bytes.len() > MAX_CHECKPOINT {
            return Err(invalid("oversized input checkpoint"));
        }
        Ok(bytes)
    }
    pub fn restore(bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() > MAX_CHECKPOINT {
            return Err(invalid("oversized input checkpoint"));
        }
        let snapshot: Snapshot = serde_json::from_slice(bytes)?;
        if snapshot.version != 1 {
            return Err(invalid("unsupported input checkpoint"));
        }
        if let Some(discard) = snapshot.discarded_sequence {
            match discard {
                DiscardedSequence::PasteStart(n) if n == 0 || n >= BRACKETED_PASTE_START.len() => {
                    return Err(invalid("invalid paste start quarantine"));
                }
                DiscardedSequence::PasteBody(n) if n >= BRACKETED_PASTE_END.len() => {
                    return Err(invalid("invalid paste end quarantine"));
                }
                DiscardedSequence::X10Mouse(n) if n == 0 || n > 3 => {
                    return Err(invalid("invalid mouse quarantine"));
                }
                _ => {}
            }
            if !snapshot.buffer.is_empty() || snapshot.escape.is_some() {
                return Err(invalid("quarantine contains active input"));
            }
        }
        let escape = snapshot
            .escape
            .map(|(d, s, n)| goal_clock::Stamp::from_parts(d, s, n).map_err(invalid))
            .transpose()?;
        if escape.is_some() != (snapshot.buffer.as_slice() == b"\x1b") {
            return Err(invalid("input checkpoint changed its Escape clock"));
        }
        if let Some(stamp) = escape {
            let mut now = goal_clock::Instant::now();
            goal_clock::adopt(&mut now, stamp).map_err(invalid)?;
        }
        if !snapshot.buffer.is_empty() {
            let mut parsed = Self::default();
            parsed.feed(&snapshot.buffer, true)?;
            if !parsed.ready.is_empty() || parsed.buffer != snapshot.buffer {
                return Err(invalid("pending input contains a completed event"));
            }
        }
        let mut ready = VecDeque::new();
        for frame in snapshot.ready {
            let event = match &frame {
                Frame::Input { bytes, raw_mode } => {
                    if bytes.as_slice() == b"\x1b" {
                        parse_event(bytes, false, *raw_mode)?
                            .ok_or_else(|| invalid("queued Escape is incomplete"))?
                    } else {
                        let mut parsed = Self::default();
                        parsed.feed(bytes, *raw_mode)?;
                        if parsed.ready.len() != 1 || !parsed.buffer.is_empty() {
                            return Err(invalid("queued input frame has partial or extra data"));
                        }
                        parsed
                            .pop()
                            .ok_or_else(|| invalid("queued input frame is incomplete"))?
                    }
                }
                Frame::Resize { columns, rows } => {
                    InternalEvent::Event(Event::Resize(*columns, *rows))
                }
            };
            ready.push_back(Ready { event, wire: frame });
        }
        Ok(Self {
            buffer: snapshot.buffer,
            ready,
            escape,
            discarded_sequence: snapshot.discarded_sequence,
        })
    }
}
