// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The banner's colours: aterm's one spectrum and the codex kitty's coat.
//!
//! `SPECTRUM_ANCHORS`, `SPECTRUM_ANCHOR_AT` and `SPECTRUM_LUT` are copied verbatim from aterm,
//! `crates/aterm-effects/src/spectrum.rs` as of aterm commit `35aea96ac` (the file's last
//! change, read at aterm `main` `88f332cfe`). codex-alab must build without aterm, so this is a
//! copy, not a dependency; `palette_is_aterms` pins that the table still lands every named stop
//! on its own bytes. To refresh it, copy the three items again and name the new commit here.
//! The kitty is aterm's codex flagship (`kitty_registry.rs` `flagship_look`: S117, coat 15, iris
//! 7), its colours from `cat_baker.rs` and `art/glyphs/s1_17.toml`.

/// `0x00RRGGBB`.
pub type Rgb = u32;

/// How many named stops the arc has.
pub const SPECTRUM_STOPS: usize = 7;

/// The length of [`SPECTRUM_LUT`].
pub const SPECTRUM_LUT_LEN: usize = 511;

// --- copied verbatim from aterm crates/aterm-effects/src/spectrum.rs (35aea96ac) ---

/// The seven names, `0x00RRGGBB`, red → violet: canonical ROYGBIV and the arc's
/// control points. The generator reads these directly, so its curve cannot
/// disagree with its anchors. Green and blue are adjacent stops on a continuous
/// ramp, so the arc crosses cyan without any authored cyan stripe.
pub const SPECTRUM_ANCHORS: [u32; SPECTRUM_STOPS] = [
    0x00FF_0000, // red
    0x00FF_7F00, // orange
    0x00FF_FF00, // yellow
    0x0000_FF00, // green
    0x0000_00FF, // blue
    0x004B_0082, // indigo
    0x0094_00D3, // violet
];

/// Where the seven names landed, in table indices: `@generated` beside
/// [`SPECTRUM_LUT`] by the same pass, pinned by
/// `spectrum_reproduces_its_anchors_exactly`. Red and violet sit on the ends;
/// the interior names sit where the pace put them. Consumers needing a name
/// ([`spectrum_stop`], [`spectrum_snap_index`]) read it here.
pub const SPECTRUM_ANCHOR_AT: [usize; SPECTRUM_STOPS] = [0, 58, 130, 237, 404, 474, 510];

