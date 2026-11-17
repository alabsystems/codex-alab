// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Complete startup DATA for aterm's retained-service settings channel.
//!
//! This codec preserves Unix argument and environment bytes; it does not select
//! an executable or create controller, SDK, foreground or attachment authority.
//! Native admission and the correlated service/descriptor exchange remain the
//! caller's separate responsibility. No ordinary TUI startup uses this yet.
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::io;
use std::os::unix::ffi::OsStrExt as _;
use tokio::io::AsyncWrite;
use tokio::io::AsyncWriteExt as _;

const MAGIC: &[u8; 8] = b"ALABSET1";
/// The native receiver's maximum body size, excluding its twelve-byte header.
pub const SETTINGS_BODY_LIMIT: usize = 16 * 1024 * 1024;

/// An immutable, validated ALABSET1 frame. Its environment may contain secrets,
/// so diagnostics never display the frame or the selected startup values.
pub struct StartupSettings {
    frame: Vec<u8>,
}

impl std::fmt::Debug for StartupSettings {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        output.write_str("StartupSettings(<private>)")
    }
}

impl StartupSettings {
    /// Encode the complete selected argument tail and environment. Arguments
    /// exclude argv[0] and the private retained-service dispatch marker: native
    /// owns the observed image and adds that marker itself.
    ///
    /// Empty arguments and arbitrary non-UTF-8 bytes are preserved. Invalid
    /// environment entries are rejected rather than silently overwritten.
    pub fn new(arguments: &[OsString], environment: &[(OsString, OsString)]) -> io::Result<Self> {
        // Count words and bytes before allocating the encoded frame. The same
        // bound covers count prefixes, length prefixes and raw payload bytes.
        let mut length = 8_usize;
        for argument in arguments {
            add_word(&mut length, argument.as_bytes())?;
        }
        let mut keys = BTreeSet::new();
        for (key, value) in environment {
            let key = key.as_bytes();
            if key.is_empty() || key.contains(&b'=') || !keys.insert(key) {
                return Err(invalid());
            }
            add_word(&mut length, key)?;
            add_word(&mut length, value.as_bytes())?;
        }
        let mut frame = Vec::with_capacity(12 + length);
        frame.extend_from_slice(MAGIC);
        put_number(&mut frame, length)?;
        put_number(&mut frame, arguments.len())?;
        for argument in arguments {
            put_word(&mut frame, argument.as_bytes())?;
        }
        put_number(&mut frame, environment.len())?;
        for (key, value) in environment {
            put_word(&mut frame, key.as_bytes())?;
            put_word(&mut frame, value.as_bytes())?;
        }
        Ok(Self { frame })
    }

    /// Write the complete frame, then close only the writer's direction. Native
    /// requires physical EOF after the body and rejects any trailing bytes.
    /// The caller must authenticate the physical peer and bound the whole
    /// exchange before calling; completion is no native admission receipt.
    pub async fn write_and_shutdown<W: AsyncWrite + Unpin>(
        &self,
        writer: &mut W,
    ) -> io::Result<()> {
        writer.write_all(&self.frame).await?;
        writer.shutdown().await
    }
}

fn add_word(length: &mut usize, word: &[u8]) -> io::Result<()> {
    if word.contains(&0) {
        return Err(invalid());
    }
    *length = length
        .checked_add(4)
        .and_then(|length| length.checked_add(word.len()))
        .filter(|length| *length <= SETTINGS_BODY_LIMIT)
        .ok_or_else(invalid)?;
    Ok(())
}

fn put_number(frame: &mut Vec<u8>, value: usize) -> io::Result<()> {
    frame.extend_from_slice(&u32::try_from(value).map_err(|_| invalid())?.to_be_bytes());
    Ok(())
}

fn put_word(frame: &mut Vec<u8>, word: &[u8]) -> io::Result<()> {
    put_number(frame, word.len())?;
    frame.extend_from_slice(word);
    Ok(())
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "invalid native startup settings",
    )
}

#[cfg(test)]
#[path = "source_retained_service_tests.rs"]
mod tests;
