// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Rich payload keys reconnect to native strong register generations.
use super::*;
use codex_alab_tui::upgrade_editor_graph_scope as graph;
use serde::Deserialize;
use serde::Serialize;

crate::alab_editor_upgrade::native_state!(
    super::RegisterPayload,
    RegisterPayloadWire,
    #[serde(remote = "super::RegisterPayload", deny_unknown_fields)]
    enum RegisterPayloadWire {
        Mention(ComposerMentionBinding),
        Image(PathBuf),
        Paste(String),
    }
);
crate::alab_editor_upgrade::native_state!(
    super::KillBufferSnapshot,
    KillBufferSnapshotWire,
    #[serde(remote = "super::KillBufferSnapshot", deny_unknown_fields)]
    struct KillBufferSnapshotWire {
        textarea: crate::bottom_pane::textarea::KillBufferSnapshot,
        payloads: HashMap<u64, RegisterPayload>,
    }
);

#[derive(Serialize)]
struct GenerationRef<'a> {
    #[serde(with = "graph::weak")]
    generation: Weak<Vec<TextElementSnapshot>>,
    payloads: &'a HashMap<u64, RegisterPayload>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Generation {
    #[serde(with = "graph::weak")]
    generation: Weak<Vec<TextElementSnapshot>>,
    payloads: HashMap<u64, RegisterPayload>,
}
impl Serialize for VimRegister {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Native sync prunes dead generations. Do not mutate the source to do so.
        let entries: Vec<_> = self
            .payloads
            .iter()
            .filter(|(generation, _)| generation.strong_count() > 0)
            .map(|(generation, payloads)| GenerationRef {
                generation: generation.clone(),
                payloads,
            })
            .collect();
        entries.serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for VimRegister {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let entries = Vec::<Generation>::deserialize(deserializer)?;
        let mut payloads = Vec::with_capacity(entries.len());
        for entry in entries {
            let elements = entry
                .generation
                .upgrade()
                .ok_or_else(|| serde::de::Error::custom("rich register generation has no owner"))?;
            let ids: std::collections::HashSet<_> =
                elements.iter().map(|element| element.id).collect();
            if entry.payloads.keys().any(|id| !ids.contains(id)) {
                return Err(serde::de::Error::custom(
                    "rich register payload references an absent element",
                ));
            }
            payloads.push((entry.generation, entry.payloads));
        }
        Ok(Self { payloads })
    }
}