/// The one spectrum, resolved: 511 entries, 2 KB, `0x00RRGGBB`, red at index
/// `0` and violet at `510`.
///
/// `@generated` by `generate_spectrum_lut`; do not edit by hand. The seven
/// anchors sit verbatim at the indices [`SPECTRUM_ANCHOR_AT`] names; entries
/// between them are the drawn path resampled at an even pace.
#[rustfmt::skip]
pub const SPECTRUM_LUT: [u32; SPECTRUM_LUT_LEN] = [
    0x00FF_0000, 0x00FF_0300, 0x00FF_0500, 0x00FF_0800, 0x00FF_0B00, 0x00FF_0D00,
    0x00FF_1000, 0x00FF_1300, 0x00FF_1500, 0x00FF_1800, 0x00FF_1A00, 0x00FF_1D00,
    0x00FF_1F00, 0x00FF_2200, 0x00FF_2400, 0x00FF_2600, 0x00FF_2900, 0x00FF_2B00,
    0x00FF_2D00, 0x00FF_3000, 0x00FF_3200, 0x00FF_3400, 0x00FF_3700, 0x00FF_3900,
    0x00FF_3B00, 0x00FF_3D00, 0x00FF_3F00, 0x00FF_4200, 0x00FF_4400, 0x00FF_4600,
    0x00FF_4800, 0x00FF_4A00, 0x00FF_4C00, 0x00FF_4E00, 0x00FF_5000, 0x00FF_5200,
    0x00FF_5400, 0x00FF_5600, 0x00FF_5800, 0x00FF_5A00, 0x00FF_5C00, 0x00FF_5E00,
    0x00FF_6000, 0x00FF_6200, 0x00FF_6400, 0x00FF_6600, 0x00FF_6800, 0x00FF_6A00,
    0x00FF_6C00, 0x00FF_6E00, 0x00FF_7000, 0x00FF_7200, 0x00FF_7400, 0x00FF_7600,
    0x00FF_7800, 0x00FF_7900, 0x00FF_7B00, 0x00FF_7D00, 0x00FF_7F00, 0x00FF_8100,
    0x00FF_8300, 0x00FF_8500, 0x00FF_8600, 0x00FF_8800, 0x00FF_8A00, 0x00FF_8C00,
    0x00FF_8E00, 0x00FF_9000, 0x00FF_9100, 0x00FF_9300, 0x00FF_9500, 0x00FF_9700,
    0x00FF_9900, 0x00FF_9A00, 0x00FF_9C00, 0x00FF_9E00, 0x00FF_A000, 0x00FF_A200,
    0x00FF_A300, 0x00FF_A500, 0x00FF_A700, 0x00FF_A900, 0x00FF_AB00, 0x00FF_AC00,
    0x00FF_AE00, 0x00FF_B000, 0x00FF_B200, 0x00FF_B300, 0x00FF_B500, 0x00FF_B700,
    0x00FF_B900, 0x00FF_BB00, 0x00FF_BC00, 0x00FF_BE00, 0x00FF_C000, 0x00FF_C200,
    0x00FF_C300, 0x00FF_C500, 0x00FF_C700, 0x00FF_C900, 0x00FF_CA00, 0x00FF_CC00,
    0x00FF_CE00, 0x00FF_D000, 0x00FF_D100, 0x00FF_D300, 0x00FF_D500, 0x00FF_D700,
    0x00FF_D800, 0x00FF_DA00, 0x00FF_DC00, 0x00FF_DE00, 0x00FF_DF00, 0x00FF_E100,
    0x00FF_E300, 0x00FF_E500, 0x00FF_E600, 0x00FF_E800, 0x00FF_EA00, 0x00FF_EC00,
    0x00FF_ED00, 0x00FF_EF00, 0x00FF_F100, 0x00FF_F300, 0x00FF_F400, 0x00FF_F600,
    0x00FF_F800, 0x00FF_FA00, 0x00FF_FB00, 0x00FF_FD00, 0x00FF_FF00, 0x00FD_FF00,
    0x00FB_FF00, 0x00F9_FF00, 0x00F7_FF00, 0x00F4_FF00, 0x00F2_FF00, 0x00F0_FF00,
    0x00EE_FF00, 0x00EC_FF00, 0x00EA_FF00, 0x00E8_FF00, 0x00E5_FF00, 0x00E3_FF00,
    0x00E1_FF00, 0x00DF_FF00, 0x00DD_FF00, 0x00DB_FF00, 0x00D9_FF00, 0x00D6_FF00,
    0x00D4_FF00, 0x00D2_FF00, 0x00D0_FF00, 0x00CE_FF00, 0x00CC_FF00, 0x00C9_FF00,
    0x00C7_FF00, 0x00C5_FF00, 0x00C3_FF00, 0x00C1_FF00, 0x00BE_FF00, 0x00BC_FF00,
    0x00BA_FF00, 0x00B8_FF00, 0x00B6_FF00, 0x00B3_FF00, 0x00B1_FF00, 0x00AF_FF00,
    0x00AD_FF00, 0x00AA_FF00, 0x00A8_FF00, 0x00A6_FF00, 0x00A4_FF00, 0x00A1_FF00,
    0x009F_FF00, 0x009D_FF00, 0x009A_FF00, 0x0098_FF00, 0x0096_FF00, 0x0094_FF00,
    0x0091_FF00, 0x008F_FF00, 0x008D_FF00, 0x008A_FF00, 0x0088_FF00, 0x0086_FF00,
    0x0083_FF00, 0x0081_FF00, 0x007F_FF00, 0x007C_FF00, 0x007A_FF00, 0x0077_FF00,
    0x0075_FF00, 0x0073_FF00, 0x0070_FF00, 0x006E_FF00, 0x006B_FF00, 0x0069_FF00,
    0x0067_FF00, 0x0064_FF00, 0x0062_FF00, 0x005F_FF00, 0x005D_FF00, 0x005A_FF00,
    0x0058_FF00, 0x0055_FF00, 0x0053_FF00, 0x0050_FF00, 0x004E_FF00, 0x004B_FF00,
    0x0049_FF00, 0x0046_FF00, 0x0043_FF00, 0x0041_FF00, 0x003E_FF00, 0x003C_FF00,
    0x0039_FF00, 0x0036_FF00, 0x0034_FF00, 0x0031_FF00, 0x002E_FF00, 0x002C_FF00,
    0x0029_FF00, 0x0026_FF00, 0x0024_FF00, 0x0021_FF00, 0x001E_FF00, 0x001C_FF00,
    0x0019_FF00, 0x0016_FF00, 0x0013_FF00, 0x0011_FF00, 0x000E_FF00, 0x000B_FF00,
    0x0008_FF00, 0x0006_FF00, 0x0003_FF00, 0x0000_FF00, 0x0000_FF04, 0x0000_FF08,
    0x0000_FF0C, 0x0000_FF10, 0x0000_FF14, 0x0000_FF18, 0x0000_FF1C, 0x0000_FF20,
    0x0000_FF24, 0x0000_FF28, 0x0000_FF2C, 0x0000_FF30, 0x0000_FF34, 0x0000_FF38,
    0x0000_FF3B, 0x0000_FF3F, 0x0000_FF43, 0x0000_FF46, 0x0000_FF4A, 0x0000_FF4E,
    0x0000_FF51, 0x0000_FF55, 0x0000_FF58, 0x0000_FF5C, 0x0000_FF5F, 0x0000_FF63,
    0x0000_FF66, 0x0000_FF6A, 0x0000_FF6D, 0x0000_FF71, 0x0000_FF74, 0x0000_FF77,
    0x0000_FF7B, 0x0000_FF7E, 0x0000_FF81, 0x0000_FF85, 0x0000_FF88, 0x0000_FF8B,
    0x0000_FF8F, 0x0000_FF92, 0x0000_FF95, 0x0000_FF98, 0x0000_FF9C, 0x0000_FF9F,
    0x0000_FFA2, 0x0000_FFA5, 0x0000_FFA9, 0x0000_FFAC, 0x0000_FFAF, 0x0000_FFB2,
    0x0000_FFB5, 0x0000_FFB9, 0x0000_FFBC, 0x0000_FFBF, 0x0000_FFC2, 0x0000_FFC5,
    0x0000_FFC8, 0x0000_FFCC, 0x0000_FFCF, 0x0000_FFD2, 0x0000_FFD5, 0x0000_FFD8,
    0x0000_FFDB, 0x0000_FFDF, 0x0000_FFE2, 0x0000_FFE5, 0x0000_FFE8, 0x0000_FFEB,
    0x0000_FFEE, 0x0000_FFF1, 0x0000_FFF4, 0x0000_FFF8, 0x0000_FFFB, 0x0000_FFFE,
    0x0000_FDFF, 0x0000_FBFF, 0x0000_F8FF, 0x0000_F6FF, 0x0000_F4FF, 0x0000_F1FF,
    0x0000_EFFF, 0x0000_ECFF, 0x0000_EAFF, 0x0000_E7FF, 0x0000_E5FF, 0x0000_E2FF,
    0x0000_E0FF, 0x0000_DDFF, 0x0000_DBFF, 0x0000_D8FF, 0x0000_D6FF, 0x0000_D3FF,
    0x0000_D1FF, 0x0000_CEFF, 0x0000_CCFF, 0x0000_C9FF, 0x0000_C7FF, 0x0000_C4FF,
    0x0000_C2FF, 0x0000_BFFF, 0x0000_BDFF, 0x0000_BAFF, 0x0000_B8FF, 0x0000_B5FF,
    0x0000_B3FF, 0x0000_B0FF, 0x0000_AEFF, 0x0000_ABFF, 0x0000_A9FF, 0x0000_A6FF,
    0x0000_A4FF, 0x0000_A1FF, 0x0000_9FFF, 0x0000_9CFF, 0x0000_9AFF, 0x0000_97FF,
    0x0000_95FF, 0x0000_92FF, 0x0000_90FF, 0x0000_8DFF, 0x0000_8AFF, 0x0000_88FF,
    0x0000_85FF, 0x0000_83FF, 0x0000_80FF, 0x0000_7EFF, 0x0000_7BFF, 0x0000_78FF,
    0x0000_76FF, 0x0000_73FF, 0x0000_70FF, 0x0000_6EFF, 0x0000_6BFF, 0x0000_69FF,
    0x0000_66FF, 0x0000_63FF, 0x0000_60FF, 0x0000_5EFF, 0x0000_5BFF, 0x0000_58FF,
    0x0000_55FF, 0x0000_53FF, 0x0000_50FF, 0x0000_4DFF, 0x0000_4AFF, 0x0000_47FF,
    0x0000_44FF, 0x0000_41FF, 0x0000_3EFF, 0x0000_3BFF, 0x0000_38FF, 0x0000_35FF,
    0x0000_32FF, 0x0000_2FFF, 0x0000_2BFF, 0x0000_28FF, 0x0000_25FF, 0x0000_21FF,
    0x0000_1EFF, 0x0000_1AFF, 0x0000_17FF, 0x0000_13FF, 0x0000_0FFF, 0x0000_0CFF,
    0x0000_08FF, 0x0000_04FF, 0x0000_00FF, 0x0001_00FD, 0x0003_00FA, 0x0004_00F8,
    0x0006_00F5, 0x0007_00F3, 0x0009_00F1, 0x000A_00EE, 0x000B_00EC, 0x000D_00EA,
    0x000E_00E8, 0x000F_00E5, 0x0011_00E3, 0x0012_00E1, 0x0013_00DF, 0x0015_00DC,
    0x0016_00DA, 0x0017_00D8, 0x0019_00D6, 0x001A_00D4, 0x001B_00D2, 0x001C_00D0,
    0x001E_00CE, 0x001F_00CC, 0x0020_00CA, 0x0021_00C8, 0x0022_00C6, 0x0023_00C4,
    0x0025_00C2, 0x0026_00C0, 0x0027_00BE, 0x0028_00BC, 0x0029_00BB, 0x002A_00B9,
    0x002B_00B7, 0x002C_00B5, 0x002D_00B3, 0x002E_00B2, 0x002F_00B0, 0x0030_00AE,
    0x0031_00AD, 0x0032_00AB, 0x0033_00A9, 0x0034_00A8, 0x0035_00A6, 0x0036_00A4,
    0x0037_00A3, 0x0038_00A1, 0x0039_00A0, 0x003A_009E, 0x003B_009D, 0x003C_009B,
    0x003D_009A, 0x003E_0098, 0x003E_0097, 0x003F_0096, 0x0040_0094, 0x0041_0093,
    0x0042_0091, 0x0043_0090, 0x0043_008F, 0x0044_008D, 0x0045_008C, 0x0046_008B,
    0x0047_0089, 0x0047_0088, 0x0048_0087, 0x0049_0086, 0x004A_0084, 0x004A_0083,
    0x004B_0082, 0x004D_0084, 0x004F_0086, 0x0050_0088, 0x0052_008A, 0x0054_008C,
    0x0056_008E, 0x0058_0090, 0x005A_0092, 0x005C_0094, 0x005E_0097, 0x005F_0099,
    0x0061_009B, 0x0063_009D, 0x0065_009F, 0x0067_00A1, 0x0069_00A4, 0x006B_00A6,
    0x006D_00A8, 0x006F_00AA, 0x0072_00AD, 0x0074_00AF, 0x0076_00B1, 0x0078_00B4,
    0x007A_00B6, 0x007C_00B8, 0x007E_00BB, 0x0080_00BD, 0x0082_00C0, 0x0085_00C2,
    0x0087_00C4, 0x0089_00C7, 0x008B_00C9, 0x008D_00CC, 0x0090_00CE, 0x0092_00D1,
    0x0094_00D3,
];

