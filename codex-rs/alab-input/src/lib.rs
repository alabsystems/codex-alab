// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Joined Unix terminal input with checkpointable queues and partial sequences.
//! A checkpoint is input data only. Its transport must establish process/boot
//! custody independently; it provides no SDK or execution authority.
#![cfg(unix)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod broker;
pub use broker::Broker;
pub use broker::Fence;
pub use broker::Subscription;
mod parse;
mod parser;
mod reader;
use crossterm::event::Event;
use crossterm::event::KeyboardEnhancementFlags;
pub use parser::Parser;
pub use reader::Driver;
pub use reader::Frozen;
pub use reader::open_terminal_input;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InternalEvent {
    Event(Event),
    CursorPosition(u16, u16),
    KeyboardEnhancementFlags(KeyboardEnhancementFlags),
    PrimaryDeviceAttributes,
    OscColor { slot: u8, payload: OscColorPayload },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OscColorPayload {
    Rgb { r: u8, g: u8, b: u8 },
    Unrecognized(String),
}
fn invalid(why: &'static str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, why)
}

#[cfg(test)]
mod tests;
