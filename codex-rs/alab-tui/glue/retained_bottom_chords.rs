// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Exact native chord declarations/configured alternatives; no action execution/factory.
use super::*;
use crate::key_hint::alab_retained_keys::Key;
use crate::keymap::keymap_action_id;
use serde::Deserialize;
use serde::Serialize;
use std::io;
#[derive(Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Action {
    context: u8,
    name: String,
}
impl Action {
    fn capture(actual: KeymapActionId) -> Self {
        let context = match actual.context {
            KeymapContext::Global => 0,
            KeymapContext::Chat => 1,
            KeymapContext::Voice => 2,
            KeymapContext::Composer => 3,
            KeymapContext::Editor => 4,
            KeymapContext::VimNormal => 5,
            KeymapContext::VimOperator => 6,
            KeymapContext::VimSearch => 7,
            KeymapContext::VimTextObject => 8,
            KeymapContext::Pager => 9,
            KeymapContext::List => 10,
            KeymapContext::Agents => 11,
            KeymapContext::Approval => 12,
        };
        Self {
            context,
            name: actual.action.into(),
        }
    }
    fn restore(self) -> io::Result<KeymapActionId> {
        let invalid = || {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "native chord action unavailable",
            )
        };
        let context = match self.context {
            0 => KeymapContext::Global,
            1 => KeymapContext::Chat,
            2 => KeymapContext::Voice,
            3 => KeymapContext::Composer,
            4 => KeymapContext::Editor,
            5 => KeymapContext::VimNormal,
            6 => KeymapContext::VimOperator,
            7 => KeymapContext::VimSearch,
            8 => KeymapContext::VimTextObject,
            9 => KeymapContext::Pager,
            10 => KeymapContext::List,
            11 => KeymapContext::Agents,
            12 => KeymapContext::Approval,
            _ => return Err(invalid()),
        };
        let actual = keymap_action_id(context.config_name(), &self.name).ok_or_else(invalid)?;
        if actual.context != context {
            return Err(invalid());
        }
        Ok(actual)
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    action: Action,
    prefix: Key,
    completion: Key,
    spec: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Data {
    leader: Vec<Key>,
    bindings: Vec<Binding>,
    configured: Vec<(Action, Vec<String>)>,
}
impl Data {
    pub(crate) fn capture(actual: &RuntimeChordKeymap) -> Self {
        let RuntimeChordKeymap {
            leader,
            bindings,
            configured_specs,
        } = actual;
        Self {
            leader: leader.iter().copied().map(Key::capture).collect(),
            bindings: bindings
                .iter()
                .map(|b| Binding {
                    action: Action::capture(b.action),
                    prefix: Key::capture(b.chord.prefix),
                    completion: Key::capture(b.chord.completion),
                    spec: b.spec.clone(),
                })
                .collect(),
            configured: configured_specs
                .iter()
                .map(|(a, v)| (Action::capture(*a), v.clone()))
                .collect(),
        }
    }
    pub(crate) fn restore(self) -> io::Result<RuntimeChordKeymap> {
        Ok(RuntimeChordKeymap {
            leader: self
                .leader
                .into_iter()
                .map(Key::restore)
                .collect::<io::Result<_>>()?,
            bindings: self
                .bindings
                .into_iter()
                .map(|b| -> io::Result<_> {
                    Ok(RuntimeChordBinding {
                        action: b.action.restore()?,
                        chord: KeyChord {
                            prefix: b.prefix.restore()?,
                            completion: b.completion.restore()?,
                        },
                        spec: b.spec,
                    })
                })
                .collect::<io::Result<_>>()?,
            configured_specs: self
                .configured
                .into_iter()
                .map(|(a, v)| a.restore().map(|a| (a, v)))
                .collect::<io::Result<_>>()?,
        })
    }
}
#[cfg(test)]
mod controls {
    use super::*;
    #[test]
    fn actual_action_identity_is_closed_without_static_string_leaks_or_new_actions()
    -> io::Result<()> {
        let actual = keymap_action_id("global", "open_transcript")
            .ok_or_else(|| io::Error::other("original native action missing"))?;
        let wire = serde_json::to_vec(&Action::capture(actual)).map_err(io::Error::other)?;
        let restored: Action = serde_json::from_slice(&wire).map_err(io::Error::other)?;
        assert_eq!(restored.restore()?, actual);
        assert!(
            Action {
                context: 0,
                name: "new-unregistered-action".into()
            }
            .restore()
            .is_err()
        );
        assert!(
            Action {
                context: u8::MAX,
                name: actual.action.into()
            }
            .restore()
            .is_err()
        );
        let voice = keymap_action_id("chat", "toggle_voice_mute")
            .ok_or_else(|| io::Error::other("actual voice action missing"))?;
        assert_eq!(voice.context, KeymapContext::Voice);
        assert_eq!(Action::capture(voice).restore()?, voice);
        // Chat shares a config name with Voice, but it is a different actual action context.
        assert!(
            Action {
                context: 1,
                name: voice.action.into()
            }
            .restore()
            .is_err()
        );
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum BindingKey {
    Physical { original: Key },
    Dispatch { action: Action, original: Key },
}
/// Only the actual known field's genuine trailing native dispatcher occurrence
/// is tagged. Every other physical code/order/duplicate stays byte-for-byte DATA.
pub(crate) fn capture_bindings(
    actual: &[KeyBinding],
    context: &str,
    name: &str,
) -> Vec<BindingKey> {
    let action = keymap_action_id(context, name);
    actual
        .iter()
        .copied()
        .enumerate()
        .map(|(index, value)| {
            match action.filter(|action| {
                index + 1 == actual.len() && dispatch_binding(*action) == Some(value)
            }) {
                Some(action) => BindingKey::Dispatch {
                    action: Action::capture(action),
                    original: Key::capture(value),
                },
                None => BindingKey::Physical {
                    original: Key::capture(value),
                },
            }
        })
        .collect()
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Dispatch {
    action: Action,
    token: Key,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Inventory {
    first: u8,
    entries: Vec<Dispatch>,
}
impl Inventory {
    pub(crate) fn capture() -> io::Result<Self> {
        let first = FIRST_DISPATCH_FUNCTION_KEY;
        let entries = keymap_action_ids()
            .filter_map(|action| {
                dispatch_binding(action).map(|token| Dispatch {
                    action: Action::capture(action),
                    token: Key::capture(token),
                })
            })
            .collect();
        let result = Self { first, entries };
        result.validate()?;
        Ok(result)
    }
    pub(crate) fn validate(&self) -> io::Result<()> {
        let invalid = || {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "native dispatch inventory shape/duplicate",
            )
        };
        if self.first == 0 || self.entries.len() > 256 {
            return Err(invalid());
        }
        let mut actions = std::collections::HashSet::new();
        for (index, entry) in self.entries.iter().enumerate() {
            let code = self
                .first
                .checked_add(u8::try_from(index).map_err(|_| invalid())?)
                .ok_or_else(invalid)?;
            if entry.action.context > 12
                || entry.action.name.is_empty()
                || entry.token.restore()?.parts() != (KeyCode::F(code), KeyModifiers::NONE)
                || !actions.insert((entry.action.context, entry.action.name.as_str()))
            {
                return Err(invalid());
            }
        }
        Ok(())
    }
    pub(crate) fn restore_bindings(
        &self,
        values: Vec<BindingKey>,
        context: &str,
        name: &str,
    ) -> io::Result<Vec<KeyBinding>> {
        let expected = keymap_action_id(context, name);
        let count = values.len();
        values
            .into_iter()
            .enumerate()
            .map(|(index, value)| match value {
                BindingKey::Physical { original } => original.restore(),
                BindingKey::Dispatch { action, original } => {
                    let invalid = || {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            "native dispatch occurrence changed its actual field/action",
                        )
                    };
                    let actual = action.clone().restore()?;
                    if index + 1 != count || expected != Some(actual) {
                        return Err(invalid());
                    }
                    let captured = self
                        .entries
                        .iter()
                        .find(|entry| entry.action == action)
                        .ok_or_else(invalid)?;
                    if captured.token.restore()? != original.restore()? {
                        return Err(invalid());
                    }
                    // Return the target's actual original token for SAME typed action.
                    // No event/action is run and no raw physical binding is normalized.
                    dispatch_binding(actual).ok_or_else(invalid)
                }
            })
            .collect()
    }
    #[cfg(test)]
    pub(crate) fn test_reorder(&mut self, a: KeymapActionId, b: KeymapActionId) -> io::Result<()> {
        let a = Action::capture(a);
        let b = Action::capture(b);
        let i = self
            .entries
            .iter()
            .position(|v| v.action == a)
            .ok_or_else(|| io::Error::other("actual first inventory action missing"))?;
        let j = self
            .entries
            .iter()
            .position(|v| v.action == b)
            .ok_or_else(|| io::Error::other("actual second inventory action missing"))?;
        let first = self.entries[i].action.clone();
        self.entries[i].action = self.entries[j].action.clone();
        self.entries[j].action = first;
        self.validate()
    }
    #[cfg(test)]
    pub(crate) fn test_retag(&self, values: &mut [BindingKey]) -> io::Result<()> {
        for value in values {
            if let BindingKey::Dispatch { action, original } = value {
                *original = self
                    .entries
                    .iter()
                    .find(|entry| entry.action == *action)
                    .ok_or_else(|| io::Error::other("actual inventory action missing"))?
                    .token;
            }
        }
        Ok(())
    }
}
