// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Exact native decoded image DATA. No path reopen, thumbnail or terminal fallback.
use super::*;
use serde::Deserialize;
use serde::Serialize;
use std::io;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ImageData {
    protocol: u8,
    columns: u16,
    rows: u16,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    cell_width: u32,
    cell_height: u32,
}
impl ImageData {
    pub(crate) fn capture(value: &TranscriptImage) -> io::Result<Self> {
        let data = Self {
            protocol: match value.protocol {
                ImageProtocol::Kitty => 0,
                ImageProtocol::KittyLocalFile => 1,
                ImageProtocol::Sixel => 2,
            },
            columns: value.columns,
            rows: value.rows,
            width: value.pixels.width(),
            height: value.pixels.height(),
            rgba: value.pixels.as_raw().clone(),
            cell_width: value.cell_width,
            cell_height: value.cell_height,
        };
        data.validate()?;
        Ok(data)
    }
    fn validate(&self) -> io::Result<()> {
        let bytes = u64::from(self.width)
            .checked_mul(u64::from(self.height))
            .and_then(|v| v.checked_mul(4));
        if self.width == 0
            || self.height == 0
            || bytes.is_none_or(|v| v > 32 * 1024 * 1024 || v != self.rgba.len() as u64)
            || self.columns == 0
            || self.rows == 0
            || self.cell_width == 0
            || self.cell_height == 0
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "missing/invalid original native image raster",
            ));
        }
        Ok(())
    }
    pub(crate) fn restore(&self) -> io::Result<TranscriptImage> {
        self.validate()?;
        let protocol = match self.protocol {
            0 => ImageProtocol::Kitty,
            1 => ImageProtocol::KittyLocalFile,
            2 => ImageProtocol::Sixel,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unknown original image protocol",
                ));
            }
        };
        let pixels =
            RgbaImage::from_raw(self.width, self.height, self.rgba.clone()).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "original native image pixels unavailable",
                )
            })?;
        Ok(TranscriptImage {
            protocol,
            columns: self.columns,
            rows: self.rows,
            pixels,
            cell_width: self.cell_width,
            cell_height: self.cell_height,
        })
    }
}
