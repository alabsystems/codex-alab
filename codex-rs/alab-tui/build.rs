// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn Error>> {
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").ok_or("missing OUT_DIR")?);
    for (name, output, rows) in [
        ("codex-mark", "codex-mark", 5),
        ("codex-mark", "codex-mark-compact", 3),
        ("codex-mark", "codex-mark-title", 2),
        ("rainbow-kitty", "rainbow-kitty", 4),
        ("rainbow-kitty", "rainbow-kitty-compact", 2),
    ] {
        let source = format!("src/banner/assets/{name}.png");
        println!("cargo:rerun-if-changed={source}");
        let mut reader = png::Decoder::new(BufReader::new(File::open(&source)?)).read_info()?;
        let mut pixels = vec![0; reader.output_buffer_size().ok_or("oversized banner PNG")?];
        let frame = reader.next_frame(&mut pixels)?;
        if frame.color_type != png::ColorType::Rgba
            || frame.bit_depth != png::BitDepth::Eight
            || frame.height < rows
        {
            return Err(format!("{source} must contain at least {rows} RGBA8 scanlines").into());
        }
        // Proportional boundaries retain every scanline when a tile splits the source unevenly.
        let boundary = |row| (u64::from(row) * u64::from(frame.height) / u64::from(rows)) as usize;
        for row in 0..rows {
            let top = boundary(row);
            let bottom = boundary(row + 1);
            let strip = &pixels[top * frame.line_size..bottom * frame.line_size];
            let file = File::create(out_dir.join(format!("{output}-{row}.png")))?;
            let mut encoder = png::Encoder::new(file, frame.width, (bottom - top) as u32);
            encoder.set_color(frame.color_type);
            encoder.set_depth(frame.bit_depth);
            encoder.set_compression(png::Compression::High);
            if let Some(intent) = reader.info().srgb {
                encoder.set_source_srgb(intent);
            }
            let mut writer = encoder.write_header()?;
            writer.write_image_data(strip)?;
            writer.finish()?;
        }
    }
    Ok(())
}
