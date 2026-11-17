// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Native editor data only; these projections neither commit nor restore a UI.
use super::*;

crate::alab_editor_upgrade::native_state!(
    super::RegisterElements,
    RegisterElementsWire,
    #[serde(remote = "super::RegisterElements", deny_unknown_fields)]
    struct RegisterElementsWire {
        #[serde(with = "codex_alab_tui::upgrade_editor_graph_scope::strong")]
        elements: Arc<Vec<TextElementSnapshot>>,
        event: Option<RegisterEvent>,
    }
);

crate::alab_editor_upgrade::native_state!(
    super::RegisterEvent,
    RegisterEventWire,
    #[serde(remote = "super::RegisterEvent", deny_unknown_fields)]
    enum RegisterEventWire {
        Copied(
            #[serde(with = "codex_alab_tui::upgrade_editor_graph_scope::strong")]
            Arc<Vec<TextElementSnapshot>>,
        ),
        Pasted(
            #[serde(with = "codex_alab_tui::upgrade_editor_graph_scope::strong")]
            Arc<Vec<TextElementSnapshot>>,
            Vec<(u64, u64)>,
        ),
    }
);

crate::alab_editor_upgrade::native_state!(
    super::RegisterSnapshot,
    RegisterSnapshotWire,
    #[serde(remote = "super::RegisterSnapshot", deny_unknown_fields)]
    struct RegisterSnapshotWire {
        text: String,
        #[serde(with = "codex_alab_tui::upgrade_editor_graph_scope::strong")]
        elements: Arc<Vec<TextElementSnapshot>>,
    }
);

#[cfg(test)]
mod tests {
    use super::*;
    use codex_alab_tui::upgrade_editor_graph_scope as graph;
    use serde::Deserialize;
    use serde::Serialize;
    use std::sync::Weak;

    #[derive(Serialize, Deserialize)]
    struct NativeRegister {
        // Payload keys intentionally precede strong roots.
        #[serde(with = "graph::weak")]
        payload: Weak<Vec<TextElementSnapshot>>,
        live: RegisterElements,
        recorded_yank: RegisterSnapshot,
    }

    #[test]
    fn native_register_event_and_recorded_yank_keep_their_rich_payload_generation() {
        let mut textarea = TextArea::new();
        textarea.set_text_clearing_elements("[image]");
        textarea.add_element_range(0..7).unwrap();
        super::super::copy(&mut textarea, 0..7);
        let first = Arc::clone(&textarea.register_elements.elements);
        let recorded_yank = RegisterSnapshot {
            text: "[image]".into(),
            elements: first.clone(),
        };
        // A later identical copy must have a different rich payload identity.
        super::super::copy(&mut textarea, 0..7);
        let source = NativeRegister {
            payload: Arc::downgrade(&first),
            live: std::mem::take(&mut textarea.register_elements),
            recorded_yank,
        };
        let bytes = graph::encode::<Vec<TextElementSnapshot>>(&source, 8192).unwrap();
        let restored: NativeRegister =
            graph::decode::<Vec<TextElementSnapshot>, _>(&bytes, 8192, 2).unwrap();
        assert!(Weak::ptr_eq(
            &restored.payload,
            &Arc::downgrade(&restored.recorded_yank.elements)
        ));
        assert!(!Weak::ptr_eq(
            &restored.payload,
            &Arc::downgrade(&restored.live.elements)
        ));
        let Some(RegisterEvent::Copied(event)) = restored.live.event else {
            panic!("native copy event lost");
        };
        assert!(Arc::ptr_eq(&event, &restored.live.elements));
        assert_eq!(restored.recorded_yank.elements.as_ref(), first.as_ref());
    }
}
