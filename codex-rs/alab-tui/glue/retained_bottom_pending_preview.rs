// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::key_hint::alab_retained_keys::Hint;
use serde::Deserialize;
use serde::Serialize;
use std::io;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Data {
    pending: Vec<String>,
    rejected: Vec<String>,
    queued: Vec<String>,
    #[serde(deserialize_with = "required")]
    edit: Option<Hint>,
    #[serde(deserialize_with = "required")]
    interrupt: Option<Hint>,
}
impl Data {
    pub(crate) fn capture(v: &PendingInputPreview) -> Self {
        Self {
            pending: v.pending_steers.clone(),
            rejected: v.rejected_steers.clone(),
            queued: v.queued_messages.clone(),
            edit: v.edit_binding.map(Hint::capture),
            interrupt: v.interrupt_binding.map(Hint::capture),
        }
    }
    pub(crate) fn restore(self) -> io::Result<PendingInputPreview> {
        Ok(PendingInputPreview {
            alab_recovery: alab_queue_review::Recovery::Unknown,
            pending_steers: self.pending,
            rejected_steers: self.rejected,
            queued_messages: self.queued,
            edit_binding: self.edit.map(Hint::restore).transpose()?,
            interrupt_binding: self.interrupt.map(Hint::restore).transpose()?,
        })
    }
}
fn required<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(d)
}
#[cfg(test)]
mod controls {
    use super::*;
    #[test]
    fn saved_preview_waits_for_the_authoritative_queue_to_supply_delivery_state() -> io::Result<()>
    {
        let saved = serde_json::json!({
            "pending": ["unconfirmed text"],
            "rejected": ["returned text"],
            "queued": ["follow-up text"],
            "edit": null,
            "interrupt": null
        });
        let preview = serde_json::from_value::<Data>(saved.clone())?.restore()?;
        assert_eq!(serde_json::to_value(Data::capture(&preview))?, saved);
        let area = Rect::new(0, 0, 80, preview.desired_height(80));
        let mut buffer = Buffer::empty(area);
        preview.render(area, &mut buffer);
        let text = format!("{buffer:?}");
        for label in ["unconfirmed text", "returned text", "follow-up text"] {
            assert!(text.contains(label), "{text}");
        }
        assert!(!text.contains("to be submitted"));
        assert!(!text.contains("automatic sending paused"));
        assert!(!text.contains("send immediately"));
        Ok(())
    }
    #[test]
    fn original_pending_rejected_and_queued_sections_and_exact_keys_survive() -> io::Result<()> {
        let mut original = PendingInputPreview::new();
        original.pending_steers = vec!["pending".into()];
        original.rejected_steers = vec!["rejected".into()];
        original.queued_messages = vec!["queued".into()];
        original.set_edit_binding(None);
        let restored = Data::capture(&original).restore()?;
        assert_eq!(restored.pending_steers, original.pending_steers);
        assert_eq!(restored.rejected_steers, original.rejected_steers);
        assert_eq!(restored.queued_messages, original.queued_messages);
        assert_eq!(restored.edit_binding, original.edit_binding);
        assert_eq!(restored.interrupt_binding, original.interrupt_binding);
        Ok(())
    }
}