// --- end of the aterm copy ---

/// aterm's `spectrum(t)`: the colour at position `t` (0 red .. 1 violet), two table reads and
/// one lerp, rounded as aterm's `lerp_rgb` rounds. A non-finite `t` draws red.
#[allow(
    clippy::manual_clamp,
    reason = "as in aterm: `clamp` propagates NaN, `max`/`min` fold it to an end"
)]
pub fn spectrum(t: f32) -> Rgb {
    let x = t.max(0.0).min(1.0) * (SPECTRUM_LUT_LEN - 1) as f32;
    let i = (x as usize).min(SPECTRUM_LUT_LEN - 1);
    let j = (i + 1).min(SPECTRUM_LUT_LEN - 1);
    lerp_rgb(SPECTRUM_LUT[i], SPECTRUM_LUT[j], x - i as f32)
}

/// The colour of named stop `i` (0 red .. 6 violet), read from the table at its own index.
pub fn spectrum_stop(i: usize) -> Rgb {
    SPECTRUM_LUT[SPECTRUM_ANCHOR_AT[i.min(SPECTRUM_STOPS - 1)]]
}

/// The index of the named stop nearest position `t`, as aterm's `spectrum_snap_index`.
pub fn spectrum_snap_index(t: f32) -> usize {
    let x = t.clamp(0.0, 1.0) * (SPECTRUM_LUT_LEN - 1) as f32;
    let mut best = 0usize;
    let mut near = f32::INFINITY;
    for (i, &at) in SPECTRUM_ANCHOR_AT.iter().enumerate() {
        let d = (x - at as f32).abs();
        if d < near {
            near = d;
            best = i;
        }
    }
    best
}

