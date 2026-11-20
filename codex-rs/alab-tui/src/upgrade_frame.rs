// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Serialize a checkpoint directly into its length-limited transport frame.
use serde::Serialize;
use std::io;
use std::io::Write;

/// The byte budget includes the header. Sized reservations stop at that budget.
pub fn encode(
    header: &[u8],
    value: &impl Serialize,
    limit: usize,
    oversized: &'static str,
) -> io::Result<Vec<u8>> {
    let mut writer = BoundedWriter {
        bytes: Vec::new(),
        limit,
        oversized,
    };
    writer.write_all(header)?;
    serde_json::to_writer(&mut writer, value)?;
    Ok(writer.bytes)
}

struct BoundedWriter {
    bytes: Vec<u8>,
    limit: usize,
    oversized: &'static str,
}

impl Write for BoundedWriter {
    fn write(&mut self, chunk: &[u8]) -> io::Result<usize> {
        if chunk.len() > self.limit - self.bytes.len() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, self.oversized));
        }
        let needed = self.bytes.len() + chunk.len();
        if needed > self.bytes.capacity() {
            let growth = self
                .bytes
                .capacity()
                .saturating_mul(2)
                .max(needed)
                .min(self.limit);
            self.bytes
                .try_reserve_exact(growth - self.bytes.len())
                .map_err(io::Error::other)?;
        }
        self.bytes.extend_from_slice(chunk);
        Ok(chunk.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
