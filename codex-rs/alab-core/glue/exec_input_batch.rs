// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Per-thread queue metadata over one shared tree-wide input encoding.
//! Preparation owns data only; every turn still needs separate live admission.
use super::*;
use std::collections::HashMap;
use std::collections::HashSet;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct InputMetadata {
    version: u32,
    thread: ThreadId,
    turn: String,
    turn_count: usize,
    notified: Option<Activity>,
    accepts_mail: bool,
    mailbox_options: Vec<SavedStart>,
    accounting: SavedTurnAccounting,
    controller_routes: Vec<ControllerRoute>,
}

impl From<&ExecInputSnapshot> for InputMetadata {
    fn from(value: &ExecInputSnapshot) -> Self {
        let ExecInputSnapshot {
            version,
            thread,
            turn,
            input: _,
            turn_count,
            notified,
            accepts_mail,
            mailbox_options,
            accounting,
            controller_routes,
        } = value;
        Self {
            version: *version,
            thread: *thread,
            turn: turn.clone(),
            turn_count: *turn_count,
            notified: *notified,
            accepts_mail: *accepts_mail,
            mailbox_options: mailbox_options.clone(),
            accounting: accounting.clone(),
            controller_routes: controller_routes.clone(),
        }
    }
}

impl InputMetadata {
    pub(super) fn with_input(self, input: ExecPendingInputs) -> ExecInputSnapshot {
        let Self {
            version,
            thread,
            turn,
            turn_count,
            notified,
            accepts_mail,
            mailbox_options,
            accounting,
            controller_routes,
        } = self;
        ExecInputSnapshot {
            version,
            thread,
            turn,
            input,
            turn_count,
            notified,
            accepts_mail,
            mailbox_options,
            accounting,
            controller_routes,
        }
    }

    pub(super) fn decode(
        &self,
        thread: ThreadId,
        turn: &str,
        mut input: Vec<TurnInput>,
    ) -> Result<Prepared, String> {
        if self.version != 4 || self.thread != thread || self.turn != turn {
            return Err("accepted input belongs to another runtime or turn".into());
        }
        if self.turn_count > input.len()
            || input.len() - self.turn_count
                != self
                    .mailbox_options
                    .len()
                    .saturating_add(self.controller_routes.len())
        {
            return Err("accepted input queue boundaries are invalid".into());
        }
        let controller = input
            .split_off(self.turn_count + self.mailbox_options.len())
            .into_iter()
            .zip(self.controller_routes.iter())
            .map(|(item, route)| match item {
                TurnInput::InterAgentCommunication(communication) => {
                    Ok(crate::agent::control::ExecMailboxMessage {
                        thread: route.thread,
                        receive_id: route.receive_id.clone(),
                        communication,
                    })
                }
                _ => Err("controller mailbox contains a non-mail item".to_owned()),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mailbox = input
            .split_off(self.turn_count)
            .into_iter()
            .zip(self.mailbox_options.iter().cloned())
            .map(|(item, options)| match item {
                TurnInput::InterAgentCommunication(mail) => Ok((mail, options.into())),
                _ => Err("accepted mailbox contains a non-mail item".to_owned()),
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Prepared {
            turn_input: input,
            notified: self.notified.map(Into::into),
            accepts_mail: self.accepts_mail,
            mailbox,
            accounting: self.accounting.restore()?,
            controller,
        })
    }
}

pub(super) async fn capture(
    session: &Session,
    turn: &str,
) -> Result<(InputMetadata, Vec<TurnInput>), String> {
    let frozen = session
        .services
        .thread_extension_data
        .get::<FrozenTurn>()
        .ok_or("accepted input was not retained at suspension")?;
    if frozen.thread != session.thread_id || frozen.turn != turn {
        return Err("accepted input suspension changed origin".into());
    }
    let mut input = frozen.queue.items.clone();
    let turn_count = input.len();
    let mailbox_options = {
        let mut mailbox = session.input_queue.mailbox_pending_mails.lock().await;
        session.input_queue.read_mailbox(&mut mailbox);
        input.extend(
            mailbox
                .iter()
                .map(|mail| TurnInput::InterAgentCommunication(mail.communication.clone())),
        );
        mailbox
            .iter()
            .map(|mail| mail.start_options.clone().into())
            .collect()
    };
    let controller = frozen
        .controller
        .as_ref()
        .map_err(Clone::clone)?
        .as_ref()
        .map(crate::agent::control::FrozenExecMailboxes::snapshot)
        .unwrap_or_default();
    input.extend(
        controller
            .iter()
            .map(|mail| TurnInput::InterAgentCommunication(mail.communication.clone())),
    );
    Ok((
        InputMetadata {
            version: 4,
            thread: session.thread_id,
            turn: turn.into(),
            turn_count,
            notified: frozen.queue.notified_activity.map(Into::into),
            accepts_mail: frozen.accepts_mail,
            mailbox_options,
            accounting: frozen.accounting.capture()?,
            controller_routes: controller
                .iter()
                .map(|mail| ControllerRoute {
                    thread: mail.thread,
                    receive_id: mail.receive_id.clone(),
                })
                .collect(),
        },
        input,
    ))
}

/// Private snapshot of several suspended threads. The first frame is the root.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecInputBatch {
    version: u32,
    frames: Vec<InputMetadata>,
    idle: Vec<idle::IdleMetadata>,
    input: ExecPendingInputs,
}

impl std::fmt::Debug for ExecInputBatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExecInputBatch")
            .field("threads", &(self.frames.len() + self.idle.len()))
            .finish_non_exhaustive()
    }
}

