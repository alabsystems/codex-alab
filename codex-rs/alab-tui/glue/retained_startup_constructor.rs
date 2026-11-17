// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The original constructor's initial-message DATA owner. This retains the SAME
//! decoded E1/PreparedMessages; no re-decode, applied flag or Native permission.
use super::*;
use crate::chatwidget::UserMessage;
use crate::chatwidget::alab_retained_transfer::PreparedMessages;
use crate::owned_local_ui_transfer::NativeSection;
use crate::owned_local_ui_transfer::UiData;
use serde::Deserialize;
use serde::Serialize;
use std::sync::Mutex;
use std::sync::PoisonError;

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase", deny_unknown_fields)]
enum InitialMessageState {
    Pending { message: UserMessage },
    Empty {},
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct InitialMessageData {
    schema: u32,
    submission: InitialMessageState,
    #[serde(deserialize_with = "required_widget_messages")]
    widget_messages: Option<Box<serde_json::value::RawValue>>,
}
impl InitialMessageData {
    pub(crate) fn from_actual(message: &Option<UserMessage>) -> Self {
        Self {
            schema: 2,
            submission: match message {
                Some(message) => InitialMessageState::Pending {
                    message: message.clone(),
                },
                None => InitialMessageState::Empty {},
            },
            widget_messages: None, // The actual pre-App constructor has no widget yet.
        }
    }
    pub(crate) fn from_widget(
        widget: &crate::chatwidget::ChatWidget,
        initial: &Option<UserMessage>,
    ) -> io::Result<Self> {
        let mut data = Self::from_actual(initial);
        data.widget_messages = Some(widget.alab_capture_retained_messages_with_initial(initial)?);
        Ok(data)
    }
    fn prepared_widget(&self) -> io::Result<Option<PreparedMessages>> {
        self.widget_messages
            .as_ref()
            .map(|raw| {
                let prepared = PreparedMessages::decode(raw)?;
                if prepared.initial_message_for_constructor() != self.message() {
                    return Err(invalid(
                        "NoPrimary message graph differs from its original constructor state",
                    ));
                }
                Ok(prepared)
            })
            .transpose()
    }
    fn decode(actual: &serde_json::value::RawValue) -> io::Result<Self> {
        let data: Self = serde_json::from_str(actual.get()).map_err(io::Error::other)?;
        if data.schema != 2 {
            return Err(invalid("retained initial-message schema differs"));
        }
        drop(data.prepared_widget()?);
        Ok(data)
    }
    pub(crate) fn message_for_capture(&self) -> Option<UserMessage> {
        self.message()
    }
    fn message(&self) -> Option<UserMessage> {
        match &self.submission {
            InitialMessageState::Pending { message } => Some(message.clone()),
            InitialMessageState::Empty {} => None,
        }
    }
    pub(crate) fn section(&self) -> io::Result<NativeSection> {
        Ok(NativeSection::captured(
            serde_json::value::to_raw_value(self).map_err(io::Error::other)?,
        ))
    }
}
struct MessageGraph {
    prepared: Option<PreparedMessages>,
    installed: bool,
}
enum Messages {
    Original,
    Selected {
        initial: Option<UserMessage>,
        prepared: Mutex<MessageGraph>,
    },
    NoPrimary {
        data: InitialMessageData,
        prepared: Mutex<MessageGraph>,
    },
}
struct Current {
    data: InitialMessageData,
    cancelled_by_original_policy: bool,
    confirmation_cancelled_by_original_policy: bool,
}
pub(crate) struct Constructor {
    // The original reader owns SAME actual Arc<DecodedUiTransfer>/Entry/whole
    // E1. This is not a second read and does not mark any native graph installed.
    _actual: Option<Arc<RetainedFrontendReaders>>,
    messages: Messages,
    current: Mutex<Current>,
    early_startup: Mutex<Option<crate::startup_draft::alab_retained_transfer::early::Installed>>,
    configured_begin: Mutex<Option<Arc<configured_begin::Begin>>>,
    reply_owner: Mutex<Option<crate::app::RetainedReplyOwner>>,
}
impl Constructor {
    /// Call before the original startup's first await. Only the ordinary path
    /// evaluates its unchanged original CLI-message builder.
    pub(crate) fn prepare(original: impl FnOnce() -> Option<UserMessage>) -> io::Result<Arc<Self>> {
        let actual = active()?;
        let messages = if let Some(actual) = &actual {
            match actual.decoded().ui_data() {
                UiData::Selected { .. } => {
                    let prepared = actual.decoded().prepare_selected_messages()?;
                    Messages::Selected {
                        initial: prepared.initial_message_for_constructor(),
                        prepared: Mutex::new(MessageGraph {
                            prepared: Some(prepared),
                            installed: false,
                        }),
                    }
                }
                UiData::NoPrimary {
                    initial_message, ..
                } => {
                    let data = InitialMessageData::decode(initial_message.body())?;
                    let prepared = data.prepared_widget()?;
                    Messages::NoPrimary {
                        data,
                        prepared: Mutex::new(MessageGraph {
                            prepared,
                            installed: false,
                        }),
                    }
                }
            }
        } else {
            Messages::Original
        };
        let initial = choose(&messages, original);
        let owner = Arc::new(Self {
            _actual: actual.clone(),
            messages,
            early_startup: Mutex::new(None),
            configured_begin: Mutex::new(None),
            reply_owner: Mutex::new(None),
            current: Mutex::new(Current {
                data: InitialMessageData::from_actual(&initial),
                cancelled_by_original_policy: false,
                confirmation_cancelled_by_original_policy: false,
            }),
        });
        if let Some(actual) = &actual {
            super::register_prepared_constructor(&owner, actual)?;
        }
        Ok(owner)
    }
    pub(crate) fn direct_original(
        original: impl FnOnce() -> Option<UserMessage>,
    ) -> io::Result<Arc<Self>> {
        if active()?.is_some() {
            return Err(invalid(
                "entered startup lost its mandatory original constructor owner",
            ));
        }
        let initial = original();
        Ok(Arc::new(Self {
            _actual: None,
            messages: Messages::Original,
            early_startup: Mutex::new(None),
            configured_begin: Mutex::new(None),
            reply_owner: Mutex::new(None),
            current: Mutex::new(Current {
                data: InitialMessageData::from_actual(&initial),
                cancelled_by_original_policy: false,
                confirmation_cancelled_by_original_policy: false,
            }),
        }))
    }
    pub(crate) fn check_current_scope(&self) -> io::Result<()> {
        match (active()?, &self._actual) {
            (None, None) => Ok(()),
            (Some(actual), Some(original)) if Arc::ptr_eq(&actual, original) => Ok(()),
            _ => Err(invalid(
                "original constructor lost SAME actual entered reader/decoder scope",
            )),
        }
    }
    pub(crate) fn initial_message_for_constructor(
        &self,
        original: impl FnOnce() -> Option<UserMessage>,
    ) -> io::Result<Option<UserMessage>> {
        self.check_current_scope()?;
        if self
            .current
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .cancelled_by_original_policy
        {
            return Ok(None);
        }
        Ok(choose(&self.messages, original))
    }
    /// Install the complete actual message graph before configured hold or
    /// session-event drain. Borrowing the initial value did not install it.
    pub(crate) fn install_original_messages(
        &self,
        widget: &mut crate::chatwidget::ChatWidget,
    ) -> io::Result<()> {
        self.check_current_scope()?;
        let prepared = match &self.messages {
            Messages::Original => return Ok(()),
            Messages::Selected { prepared, .. } | Messages::NoPrimary { prepared, .. } => prepared,
        };
        let mut slot = prepared.lock().unwrap_or_else(PoisonError::into_inner);
        if slot.installed {
            return Err(invalid(
                "original message graph was already moved into its actual widget",
            ));
        }
        let Some(actual) = slot.prepared.as_ref() else {
            return Ok(());
        }; // Actual pre-App source has no widget graph.
        let cancel_confirmation = self
            .current
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .confirmation_cancelled_by_original_policy;
        // The original constructor's post-filter value is authoritative. It
        // may be None without a separate explicit cancel callback.
        actual.validate_original_policy_destination(widget)?;
        let actual = slot
            .prepared
            .take()
            .ok_or_else(|| invalid("original message graph lost before synchronous install"))?;
        actual.install_after_original_policy(widget, cancel_confirmation)?;
        slot.installed = true;
        Ok(())
    }
    /// Only an original startup cancellation/edit/policy branch records this.
    /// The flag can remove confirmation DATA; it cannot authorize submission.
    pub(crate) fn cancel_original_confirmation(&self) {
        self.current
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .confirmation_cancelled_by_original_policy = true;
    }
    /// Record the original constructor's actual post-filter value BEFORE the
    /// configured holder can take it. This is custody DATA, never UI-applied.
    pub(crate) fn constructed(&self, actual: &Option<UserMessage>) -> io::Result<()> {
        self.check_current_scope()?;
        self.current
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .data = InitialMessageData::from_actual(actual);
        Ok(())
    }
    /// Called at the original actual consent/cancel branch, not from argv or
    /// restoration guesses. It must not revive a captured consumed prompt.
    pub(crate) fn cancel_from_original_policy(&self) -> io::Result<()> {
        self.check_current_scope()?;
        let mut current = self.current.lock().unwrap_or_else(PoisonError::into_inner);
        current.data = InitialMessageData::from_actual(&None);
        current.cancelled_by_original_policy = true;
        Ok(())
    }
    pub(crate) fn capture(&self) -> io::Result<InitialMessageData> {
        self.check_current_scope()?;
        Ok(self
            .current
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .data
            .clone())
    }
}
fn choose(
    messages: &Messages,
    original: impl FnOnce() -> Option<UserMessage>,
) -> Option<UserMessage> {
    match messages {
        Messages::Original => original(),
        Messages::Selected { initial, .. } => initial.clone(),
        Messages::NoPrimary { data, .. } => data.message(),
    }
}
#[cfg(test)]
fn selected_message(
    actual: &PreparedMessages,
    _original: impl FnOnce() -> Option<UserMessage>,
) -> Option<UserMessage> {
    actual.initial_message_for_constructor()
}
fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

#[cfg(test)]
pub(crate) fn selected_message_for_tests(
    actual: &PreparedMessages,
    original: impl FnOnce() -> Option<UserMessage>,
) -> Option<UserMessage> {
    selected_message(actual, original)
}
#[cfg(test)]
pub(crate) fn no_primary_message_for_tests(
    raw: &serde_json::value::RawValue,
    original: impl FnOnce() -> Option<UserMessage>,
) -> io::Result<Option<UserMessage>> {
    Ok(choose(
        &Messages::NoPrimary {
            data: InitialMessageData::decode(raw)?,
            prepared: Mutex::new(MessageGraph {
                prepared: None,
                installed: false,
            }),
        },
        original,
    ))
}
#[cfg(test)]
pub(crate) fn original_message_for_tests(
    original: impl FnOnce() -> Option<UserMessage>,
) -> Option<UserMessage> {
    choose(&Messages::Original, original)
}

#[path = "retained_callback_constructor.rs"]
mod callbacks;

#[path = "retained_startup_early_constructor.rs"]
mod early_startup;

#[path = "retained_callback_startup_constructor.rs"]
mod startup_callbacks;

#[path = "retained_startup_configured_begin.rs"]
pub(crate) mod configured_begin;

#[path = "retained_startup_reply_owner.rs"]
mod reply_owner;

fn required_widget_messages<'de, D: serde::Deserializer<'de>>(
    decoder: D,
) -> Result<Option<Box<serde_json::value::RawValue>>, D::Error> {
    Option::<Box<serde_json::value::RawValue>>::deserialize(decoder)
}

#[cfg(test)]
impl InitialMessageData {
    pub(crate) fn widget_messages_for_tests(&self) -> Option<&serde_json::value::RawValue> {
        self.widget_messages.as_deref()
    }
}
