// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Exact physical binding DATA; no key normalization or default keymap on restore.
use super::*;
use crossterm::event::MediaKeyCode;
use crossterm::event::ModifierKeyCode;
use serde::Deserialize;
use serde::Serialize;
use std::io;
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Code {
    kind: u8,
    value: u32,
}
impl Code {
    fn capture(actual: KeyCode) -> Self {
        let (kind, value) = match actual {
            KeyCode::Backspace => (0, 0),
            KeyCode::Enter => (1, 0),
            KeyCode::Left => (2, 0),
            KeyCode::Right => (3, 0),
            KeyCode::Up => (4, 0),
            KeyCode::Down => (5, 0),
            KeyCode::Home => (6, 0),
            KeyCode::End => (7, 0),
            KeyCode::PageUp => (8, 0),
            KeyCode::PageDown => (9, 0),
            KeyCode::Tab => (10, 0),
            KeyCode::BackTab => (11, 0),
            KeyCode::Delete => (12, 0),
            KeyCode::Insert => (13, 0),
            KeyCode::Null => (14, 0),
            KeyCode::Esc => (15, 0),
            KeyCode::CapsLock => (16, 0),
            KeyCode::ScrollLock => (17, 0),
            KeyCode::NumLock => (18, 0),
            KeyCode::PrintScreen => (19, 0),
            KeyCode::Pause => (20, 0),
            KeyCode::Menu => (21, 0),
            KeyCode::KeypadBegin => (22, 0),
            KeyCode::F(value) => (23, u32::from(value)),
            KeyCode::Char(value) => (24, u32::from(value)),
            KeyCode::Media(value) => (
                25,
                match value {
                    MediaKeyCode::Play => 0,
                    MediaKeyCode::Pause => 1,
                    MediaKeyCode::PlayPause => 2,
                    MediaKeyCode::Reverse => 3,
                    MediaKeyCode::Stop => 4,
                    MediaKeyCode::FastForward => 5,
                    MediaKeyCode::Rewind => 6,
                    MediaKeyCode::TrackNext => 7,
                    MediaKeyCode::TrackPrevious => 8,
                    MediaKeyCode::Record => 9,
                    MediaKeyCode::LowerVolume => 10,
                    MediaKeyCode::RaiseVolume => 11,
                    MediaKeyCode::MuteVolume => 12,
                },
            ),
            KeyCode::Modifier(value) => (
                26,
                match value {
                    ModifierKeyCode::LeftShift => 0,
                    ModifierKeyCode::LeftControl => 1,
                    ModifierKeyCode::LeftAlt => 2,
                    ModifierKeyCode::LeftSuper => 3,
                    ModifierKeyCode::LeftHyper => 4,
                    ModifierKeyCode::LeftMeta => 5,
                    ModifierKeyCode::RightShift => 6,
                    ModifierKeyCode::RightControl => 7,
                    ModifierKeyCode::RightAlt => 8,
                    ModifierKeyCode::RightSuper => 9,
                    ModifierKeyCode::RightHyper => 10,
                    ModifierKeyCode::RightMeta => 11,
                    ModifierKeyCode::IsoLevel3Shift => 12,
                    ModifierKeyCode::IsoLevel5Shift => 13,
                },
            ),
        };
        Self { kind, value }
    }
    fn restore(self) -> io::Result<KeyCode> {
        let invalid = || io::Error::new(io::ErrorKind::InvalidData, "native physical key shape");
        if self.kind <= 22 && self.value != 0 {
            return Err(invalid());
        }
        Ok(match self.kind {
            0 => KeyCode::Backspace,
            1 => KeyCode::Enter,
            2 => KeyCode::Left,
            3 => KeyCode::Right,
            4 => KeyCode::Up,
            5 => KeyCode::Down,
            6 => KeyCode::Home,
            7 => KeyCode::End,
            8 => KeyCode::PageUp,
            9 => KeyCode::PageDown,
            10 => KeyCode::Tab,
            11 => KeyCode::BackTab,
            12 => KeyCode::Delete,
            13 => KeyCode::Insert,
            14 => KeyCode::Null,
            15 => KeyCode::Esc,
            16 => KeyCode::CapsLock,
            17 => KeyCode::ScrollLock,
            18 => KeyCode::NumLock,
            19 => KeyCode::PrintScreen,
            20 => KeyCode::Pause,
            21 => KeyCode::Menu,
            22 => KeyCode::KeypadBegin,
            23 => KeyCode::F(u8::try_from(self.value).map_err(|_| invalid())?),
            24 => KeyCode::Char(char::from_u32(self.value).ok_or_else(invalid)?),
            25 => KeyCode::Media(match self.value {
                0 => MediaKeyCode::Play,
                1 => MediaKeyCode::Pause,
                2 => MediaKeyCode::PlayPause,
                3 => MediaKeyCode::Reverse,
                4 => MediaKeyCode::Stop,
                5 => MediaKeyCode::FastForward,
                6 => MediaKeyCode::Rewind,
                7 => MediaKeyCode::TrackNext,
                8 => MediaKeyCode::TrackPrevious,
                9 => MediaKeyCode::Record,
                10 => MediaKeyCode::LowerVolume,
                11 => MediaKeyCode::RaiseVolume,
                12 => MediaKeyCode::MuteVolume,
                _ => return Err(invalid()),
            }),
            26 => KeyCode::Modifier(match self.value {
                0 => ModifierKeyCode::LeftShift,
                1 => ModifierKeyCode::LeftControl,
                2 => ModifierKeyCode::LeftAlt,
                3 => ModifierKeyCode::LeftSuper,
                4 => ModifierKeyCode::LeftHyper,
                5 => ModifierKeyCode::LeftMeta,
                6 => ModifierKeyCode::RightShift,
                7 => ModifierKeyCode::RightControl,
                8 => ModifierKeyCode::RightAlt,
                9 => ModifierKeyCode::RightSuper,
                10 => ModifierKeyCode::RightHyper,
                11 => ModifierKeyCode::RightMeta,
                12 => ModifierKeyCode::IsoLevel3Shift,
                13 => ModifierKeyCode::IsoLevel5Shift,
                _ => return Err(invalid()),
            }),
            _ => return Err(invalid()),
        })
    }
}
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Key {
    code: Code,
    modifiers: u8,
}
impl Key {
    pub(crate) fn capture(actual: KeyBinding) -> Self {
        let (code, modifiers) = actual.parts();
        Self {
            code: Code::capture(code),
            modifiers: modifiers.bits(),
        }
    }
    pub(crate) fn restore(self) -> io::Result<KeyBinding> {
        Ok(KeyBinding::new(
            self.code.restore()?,
            KeyModifiers::from_bits(self.modifiers).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "native key modifiers")
            })?,
        ))
    }
}
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum Hint {
    Single { key: Key },
    Chord { prefix: Key, completion: Key },
}
impl Hint {
    pub(crate) fn capture(actual: ShortcutHint) -> Self {
        match actual {
            ShortcutHint::Single(key) => Self::Single {
                key: Key::capture(key),
            },
            ShortcutHint::Chord { prefix, completion } => Self::Chord {
                prefix: Key::capture(prefix),
                completion: Key::capture(completion),
            },
        }
    }
    pub(crate) fn restore(self) -> io::Result<ShortcutHint> {
        Ok(match self {
            Self::Single { key } => ShortcutHint::Single(key.restore()?),
            Self::Chord { prefix, completion } => ShortcutHint::Chord {
                prefix: prefix.restore()?,
                completion: completion.restore()?,
            },
        })
    }
}
#[cfg(test)]
mod controls {
    use super::*;
    #[test]
    fn exact_physical_unicode_chord_media_and_modifier_bindings_survive_without_normalization()
    -> io::Result<()> {
        for code in [
            KeyCode::BackTab,
            KeyCode::Char('界'),
            KeyCode::F(0),
            KeyCode::Media(MediaKeyCode::PlayPause),
            KeyCode::Modifier(ModifierKeyCode::IsoLevel5Shift),
        ] {
            let key = KeyBinding::new(code, KeyModifiers::CONTROL | KeyModifiers::ALT);
            let actual = Hint::capture(ShortcutHint::Chord {
                prefix: key,
                completion: key,
            });
            let raw = serde_json::to_vec(&actual).map_err(io::Error::other)?;
            let restored: Hint = serde_json::from_slice(&raw).map_err(io::Error::other)?;
            assert_eq!(
                restored.restore()?,
                ShortcutHint::Chord {
                    prefix: key,
                    completion: key
                }
            );
        }
        assert!(Code { kind: 0, value: 1 }.restore().is_err());
        assert!(
            Code {
                kind: 24,
                value: 0xD800
            }
            .restore()
            .is_err()
        );
        assert!(
            Code {
                kind: 25,
                value: 99
            }
            .restore()
            .is_err()
        );
        assert!(
            Key {
                code: Code { kind: 0, value: 0 },
                modifiers: u8::MAX
            }
            .restore()
            .is_err()
        );
        Ok(())
    }
}