/// Decoded queues, not a grant. Each frame can be attached exactly once.
pub struct PreparedExecInputBatch {
    entries: HashMap<ThreadId, (String, Prepared)>,
    idle: HashMap<ThreadId, PreparedExecIdleInput>,
}

impl ExecInputBatch {
    /// Capture only after all supplied threads have stopped their producers.
    pub async fn capture(threads: &[(&crate::CodexThread, &str)]) -> Result<Self, String> {
        Self::capture_with_idle(threads, &[]).await
    }

    /// Active frames retain their original turn; idle frames carry mailbox
    /// input only. Capture after the shared producer-shutdown barrier.
    pub async fn capture_with_idle(
        threads: &[(&crate::CodexThread, &str)],
        idle_threads: &[&crate::CodexThread],
    ) -> Result<Self, String> {
        let mut frames = Vec::with_capacity(threads.len());
        let mut groups = Vec::with_capacity(threads.len());
        for (thread, turn) in threads {
            let (frame, input) = capture(&thread.session, turn).await?;
            frames.push(frame);
            groups.push(input);
        }
        let mut idle = Vec::with_capacity(idle_threads.len());
        for thread in idle_threads {
            let (frame, input) = idle::capture(&thread.session).await?;
            idle.push(frame);
            groups.push(input);
        }
        let input = ExecPendingInputs::capture_groups(
            &groups.iter().map(Vec::as_slice).collect::<Vec<_>>(),
        )?;
        let batch = Self {
            version: 2,
            frames,
            idle,
            input,
        };
        batch.check_frames()?;
        Ok(batch)
    }

    fn check_frames(&self) -> Result<(), String> {
        if self.version != 2 || self.frames.is_empty() {
            return Err("input batch has no supported root frame".into());
        }
        let mut threads = HashSet::new();
        for frame in &self.frames {
            if frame.turn.is_empty() || !threads.insert(frame.thread) {
                return Err("input batch contains an invalid or repeated thread".into());
            }
        }
        for frame in &self.idle {
            if !threads.insert(frame.thread) {
                return Err("input batch repeats an active or idle thread".into());
            }
        }
        Ok(())
    }

    pub(crate) fn validate_members(
        &self,
        active: &[(ThreadId, &str)],
        idle: &[ThreadId],
    ) -> Result<(), String> {
        let expected = active
            .iter()
            .map(|(id, turn)| (*id, (*turn).to_owned()))
            .collect::<HashSet<_>>();
        let actual = self
            .frames
            .iter()
            .map(|frame| (frame.thread, frame.turn.clone()))
            .collect::<HashSet<_>>();
        let expected_idle = idle.iter().copied().collect::<HashSet<_>>();
        let actual_idle = self
            .idle
            .iter()
            .map(|frame| frame.thread)
            .collect::<HashSet<_>>();
        if expected.len() != active.len()
            || expected_idle.len() != idle.len()
            || actual != expected
            || actual_idle != expected_idle
        {
            return Err("tree input frames do not match the captured thread states".into());
        }
        Ok(())
    }

