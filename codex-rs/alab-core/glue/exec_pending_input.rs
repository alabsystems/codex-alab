// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Private accepted-input representation, never a provider or rollout format.
use super::TurnInput;
use super::UserInputMetadata;
use crate::PendingAgentMessage;
use crate::agent_communication::ExecPublication;
use crate::agent_communication::MessageDelivery;
use crate::session::alab_exec_recovery_options::ephemeral::codec::MemoryItem;
use codex_protocol::protocol::InterAgentCommunication;
use codex_protocol::user_input::UserInput;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::collections::HashSet;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExecPendingInputs {
    version: u32,
    items: Vec<SavedInput>,
    publications: Vec<ExecPublication>,
    group_ends: Vec<usize>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum SavedInput {
    User {
        content: Vec<UserInput>,
        client_id: Option<String>,
        metadata: UserInputMetadata,
    },
    FunctionOutput {
        item: MemoryItem,
    },
    Response {
        item: MemoryItem,
    },
    Mail {
        communication: InterAgentCommunication,
        publication: Option<usize>,
    },
}

impl SavedInput {
    fn publication(&self) -> Option<usize> {
        match self {
            Self::Mail { publication, .. } => *publication,
            _ => None,
        }
    }
}

impl ExecPendingInputs {
    pub(crate) fn capture(input: &[TurnInput]) -> Result<Self, String> {
        Self::capture_groups(&[input])
    }

    /// Keep one publication table across every thread's queues. Encoding each
    /// thread separately would split shared delivery/consumption identity.
    pub(crate) fn capture_groups(groups: &[&[TurnInput]]) -> Result<Self, String> {
        let mut publications = Vec::new();
        let mut identities = HashMap::new();
        let mut items = Vec::new();
        let mut group_ends = Vec::with_capacity(groups.len());
        for input in groups {
            for item in *input {
                items.push(match item {
                    TurnInput::UserInput {
                        content,
                        client_id,
                        metadata,
                    } => SavedInput::User {
                        content: content.clone(),
                        client_id: client_id.clone(),
                        metadata: metadata.clone(),
                    },
                    TurnInput::FunctionCallOutput(item) => SavedInput::FunctionOutput {
                        item: MemoryItem::capture(item)?,
                    },
                    TurnInput::ResponseItem(item) => SavedInput::Response {
                        item: MemoryItem::capture(item)?,
                    },
                    TurnInput::InterAgentCommunication(mail) => {
                        let publication = if let Some(delivery) = &mail.delivery {
                            let (identity, saved) = delivery.capture_exec()?;
                            if let Some(&index) = identities.get(&identity) {
                                if publications.get(index) != Some(&saved) {
                                    return Err("message publication changed during capture".into());
                                }
                                Some(index)
                            } else {
                                let index = publications.len();
                                identities.insert(identity, index);
                                publications.push(saved);
                                Some(index)
                            }
                        } else {
                            None
                        };
                        SavedInput::Mail {
                            communication: (**mail).clone(),
                            publication,
                        }
                    }
                });
            }
            group_ends.push(items.len());
        }
        let saved = Self {
            version: 5,
            items,
            publications,
            group_ends,
        };
        let bytes =
            serde_json::to_vec(&saved).map_err(|_| "accepted input could not be encoded")?;
        let decoded: Self =
            serde_json::from_slice(&bytes).map_err(|_| "accepted input could not be decoded")?;
        let restored = decoded.restore_groups()?;
        if restored.len() != groups.len()
            || restored
                .iter()
                .zip(groups)
                .any(|(restored, original)| restored.as_slice() != *original)
        {
            return Err("accepted input encoding would discard content or metadata".into());
        }
        Ok(saved)
    }

    pub(crate) fn restore(self) -> Result<Vec<TurnInput>, String> {
        let mut groups = self.restore_groups()?;
        if groups.len() != 1 {
            return Err("single-thread input requires exactly one queue group".into());
        }
        Ok(groups.remove(0))
    }

    pub(crate) fn select_groups(&self, selected: &[usize]) -> Result<Self, String> {
        self.clone().restore_groups()?;
        let mut items = Vec::new();
        let mut ends = Vec::new();
        let mut publications = Vec::new();
        let mut rows = HashMap::new();
        let mut included = HashSet::new();
        let mut selected_results = HashSet::new();
        for &index in selected {
            if !included.insert(index) {
                return Err("input group selection repeats a group".into());
            }
            let end = *self
                .group_ends
                .get(index)
                .ok_or("input group selection is out of bounds")?;
            let start = if index == 0 {
                0
            } else {
                self.group_ends[index - 1]
            };
            for mut item in self.items[start..end].iter().cloned() {
                if let SavedInput::Mail {
                    publication: Some(row),
                    ..
                } = &mut item
                {
                    let old = *row;
                    *row = *rows.entry(old).or_insert_with(|| {
                        let index = publications.len();
                        let publication = &self.publications[old];
                        if matches!(publication, ExecPublication::QueuedResult { .. }) {
                            selected_results.insert(old);
                        }
                        publications.push(publication.clone());
                        index
                    });
                }
                items.push(item);
            }
            ends.push(items.len());
        }
        // A queued writer belongs to one tree's runtime. Reject a cross-tree
        // alias before either runtime can accept that publication.
        if !selected_results.is_empty() {
            let mut start = 0;
            for (group, &end) in self.group_ends.iter().enumerate() {
                if !included.contains(&group)
                    && self.items[start..end]
                        .iter()
                        .filter_map(SavedInput::publication)
                        .any(|row| selected_results.contains(&row))
                {
                    return Err("queued result writer is shared between destination trees".into());
                }
                start = end;
            }
        }
        Ok(Self {
            version: self.version,
            items,
            publications,
            group_ends: ends,
        })
    }

    pub(crate) fn restore_groups(self) -> Result<Vec<Vec<TurnInput>>, String> {
        if !matches!(self.version, 2 | 5) {
            return Err("unsupported accepted-input snapshot version".into());
        }
        let mut queued_scopes = HashSet::new();
        let mut queued_indices = HashSet::new();
        for (index, publication) in self.publications.iter().enumerate() {
            if let ExecPublication::QueuedResult { scope_id, .. } = publication {
                if self.version != 5 {
                    return Err(
                        "older accepted-input format cannot carry a queued result writer".into(),
                    );
                }
                if !queued_scopes.insert(*scope_id) {
                    return Err("accepted input repeats a queued result publication".into());
                }
                queued_indices.insert(index);
            }
        }
        if !queued_indices.is_empty() {
            if self.publications.iter().any(|publication| {
                matches!(publication, ExecPublication::Pending { scope_id, .. } if queued_scopes.contains(scope_id))
            }) {
                return Err("queued result scope also appears as ordinary pending mail".into());
            }
            for index in self.items.iter().filter_map(SavedInput::publication) {
                queued_indices.remove(&index);
            }
            if !queued_indices.is_empty() {
                return Err("queued result publication has no receiving mail".into());
            }
        }
        let mut previous = 0;
        for end in &self.group_ends {
            if *end < previous || *end > self.items.len() {
                return Err("accepted-input group boundaries are invalid".into());
            }
            previous = *end;
        }
        if previous != self.items.len() {
            return Err("accepted-input groups do not cover the stored items".into());
        }
        let publications: Vec<_> = self
            .publications
            .into_iter()
            .map(MessageDelivery::restore_exec)
            .collect();
        let items = self
            .items
            .into_iter()
            .map(|item| {
                Ok(match item {
                    SavedInput::User {
                        content,
                        client_id,
                        metadata,
                    } => TurnInput::UserInput {
                        content,
                        client_id,
                        metadata,
                    },
                    SavedInput::FunctionOutput { item } => {
                        TurnInput::FunctionCallOutput(item.restore()?)
                    }
                    SavedInput::Response { item } => TurnInput::ResponseItem(item.restore()?),
                    SavedInput::Mail {
                        communication,
                        publication,
                    } => {
                        let delivery = publication
                            .map(|index| {
                                publications.get(index).cloned().ok_or_else(|| {
                                    "accepted input has an invalid publication reference".to_owned()
                                })
                            })
                            .transpose()?;
                        TurnInput::InterAgentCommunication(PendingAgentMessage::new(
                            communication,
                            delivery,
                        ))
                    }
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let mut items = items.into_iter();
        let mut previous = 0;
        Ok(self
            .group_ends
            .into_iter()
            .map(|end| {
                let group = items.by_ref().take(end - previous).collect();
                previous = end;
                group
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_history::CodexHarnessMetadata;
    use codex_history::ResponseItemEnvelope;
    use codex_protocol::models::FunctionCallOutputPayload;
    use codex_protocol::models::ResponseItem;

    #[test]
    fn accepted_input_retains_order_provenance_and_completed_tool_failure() {
        let mut output = FunctionCallOutputPayload::from_text("already completed".into());
        output.success = Some(false);
        let input = vec![
            TurnInput::UserInput {
                content: vec![UserInput::Text {
                    text: "accepted steer".into(),
                    text_elements: vec![],
                }],
                client_id: Some("original-client".into()),
                metadata: UserInputMetadata {
                    acceptance_order: Some(17),
                    response_item_uuid: Some(uuid::Uuid::from_u128(17)),
                    ..Default::default()
                },
            },
            TurnInput::FunctionCallOutput(ResponseItemEnvelope {
                item: ResponseItem::FunctionCallOutput {
                    id: None,
                    call_id: Some("completed-call".into()),
                    name: Some("completed-tool".into()),
                    namespace: None,
                    output,
                    internal_chat_message_metadata_passthrough: None,
                },
                metadata: Some(CodexHarnessMetadata {
                    user_input_order: Some(18),
                    ..Default::default()
                }),
            }),
        ];
        assert!(
            serde_json::to_vec(&input).is_err(),
            "exercise the annotated-input boundary"
        );
        let saved = ExecPendingInputs::capture(&input).unwrap();
        let decoded: ExecPendingInputs =
            serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
        assert_eq!(decoded.restore().unwrap(), input);
    }

    #[test]
    fn unsupported_version_is_rejected() {
        let mut saved = ExecPendingInputs::capture(&[]).unwrap();
        saved.version = 3;
        assert!(saved.restore().is_err());
    }

    fn mail() -> InterAgentCommunication {
        InterAgentCommunication::new(
            codex_protocol::AgentPath::root(),
            codex_protocol::AgentPath::root(),
            Vec::new(),
            "accepted mail".into(),
            false,
        )
    }

    #[test]
    fn duplicated_deliveries_keep_shared_consumption_without_touching_the_source() {
        let delivery =
            MessageDelivery::restore_legacy_pending_for_test(codex_protocol::ThreadId::new());
        let pending = PendingAgentMessage::new(mail(), Some(delivery.clone()));
        let source = vec![
            TurnInput::InterAgentCommunication(pending.clone()),
            TurnInput::InterAgentCommunication(pending),
        ];
        let saved = ExecPendingInputs::capture(&source).unwrap();
        assert_eq!(saved.publications.len(), 1);
        let saved: ExecPendingInputs =
            serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
        let restored = saved.restore().unwrap();
        let TurnInput::InterAgentCommunication(first) = &restored[0] else {
            panic!("first mail")
        };
        let TurnInput::InterAgentCommunication(second) = &restored[1] else {
            panic!("second mail")
        };
        first
            .delivery
            .as_ref()
            .unwrap()
            .consumed(codex_protocol::ThreadId::new());
        assert!(matches!(
            second.delivery.as_ref().unwrap().capture_exec().unwrap().1,
            ExecPublication::Consumed
        ));
        assert!(matches!(
            delivery.capture_exec().unwrap().1,
            ExecPublication::Pending { .. }
        ));
    }

    #[test]
    fn invalid_publication_reference_is_rejected() {
        let saved = ExecPendingInputs {
            version: 2,
            items: vec![SavedInput::Mail {
                communication: mail(),
                publication: Some(0),
            }],
            publications: Vec::new(),
            group_ends: vec![1],
        };
        assert!(saved.restore().is_err());
    }

    #[test]
    fn different_threads_keep_shared_delivery_identity_without_merging_distinct_deliveries() {
        let sender = codex_protocol::ThreadId::new();
        let shared = MessageDelivery::restore_legacy_pending_for_test(sender);
        // A distinct publication may carry the same visible scope. Deduplication
        // must follow the live delivery owner, not equality of its serialized ID.
        let distinct = MessageDelivery::restore_exec(shared.capture_exec().unwrap().1);
        let first = vec![TurnInput::InterAgentCommunication(
            PendingAgentMessage::new(mail(), Some(shared.clone())),
        )];
        let second = first.clone();
        let third = vec![TurnInput::InterAgentCommunication(
            PendingAgentMessage::new(mail(), Some(distinct)),
        )];
        let saved = ExecPendingInputs::capture_groups(&[&first, &[], &second, &third]).unwrap();
        assert_eq!(saved.publications.len(), 2);
        let decoded: ExecPendingInputs =
            serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
        let groups = decoded.restore_groups().unwrap();
        assert_eq!(groups.len(), 4);
        assert!(groups[1].is_empty());
        let delivery = |group: usize| {
            let TurnInput::InterAgentCommunication(mail) = &groups[group][0] else {
                panic!("mail")
            };
            mail.delivery.as_ref().unwrap()
        };
        delivery(0).consumed(sender);
        assert_eq!(
            delivery(2).capture_exec().unwrap().1,
            ExecPublication::Consumed
        );
        assert!(matches!(
            delivery(3).capture_exec().unwrap().1,
            ExecPublication::Pending { .. }
        ));
        assert!(matches!(
            shared.capture_exec().unwrap().1,
            ExecPublication::Pending { .. }
        ));
        assert!(
            saved.restore().is_err(),
            "a root-only reader must not discard child groups"
        );
    }

    #[test]
    fn queue_group_boundaries_must_be_ordered_and_cover_every_item() {
        let item = vec![TurnInput::InterAgentCommunication(
            PendingAgentMessage::new(mail(), None),
        )];
        let saved = ExecPendingInputs::capture_groups(&[&item, &[]]).unwrap();
        for ends in [vec![1, 0], vec![0], vec![2], Vec::new()] {
            let mut invalid = saved.clone();
            invalid.group_ends = ends;
            assert!(invalid.restore_groups().is_err());
        }
        assert!(
            ExecPendingInputs::capture_groups(&[])
                .unwrap()
                .restore_groups()
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn private_client_input_writer_emits_the_forward_compatibility_fixture() {
        let fixture = include_bytes!("exec_queued_client_v5.json");
        let saved: ExecPendingInputs = serde_json::from_slice(fixture).unwrap();
        let input = saved.restore().unwrap();
        let [
            TurnInput::UserInput { .. },
            TurnInput::InterAgentCommunication(mail),
        ] = input.as_slice()
        else {
            panic!("the fixture must include both input records");
        };
        let runtime = crate::agent::control::LocalAgentControl::default().runtime;
        runtime
            .restore_exec_accepted_mail(Vec::new(), std::iter::once(mail), false)
            .unwrap();
        let _owner = crate::agent_communication::ExecResultGrant::prepare(
            runtime.exec_result_counter(),
            std::iter::once(mail),
        )
        .unwrap();
        let encoded = serde_json::to_vec(&ExecPendingInputs::capture(&input).unwrap()).unwrap();
        assert_eq!(encoded.as_slice(), fixture.strip_suffix(b"\n").unwrap());
    }

    #[test]
    fn selecting_a_tree_retains_only_its_own_complete_result_writers() {
        let sender = codex_protocol::ThreadId::new();
        let publication = || ExecPublication::QueuedResult {
            sender,
            scope_id: uuid::Uuid::now_v7(),
        };
        let message = |row| SavedInput::Mail {
            communication: InterAgentCommunication::new(
                codex_protocol::AgentPath::root(),
                codex_protocol::AgentPath::root(),
                Vec::new(),
                "queued".into(),
                false,
            ),
            publication: Some(row),
        };
        let saved = ExecPendingInputs {
            version: 5,
            items: vec![message(0), message(1), message(1)],
            publications: vec![publication(), publication()],
            group_ends: vec![1, 2, 3],
        };
        let projected = saved.select_groups(&[2, 1]).unwrap();
        assert_eq!(projected.publications, [saved.publications[1].clone()]);
        assert!(projected.items.iter().all(|item| matches!(
            item,
            SavedInput::Mail {
                publication: Some(0),
                ..
            }
        )));
        assert_eq!(projected.restore_groups().unwrap().len(), 2);
        assert_eq!(
            saved
                .select_groups(&[0])
                .unwrap()
                .restore_groups()
                .unwrap()
                .len(),
            1
        );
        assert!(saved.select_groups(&[1]).is_err());
        assert!(saved.select_groups(&[1, 1]).is_err());
    }

    #[test]
    fn queued_result_write_requires_its_own_input_format() {
        let sender = codex_protocol::ThreadId::new();
        let scope_id = uuid::Uuid::now_v7();
        let mail = InterAgentCommunication::new(
            codex_protocol::AgentPath::root(),
            codex_protocol::AgentPath::root(),
            Vec::new(),
            "queued".into(),
            false,
        );
        let mut saved = ExecPendingInputs {
            version: 5,
            items: vec![SavedInput::Mail {
                communication: mail,
                publication: Some(0),
            }],
            publications: vec![ExecPublication::QueuedResult { sender, scope_id }],
            group_ends: vec![1],
        };
        for old in [2, 3, 4] {
            saved.version = old;
            assert!(saved.clone().restore_groups().is_err());
        }
        saved.version = 5;
        let decoded: ExecPendingInputs =
            serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
        let items = decoded.restore().unwrap();
        let [TurnInput::InterAgentCommunication(mail)] = items.as_slice() else {
            panic!("missing queued result")
        };
        assert!(
            !mail.is_terminal_result(),
            "plain decoding creates no writer"
        );
        assert!(mail.delivery.as_ref().unwrap().is_restored_queued_result());
        let mut unreferenced = saved.clone();
        let SavedInput::Mail { publication, .. } = &mut unreferenced.items[0] else {
            panic!("missing queued mail")
        };
        *publication = None;
        assert!(unreferenced.restore_groups().is_err());
        let mut conflicting = saved.clone();
        conflicting
            .publications
            .push(ExecPublication::Pending { sender, scope_id });
        assert!(conflicting.restore_groups().is_err());
        saved.publications.push(saved.publications[0].clone());
        assert!(saved.restore_groups().is_err());
    }
}
