// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Selection data only; live clipboard work requires its own ownership transfer.
use super::*;
use serde::Deserialize;
use serde::Serialize;

#[derive(Serialize, Deserialize)]
enum Unit {
    Character,
    Word,
    Line,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    origin: Range<usize>,
    unit: Unit,
    dragging: bool,
    moved: bool,
}
impl Serialize for MouseSelection {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.pending_copy.is_some() || self.primary_owner.is_some() {
            return Err(serde::ser::Error::custom(
                "editor clipboard owner has not been transferred",
            ));
        }
        Selection {
            origin: self.origin.clone(),
            unit: match self.unit {
                SelectionUnit::Character => Unit::Character,
                SelectionUnit::Word => Unit::Word,
                SelectionUnit::Line => Unit::Line,
            },
            dragging: self.dragging,
            moved: self.moved,
        }
        .serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for MouseSelection {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = Selection::deserialize(deserializer)?;
        if wire.origin.start > wire.origin.end {
            return Err(serde::de::Error::custom("reversed editor selection"));
        }
        Ok(Self {
            origin: wire.origin,
            unit: match wire.unit {
                Unit::Character => SelectionUnit::Character,
                Unit::Word => SelectionUnit::Word,
                Unit::Line => SelectionUnit::Line,
            },
            dragging: wire.dragging,
            moved: wire.moved,
            pending_copy: None,
            primary_owner: None,
        })
    }
}
impl MouseSelection {
    pub(in crate::bottom_pane) fn alab_validate_editor_upgrade(
        &self,
        text: &str,
    ) -> Result<(), &'static str> {
        if text.get(self.origin.clone()).is_none() {
            return Err("editor selection outside text boundaries");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_mouse_codec_refuses_live_clipboard_work_without_mutating_ownership() {
        let mut selection = MouseSelection {
            pending_copy: Some((7, 0..1)),
            primary_owner: None,
            origin: 0..1,
            unit: SelectionUnit::Character,
            dragging: false,
            moved: true,
        };
        assert!(serde_json::to_vec(&selection).is_err());
        assert_eq!(selection.pending_copy, Some((7, 0..1)));
        selection.pending_copy = None;
        let owner = Arc::new(());
        selection.primary_owner = Some(owner.clone());
        assert!(serde_json::to_vec(&selection).is_err());
        assert!(Arc::ptr_eq(
            selection.primary_owner.as_ref().unwrap(),
            &owner
        ));
        selection.primary_owner = None;
        let restored: MouseSelection =
            serde_json::from_slice(&serde_json::to_vec(&selection).unwrap()).unwrap();
        assert_eq!(restored.origin, selection.origin);
        assert!(restored.primary_owner.is_none());
        assert!(restored.pending_copy.is_none());
    }
}
