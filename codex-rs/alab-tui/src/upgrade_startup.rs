// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Terminal startup data, installed only before the actual initialization boundary.
//! Its transport authenticates custody separately; these bytes grant no SDK role.
use serde::Deserialize;
use serde::Serialize;
use std::io;
use std::sync::Mutex;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub version: u32,
    pub cursor: [u16; 2],
    #[serde(deserialize_with = "required")]
    pub colors: Option<[[u8; 3]; 2]>,
    pub enhanced_keys: bool,
    pub terminal_app_over_ssh: bool,
    pub reports_key_releases: bool,
    pub da1_supported: bool,
    #[serde(deserialize_with = "required")]
    pub tmux_image_base: Option<u32>,
    pub viewport: [u16; 4],
    #[serde(deserialize_with = "required")]
    pub saved_viewport: Option<[u16; 4]>,
    pub hidden_cursor: bool,
    pub visible_history_rows: u16,
    pub screen: Screen,
    pub focused: bool,
    pub owned_screen: bool,
    pub alt_screen_enabled: bool,
    pub mouse_capture: u8,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Screen {
    pub active: bool,
    pub mouse_active: bool,
    pub mouse_capture_disabled: bool,
    pub input_configured: bool,
    pub keyboard_active: bool,
    pub parent_reports_key_releases: bool,
    pub pointer: u8,
}
struct State {
    started: bool,
    pending: Option<Snapshot>,
    applied: Option<Snapshot>,
}
static STATE: Mutex<State> = Mutex::new(State {
    started: false,
    pending: None,
    applied: None,
});
impl Snapshot {
    pub fn validate(&self) -> io::Result<()> {
        if self.version != 1
            || self.tmux_image_base == Some(0)
            || self.mouse_capture > 2
            || !matches!(self.screen.pointer, 0..=3 | 255)
        {
            return Err(io::Error::other("incompatible terminal startup data"));
        }
        Ok(())
    }
}
/// Validate all data before changing either native owner. The actual startup
/// hook and this installation serialize under the same destination boundary.
pub fn restore(
    startup: Snapshot,
    modes: &crate::terminal_modes::Snapshot,
    input: &[u8],
) -> io::Result<()> {
    startup.validate()?;
    let mut state = STATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if state.started || state.pending.is_some() {
        return Err(io::Error::other("terminal startup restoration is too late"));
    }
    crate::input::restore_after_exec(modes, input)?;
    state.pending = Some(startup);
    Ok(())
}
/// The production initialization hook calls this before any terminal output,
/// probe or input consumer. Ordinary startup permanently retires installation.
pub fn begin() -> Option<Initialization> {
    let mut state = STATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    state.started = true;
    state
        .pending
        .take()
        .map(|snapshot| Initialization { snapshot })
}

/// The actual initialization attempt owns the exact snapshot selected by begin.
pub struct Initialization {
    snapshot: Snapshot,
}
impl Initialization {
    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }
    pub fn complete(self) {
        STATE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .applied = Some(self.snapshot);
    }
}
pub fn take_initialized() -> Option<Snapshot> {
    STATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .applied
        .take()
}
fn required<'de, D, T>(decoder: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(decoder)
}

/// An early CLI failure may precede the native TUI protocol tracker. Transfer its
/// pending cleanup data to that tracker before ordinary exit restoration.
pub fn take_for_failure() -> Option<Snapshot> {
    let mut state = STATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    state.pending.take().or_else(|| state.applied.take())
}