    pub fn validate(&self, root: ThreadId, turn: &str) -> Result<(), String> {
        self.clone().prepare(root, turn).map(|_| ())
    }

    /// Decode every group together so shared publication state remains shared.
    /// This neither enqueues input nor authorizes execution in any thread.
    pub fn prepare(self, root: ThreadId, turn: &str) -> Result<PreparedExecInputBatch, String> {
        self.check_frames()?;
        if self.frames[0].thread != root || self.frames[0].turn != turn {
            return Err("input batch belongs to another root or turn".into());
        }
        let groups = self.input.restore_groups()?;
        if groups.len() != self.frames.len() + self.idle.len() {
            return Err("input batch frame and group counts differ".into());
        }
        let mut groups = groups.into_iter();
        let mut entries = HashMap::with_capacity(self.frames.len());
        for frame in self.frames {
            let group = groups.next().ok_or("input batch lost an active group")?;
            let prepared = frame.decode(frame.thread, &frame.turn, group)?;
            entries.insert(frame.thread, (frame.turn, prepared));
        }
        let mut idle = HashMap::with_capacity(self.idle.len());
        for frame in self.idle {
            let group = groups.next().ok_or("input batch lost an idle group")?;
            let thread = frame.thread;
            idle.insert(thread, frame.decode(group)?);
        }
        Ok(PreparedExecInputBatch { entries, idle })
    }
}

impl PreparedExecInputBatch {
    fn take(&mut self, thread: ThreadId, turn: &str) -> Result<Prepared, String> {
        let (expected, _) = self
            .entries
            .get(&thread)
            .ok_or("input batch has no pending frame for this thread")?;
        if expected != turn {
            return Err("input batch frame belongs to another turn".into());
        }
        self.entries
            .remove(&thread)
            .map(|(_, prepared)| prepared)
            .ok_or_else(|| "input batch frame was already attached".into())
    }

    pub fn len(&self) -> usize {
        self.entries.len() + self.idle.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty() && self.idle.is_empty()
    }

    /// Take idle data once without inventing an original turn or installing
    /// messages. The tree consumer still owns activation and fresh admission.
    pub fn take_idle(&mut self, thread: ThreadId) -> Result<PreparedExecIdleInput, String> {
        self.idle
            .remove(&thread)
            .ok_or_else(|| "input batch has no idle frame for this thread".into())
    }