/// aterm's `lerp_rgb` (`effect_util.rs`): per-channel, `+ 0.5` and truncate.
fn lerp_rgb(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    let mix = |sh: u32| {
        let ca = ((a >> sh) & 0xff) as f32;
        let cb = ((b >> sh) & 0xff) as f32;
        ((ca + (cb - ca) * t) + 0.5) as u32
    };
    (mix(16) << 16) | (mix(8) << 8) | mix(0)
}

// --- copied verbatim from aterm crates/aterm-effects/src/harness_card.rs (5736bfb96d) ---

/// The moonbow on a dark ground, `0x00RRGGBB`: white, pearl, silver, steel. aterm's harness
/// glass wears it under Autopilot off (`CardPalette::Off`).
pub const MOONBOW: [u32; 4] = [0x00FF_FFFF, 0x00E4_EAF3, 0x00C3_CCDA, 0x00A2_ADBE];

/// The moonbow in print, on a light ground: pearl to graphite.
pub const PRINT_OFF: [u32; 6] = [
    0x00C4_CAD4,
    0x00AA_B1BE,
    0x0093_9BA9,
    0x007C_8494,
    0x0065_6D7D,
    0x004F_5666,
];

// --- end of copy ---

/// The moonbow at position `t` (0 white or pearl .. 1 steel or graphite): [`MOONBOW`] on a
/// dark ground, [`PRINT_OFF`] on a `light` one, its stops spread evenly, as aterm's
/// `CardPalette::Off.art`. A non-finite `t` draws the first stop.
pub fn moonbow(t: f32, light: bool) -> Rgb {
    let stops: &[u32] = if light { &PRINT_OFF } else { &MOONBOW };
    let t = if t.is_finite() {
        t.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let x = t * (stops.len() - 1) as f32;
    let i = (x as usize).min(stops.len() - 2);
    mix(stops[i], stops[i + 1], x - i as f32)
}

/// The white-hot twinkle (`effect_util.rs` `twinkle_rgb(false)`), the meteor head and hero star.
pub const WHITE_HOT: Rgb = 0xFF_FFFF;
/// The gold twinkle (`effect_util.rs` `twinkle_rgb(true)`).
pub const GOLD: Rgb = 0xFF_E9A8;
/// How far an m2 star's body goes from white to its stop (RAINBOW-KITTY-V2 D2).
pub const M2_BODY_TINT: f32 = 0.6;
/// The codex kitty's coat, `COAT_RAMP[15]`.
pub const COAT: Rgb = 0xFA_F4E8;
/// Its iris, `EYE_RAMP[7]` ("storm-teal").
pub const IRIS: Rgb = 0x5D_7B78;
/// The left (dark) calico patch, s1_17 `pattern` layer 1.
pub const PATCH_DARK: Rgb = 0x4B_434D;
/// The right (ginger) calico patch, s1_17 `pattern` layer 2.
pub const PATCH_GINGER: Rgb = 0xB9_6F4F;
/// Inner ear and nose, s1_17 `inner_ear`.
pub const INNER_EAR: Rgb = 0xD9_8D9A;
/// The muzzle, s1_17 `muzzle`.
pub const MUZZLE: Rgb = 0xF1_DDC9;
/// The outline on dark backgrounds, `cat_baker.rs` `OUTLINE_PALE`.
pub const OUTLINE_PALE: Rgb = 0xF4_E6DD;
/// aterm's ink: the outline on the lightest band, and white-hot's stand-in on light themes.
pub const INK: Rgb = 0x10_0D16;
/// The pupil, `cat_baker.rs` `EYE_INK_DARK`: the iris alone is about 1.15:1 on the ginger patch.
pub const EYE_INK_DARK: Rgb = 0x18_1520;
/// The background assumed when the terminal does not report one: codex's blossom fallback.
pub const FALLBACK_DARK_BG: Rgb = 0x0F_1425;
/// The contrast every coloured run keeps against a light background.
pub const LIGHT_CONTRAST_FLOOR: f64 = 1.6;
/// The contrast the wordmark's letters keep against any background: WCAG AA for text. The
/// spectrum's blue, indigo and violet stops are 1.4 to 2.8:1 on a dark terminal.
pub const TEXT_CONTRAST_FLOOR: f64 = 4.5;

/// `(r, g, b)` of a packed colour.
pub fn channels(c: Rgb) -> (u8, u8, u8) {
    (
        ((c >> 16) & 0xff) as u8,
        ((c >> 8) & 0xff) as u8,
        (c & 0xff) as u8,
    )
}

/// The packed colour of `(r, g, b)`.
pub fn pack((r, g, b): (u8, u8, u8)) -> Rgb {
    (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
}

/// `a` moved toward `b` by `t`, rounded to nearest.
pub fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    let ch = |sh: u32| {
        let x = ((a >> sh) & 0xff) as f32;
        let y = ((b >> sh) & 0xff) as f32;
        (x + (y - x) * t).round().clamp(0.0, 255.0) as u32
    };
    (ch(16) << 16) | (ch(8) << 8) | ch(0)
}

fn linear(v: u8) -> f64 {
    let v = f64::from(v) / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// WCAG relative luminance.
pub fn luminance(c: Rgb) -> f64 {
    let (r, g, b) = channels(c);
    0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
}

/// WCAG contrast ratio of two colours.
pub fn contrast(a: Rgb, b: Rgb) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Whether a background is light: Rec.601 luma over 128, as codex's `color::is_light`.
pub fn is_light(bg: Rgb) -> bool {
    let (r, g, b) = channels(bg);
    0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b) > 128.0
}

/// `c`, darkened toward [`INK`] until it clears [`LIGHT_CONTRAST_FLOOR`] against `bg`: at most
/// six bisection steps, hue family kept (the shape of aterm's `cat_baker::lift_coat_luminance`).
/// Yellow on white becomes a dark olive gold. A colour already clear comes back unchanged.
pub fn rescue_on_light(c: Rgb, bg: Rgb) -> Rgb {
    rescue(c, bg, INK, LIGHT_CONTRAST_FLOOR)
}

/// `c`, moved toward `toward` until it clears `floor` against `bg`, in at most six bisection
/// steps; `toward` itself when even that falls short. A colour already clear comes back
/// unchanged.
pub fn rescue(c: Rgb, bg: Rgb, toward: Rgb, floor: f64) -> Rgb {
    if contrast(c, bg) >= floor {
        return c;
    }
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..6 {
        let mid = (lo + hi) / 2.0;
        if contrast(mix(c, toward, mid), bg) >= floor {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    mix(c, toward, hi)
}

/// The xterm-256 colour nearest `c` among the fixed entries 16..=255 (0..=15 are the theme's),
/// by CIE76 distance, as codex's `terminal_palette::best_color_for_level` measures it.
pub fn nearest_xterm256(c: Rgb) -> u8 {
    let target = lab(c);
    let mut best = 16u8;
    let mut near = f64::INFINITY;
    for index in 16..=255u8 {
        let d = distance(lab(xterm256(index)), target);
        if d < near {
            near = d;
            best = index;
        }
    }
    best
}

/// [`nearest_xterm256`] among the entries that still clear `floor` against `bg`: the nearest
/// entry to a rescued colour can land on the wrong side of the floor (yellow-green `#59E859`
/// rounds to entry 83, 1.3:1 on white). With no entry clear, the nearest of all.
pub fn nearest_xterm256_clearing(c: Rgb, bg: Rgb, floor: f64) -> u8 {
    let target = lab(c);
    let mut best = None;
    let mut near = f64::INFINITY;
    for index in 16..=255u8 {
        let entry = xterm256(index);
        if contrast(entry, bg) < floor {
            continue;
        }
        let d = distance(lab(entry), target);
        if d < near {
            near = d;
            best = Some(index);
        }
    }
    best.unwrap_or_else(|| nearest_xterm256(c))
}

/// The colour xterm draws for fixed entry `index` (16..=255).
pub fn xterm256(index: u8) -> Rgb {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    if index >= 232 {
        let v = 8 + 10 * (index - 232);
        return pack((v, v, v));
    }
    let i = usize::from(index.saturating_sub(16));
    pack((LEVELS[i / 36], LEVELS[(i / 6) % 6], LEVELS[i % 6]))
}

fn lab(c: Rgb) -> (f64, f64, f64) {
    let (r, g, b) = channels(c);
    let (r, g, b) = (linear(r), linear(g), linear(b));
    let x = (r * 0.4124 + g * 0.3576 + b * 0.1805) / 0.95047;
    let y = r * 0.2126 + g * 0.7152 + b * 0.0722;
    let z = (r * 0.0193 + g * 0.1192 + b * 0.9505) / 1.08883;
    let f = |t: f64| {
        if t > 0.008856 {
            t.cbrt()
        } else {
            7.787 * t + 16.0 / 116.0
        }
    };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    (116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz))
}

fn distance(a: (f64, f64, f64), b: (f64, f64, f64)) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2) + (a.2 - b.2).powi(2)).sqrt()
}