    /// Attach one prepared frame. Ordinary Core admission still gates installation.
    pub fn prepare_start(
        &mut self,
        mut options: crate::StartThreadOptions,
        thread: ThreadId,
        turn: &str,
    ) -> Result<crate::StartThreadOptions, String> {
        let prepared = self.take(thread, turn)?;
        options.thread_extension_init.insert(RestorePending {
            thread,
            turn: turn.into(),
            prepared: Mutex::new(Some(prepared)),
        });
        Ok(options)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_communication::ExecPublication;
    use crate::agent_communication::MessageDelivery;
    use codex_protocol::AgentPath;
    use codex_protocol::protocol::InterAgentCommunication;

    fn metadata(thread: ThreadId, turn: &str, tools: u64) -> InputMetadata {
        let mut accounting = TurnAccounting::default().capture().unwrap();
        accounting.tool_calls = tools;
        InputMetadata {
            version: 4,
            thread,
            turn: turn.into(),
            turn_count: 1,
            notified: Some(Activity::Mailbox),
            accepts_mail: true,
            mailbox_options: Vec::new(),
            accounting,
            controller_routes: Vec::new(),
        }
    }

    fn fixture() -> (ExecInputBatch, ThreadId, ThreadId) {
        let root = ThreadId::new();
        let child = ThreadId::new();
        let delivery = MessageDelivery::publish(root, &[]);
        let mail = TurnInput::InterAgentCommunication(crate::PendingAgentMessage::new(
            InterAgentCommunication::new(
                AgentPath::root(),
                AgentPath::root(),
                Vec::new(),
                "retained".into(),
                false,
            ),
            Some(delivery),
        ));
        let first = vec![mail.clone()];
        let second = vec![mail];
        (
            ExecInputBatch {
                version: 2,
                idle: Vec::new(),
                frames: vec![
                    metadata(root, "root-turn", 9),
                    metadata(child, "child-turn", 3),
                ],
                input: ExecPendingInputs::capture_groups(&[&first, &second]).unwrap(),
            },
            root,
            child,
        )
    }

    #[test]
    fn serialized_batch_retains_each_turn_and_one_shared_publication() {
        let (batch, root, child) = fixture();
        let decoded: ExecInputBatch =
            serde_json::from_slice(&serde_json::to_vec(&batch).unwrap()).unwrap();
        let mut prepared = decoded.prepare(root, "root-turn").unwrap();
        assert_eq!(prepared.len(), 2);
        let first = prepared.take(root, "root-turn").unwrap();
        let second = prepared.take(child, "child-turn").unwrap();
        assert_eq!(first.accounting.tool_calls, 9);
        assert_eq!(second.accounting.tool_calls, 3);
        assert!(prepared.is_empty());
        let TurnInput::InterAgentCommunication(first) = &first.turn_input[0] else {
            panic!("root mail")
        };
        let TurnInput::InterAgentCommunication(second) = &second.turn_input[0] else {
            panic!("child mail")
        };
        first.delivery.as_ref().unwrap().consumed(root);
        assert_eq!(
            second.delivery.as_ref().unwrap().capture_exec().unwrap().1,
            ExecPublication::Consumed
        );
    }

    #[test]
    fn wrong_frame_or_duplicate_attachment_does_not_consume_another_turn() {
        let (batch, root, child) = fixture();
        let mut prepared = batch.prepare(root, "root-turn").unwrap();
        assert!(prepared.take(child, "root-turn").is_err());
        assert!(prepared.take(ThreadId::new(), "child-turn").is_err());
        assert_eq!(prepared.len(), 2);
        prepared.take(child, "child-turn").unwrap();
        assert!(prepared.take(child, "child-turn").is_err());
        assert_eq!(prepared.len(), 1);
        prepared.take(root, "root-turn").unwrap();
    }

    #[test]
    fn invalid_batches_are_rejected_before_any_attachment() {
        let (batch, root, _) = fixture();
        assert!(batch.validate(ThreadId::new(), "root-turn").is_err());
        assert!(batch.validate(root, "other-turn").is_err());
        let mut duplicate = batch.clone();
        duplicate.frames[1].thread = root;
        assert!(duplicate.validate(root, "root-turn").is_err());
        let mut missing = batch.clone();
        missing.frames.pop();
        assert!(missing.validate(root, "root-turn").is_err());
        let mut version = batch.clone();
        version.version = 0;
        assert!(version.validate(root, "root-turn").is_err());
        let mut empty = batch;
        empty.frames.clear();
        assert!(empty.validate(root, "root-turn").is_err());
    }
    fn idle_fixture() -> (ExecInputBatch, ThreadId, ThreadId, ThreadId) {
        let (mut batch, root, active) = fixture();
        let idle = ThreadId::new();
        let mut groups = batch.input.restore_groups().unwrap();
        groups.push(groups[0].clone());
        batch.input = ExecPendingInputs::capture_groups(
            &groups.iter().map(Vec::as_slice).collect::<Vec<_>>(),
        )
        .unwrap();
        batch.idle.push(idle::IdleMetadata {
            thread: idle,
            mailbox_options: vec![
                TurnStartOptions {
                    parent_turn_id: Some("original-parent".into()),
                    root_turn_id: Some("original-root".into()),
                    service_tier: Some("flex".into()),
                    final_output_json_schema: Some(serde_json::json!({"type": "object"})),
                    ..Default::default()
                }
                .into(),
            ],
        });
        (batch, root, active, idle)
    }

    #[test]
    fn active_and_idle_frames_share_publications_without_inventing_a_turn() {
        let (batch, root, active, idle) = idle_fixture();
        let batch: ExecInputBatch =
            serde_json::from_slice(&serde_json::to_vec(&batch).unwrap()).unwrap();
        let mut prepared = batch.prepare(root, "root-turn").unwrap();
        assert_eq!(prepared.len(), 3);
        assert!(prepared.take(idle, "invented-turn").is_err());
        assert!(prepared.take_idle(active).is_err());
        assert_eq!(prepared.len(), 3);
        let idle_mail = prepared.take_idle(idle).unwrap();
        assert_eq!(idle_mail.thread(), idle);
        assert_eq!(idle_mail.len(), 1);
        assert_eq!(
            idle_mail.mailbox[0].1.parent_turn_id.as_deref(),
            Some("original-parent")
        );
        assert_eq!(
            idle_mail.mailbox[0].1.root_turn_id.as_deref(),
            Some("original-root")
        );
        assert_eq!(idle_mail.mailbox[0].1.service_tier.as_deref(), Some("flex"));
        assert_eq!(
            idle_mail.mailbox[0].1.final_output_json_schema,
            Some(serde_json::json!({"type": "object"}))
        );
        assert!(prepared.take_idle(idle).is_err());
        let root_input = prepared.take(root, "root-turn").unwrap();
        let child_input = prepared.take(active, "child-turn").unwrap();
        idle_mail.mailbox[0]
            .0
            .delivery
            .as_ref()
            .unwrap()
            .consumed(idle);
        for input in [&root_input.turn_input[0], &child_input.turn_input[0]] {
            let TurnInput::InterAgentCommunication(mail) = input else {
                panic!("expected mail")
            };
            assert_eq!(
                mail.delivery.as_ref().unwrap().capture_exec().unwrap().1,
                ExecPublication::Consumed
            );
        }
        assert_eq!(root_input.accounting.tool_calls, 9);
        assert_eq!(child_input.accounting.tool_calls, 3);
        assert!(prepared.is_empty());
    }

    #[test]
    fn idle_frame_rejects_aliases_bad_boundaries_and_non_mail_before_preparing() {
        let (batch, root, active, _) = idle_fixture();
        let mut alias = batch.clone();
        alias.idle[0].thread = active;
        assert!(alias.prepare(root, "root-turn").is_err());
        let mut boundary = batch.clone();
        boundary.idle[0].mailbox_options.clear();
        assert!(boundary.prepare(root, "root-turn").is_err());
        let mut non_mail = batch.clone();
        let mut groups = non_mail.input.restore_groups().unwrap();
        groups.last_mut().unwrap()[0] = TurnInput::UserInput {
            content: Vec::new(),
            client_id: None,
            metadata: Default::default(),
        };
        non_mail.input = ExecPendingInputs::capture_groups(
            &groups.iter().map(Vec::as_slice).collect::<Vec<_>>(),
        )
        .unwrap();
        assert!(non_mail.prepare(root, "root-turn").is_err());
        let mut no_active_root = batch;
        no_active_root.frames.clear();
        assert!(no_active_root.prepare(root, "root-turn").is_err());
    }

    #[test]
    fn empty_idle_child_stays_an_idle_frame() {
        let (mut batch, root, _) = fixture();
        let idle = ThreadId::new();
        batch.idle.push(idle::IdleMetadata {
            thread: idle,
            mailbox_options: Vec::new(),
        });
        let mut groups = batch.input.restore_groups().unwrap();
        groups.push(Vec::new());
        batch.input = ExecPendingInputs::capture_groups(
            &groups.iter().map(Vec::as_slice).collect::<Vec<_>>(),
        )
        .unwrap();
        let mut prepared = batch.prepare(root, "root-turn").unwrap();
        let idle_input = prepared.take_idle(idle).unwrap();
        assert!(idle_input.is_empty());
        assert_eq!(idle_input.thread(), idle);
        assert_eq!(prepared.len(), 2);
    }
}
