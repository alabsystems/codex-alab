// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Accepted input retained at suspension and installed after native admission.
use super::InputQueueActivity;
use super::PendingMailboxCommunication;
use super::TurnInput;
use super::TurnInputQueue;
use super::exec_pending_input::ExecPendingInputs;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use crate::state::ActiveTurn;
use crate::state::MailboxDeliveryPhase;
use codex_alab_core::quota::HeadlessExec;
use codex_protocol::ThreadId;
use codex_protocol::turn_input::CyberAccessProgram;
use codex_protocol::turn_input::PromptCacheTtl;
use codex_protocol::turn_input::TurnStartOptions;
use serde::Deserialize;
use serde::Serialize;
use std::sync::Mutex;

#[path = "exec_idle_input.rs"]
mod idle;
pub use idle::PreparedExecIdleInput;
pub(crate) use idle::activate as activate_idle;
pub(crate) use idle::install as install_idle;

#[path = "exec_input_batch.rs"]
mod batch;
pub use batch::ExecInputBatch;
pub use batch::PreparedExecInputBatch;

struct FrozenTurn {
    thread: ThreadId,
    turn: String,
    queue: TurnInputQueue,
    accepts_mail: bool,
    accounting: TurnAccounting,
    controller: Result<Option<crate::agent::control::FrozenExecMailboxes>, String>,
}

#[derive(Default)]
struct TurnAccounting {
    tool_calls: u64,
    has_memory_citation: bool,
    token_usage_at_turn_start: codex_protocol::protocol::TokenUsage,
    token_usage_by_model: crate::state::TurnTokenUsage,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedTurnAccounting {
    tool_calls: u64,
    has_memory_citation: bool,
    token_usage_at_turn_start: codex_protocol::protocol::TokenUsage,
    token_usage_by_model: serde_json::Value,
}

impl TurnAccounting {
    fn capture(&self) -> Result<SavedTurnAccounting, String> {
        Ok(SavedTurnAccounting {
            tool_calls: self.tool_calls,
            has_memory_citation: self.has_memory_citation,
            token_usage_at_turn_start: self.token_usage_at_turn_start.clone(),
            token_usage_by_model: self.token_usage_by_model.capture_exec_usage()?,
        })
    }
}

impl SavedTurnAccounting {
    fn restore(&self) -> Result<TurnAccounting, String> {
        Ok(TurnAccounting {
            tool_calls: self.tool_calls,
            has_memory_citation: self.has_memory_citation,
            token_usage_at_turn_start: self.token_usage_at_turn_start.clone(),
            token_usage_by_model: crate::state::TurnTokenUsage::restore_exec_usage(
                self.token_usage_by_model.clone(),
            )?,
        })
    }
}

#[derive(Clone, Copy, Serialize, Deserialize)]
enum Activity {
    Mailbox,
    Steer,
}
impl From<InputQueueActivity> for Activity {
    fn from(value: InputQueueActivity) -> Self {
        match value {
            InputQueueActivity::Mailbox => Self::Mailbox,
            InputQueueActivity::Steer => Self::Steer,
        }
    }
}
impl From<Activity> for InputQueueActivity {
    fn from(value: Activity) -> Self {
        match value {
            Activity::Mailbox => Self::Mailbox,
            Activity::Steer => Self::Steer,
        }
    }
}

#[derive(Clone, Copy, Serialize, Deserialize)]
enum SavedPromptCacheTtl {
    #[serde(rename = "30m")]
    ThirtyMinutes,
    #[serde(rename = "12h")]
    TwelveHours,
}

impl From<PromptCacheTtl> for SavedPromptCacheTtl {
    fn from(value: PromptCacheTtl) -> Self {
        match value {
            PromptCacheTtl::ThirtyMinutes => Self::ThirtyMinutes,
            PromptCacheTtl::TwelveHours => Self::TwelveHours,
        }
    }
}

impl From<SavedPromptCacheTtl> for PromptCacheTtl {
    fn from(value: SavedPromptCacheTtl) -> Self {
        match value {
            SavedPromptCacheTtl::ThirtyMinutes => Self::ThirtyMinutes,
            SavedPromptCacheTtl::TwelveHours => Self::TwelveHours,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedStart {
    turn_trigger: Option<String>,
    final_output_json_schema: Option<serde_json::Value>,
    direct_only_tool_namespaces: Option<Vec<String>>,
    service_tier: Option<String>,
    prompt_cache_ttl: Option<SavedPromptCacheTtl>,
    parent_turn_id: Option<String>,
    initiating_agent_path: Option<codex_protocol::AgentPath>,
    root_turn_id: Option<String>,
    cyber_access_program: Option<CyberAccessProgram>,
}
impl From<TurnStartOptions> for SavedStart {
    fn from(value: TurnStartOptions) -> Self {
        let TurnStartOptions {
            turn_trigger,
            final_output_json_schema,
            direct_only_tool_namespaces,
            service_tier,
            prompt_cache_ttl,
            parent_turn_id,
            initiating_agent_path,
            root_turn_id,
            cyber_access_program,
        } = value;
        Self {
            turn_trigger,
            final_output_json_schema,
            direct_only_tool_namespaces,
            service_tier,
            prompt_cache_ttl: prompt_cache_ttl.map(Into::into),
            parent_turn_id,
            initiating_agent_path,
            root_turn_id,
            cyber_access_program,
        }
    }
}
impl From<SavedStart> for TurnStartOptions {
    fn from(value: SavedStart) -> Self {
        let SavedStart {
            turn_trigger,
            final_output_json_schema,
            direct_only_tool_namespaces,
            service_tier,
            prompt_cache_ttl,
            parent_turn_id,
            initiating_agent_path,
            root_turn_id,
            cyber_access_program,
        } = value;
        Self {
            turn_trigger,
            final_output_json_schema,
            direct_only_tool_namespaces,
            service_tier,
            prompt_cache_ttl: prompt_cache_ttl.map(Into::into),
            parent_turn_id,
            initiating_agent_path,
            root_turn_id,
            cyber_access_program,
        }
    }
}

/// Input data only; native admission remains required before installation.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecInputSnapshot {
    version: u32,
    thread: ThreadId,
    turn: String,
    input: ExecPendingInputs,
    turn_count: usize,
    notified: Option<Activity>,
    accepts_mail: bool,
    mailbox_options: Vec<SavedStart>,
    accounting: SavedTurnAccounting,
    controller_routes: Vec<ControllerRoute>,
}
impl std::fmt::Debug for ExecInputSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExecInputSnapshot")
            .field("thread", &self.thread)
            .field("turn", &self.turn)
            .field("turn_count", &self.turn_count)
            .field("mailbox_count", &self.mailbox_options.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ControllerRoute {
    thread: ThreadId,
    receive_id: Option<String>,
}

struct Prepared {
    turn_input: Vec<TurnInput>,
    notified: Option<InputQueueActivity>,
    accepts_mail: bool,
    mailbox: Vec<(crate::PendingAgentMessage, TurnStartOptions)>,
    accounting: TurnAccounting,
    controller: Vec<crate::agent::control::ExecMailboxMessage>,
}
struct InputsInstalled {
    turn: String,
    baseline: codex_protocol::protocol::TokenUsage,
}

struct RestorePending {
    thread: ThreadId,
    turn: String,
    prepared: Mutex<Option<Prepared>>,
}

pub(crate) fn pending_origin(session: &Session) -> Option<(ThreadId, String)> {
    session
        .services
        .thread_extension_data
        .get::<RestorePending>()
        .map(|pending| (pending.thread, pending.turn.clone()))
}

impl ExecInputSnapshot {
    fn decode(&self, thread: ThreadId, turn: &str) -> Result<Prepared, String> {
        batch::InputMetadata::from(self).decode(thread, turn, self.input.clone().restore()?)
    }

    pub fn validate(&self, thread: ThreadId, turn: &str) -> Result<(), String> {
        self.decode(thread, turn).map(|_| ())
    }

    /// Decode before native consumption; do not enqueue anything during bootstrap.
    pub fn prepare_start(
        &self,
        mut options: crate::StartThreadOptions,
        thread: ThreadId,
        turn: &str,
    ) -> Result<crate::StartThreadOptions, String> {
        let prepared = self.decode(thread, turn)?;
        options.thread_extension_init.insert(RestorePending {
            thread,
            turn: turn.into(),
            prepared: Mutex::new(Some(prepared)),
        });
        Ok(options)
    }
}

/// Take ownership while the removed turn is still available. Other suspension
/// callers retain their existing behavior; native exec keeps the accepted data.
pub(crate) async fn freeze(session: &Session, turn: &ActiveTurn, turn_id: &str) {
    freeze_local(session, turn, turn_id, true).await;
}

pub(crate) async fn freeze_local(
    session: &Session,
    turn: &ActiveTurn,
    turn_id: &str,
    owns_controller: bool,
) {
    if session
        .services
        .thread_extension_data
        .get::<HeadlessExec>()
        .is_none()
    {
        session.input_queue.clear_pending(turn).await;
        return;
    }
    let (queue, accepts_mail, accounting) = {
        let mut state = turn.turn_state.lock().await;
        state.clear_pending_waiters();
        let accepts_mail = state.accepts_mailbox_delivery_for_current_turn();
        let accounting = TurnAccounting {
            tool_calls: state.tool_calls,
            has_memory_citation: state.has_memory_citation,
            token_usage_at_turn_start: state.token_usage_at_turn_start.clone(),
            token_usage_by_model: std::mem::take(&mut state.token_usage_by_model),
        };
        (
            std::mem::take(&mut state.pending_input),
            accepts_mail,
            accounting,
        )
    };
    session.services.thread_extension_data.insert(FrozenTurn {
        thread: session.thread_id,
        turn: turn_id.into(),
        queue,
        accepts_mail,
        accounting,
        controller: if owns_controller {
            session
                .services
                .local_agent_runtime
                .freeze_exec_mailboxes()
                .map(Some)
                .map_err(|error| error.to_string())
        } else {
            Ok(None)
        },
    });
}

impl crate::CodexThread {
    /// Reject unsupported publication state before the host drains the runtime.
    pub async fn validate_exec_input_handoff(&self, turn: &str) -> Result<(), String> {
        validate_session_input(&self.session, turn, true).await
    }

    /// Read only after producer shutdown; the raw frozen queue remains owned by
    /// the old runtime until successful replacement or explicit recovery.
    pub async fn exec_input_snapshot(&self, turn: &str) -> Result<ExecInputSnapshot, String> {
        let (metadata, input) = batch::capture(&self.session, turn).await?;
        Ok(metadata.with_input(ExecPendingInputs::capture(&input)?))
    }
}

pub(crate) async fn validate_session_input(
    session: &Session,
    turn: &str,
    require_exec: bool,
) -> Result<(), String> {
    if require_exec
        && session
            .services
            .thread_extension_data
            .get::<HeadlessExec>()
            .is_none()
    {
        return Err("accepted input handoff requires an exec runtime".into());
    }
    if !session
        .services
        .extensions
        .message_lifecycle_contributors()
        .is_empty()
    {
        return Err("exec input handoff requires a lifecycle-observer restoration protocol".into());
    }
    let state = {
        let active = session.active_turn.lock().await;
        let active = active
            .as_ref()
            .filter(|active| {
                active
                    .task
                    .as_ref()
                    .is_some_and(|task| task.turn_context.sub_id == turn)
            })
            .ok_or("accepted input handoff requires the original active turn")?;
        active.turn_state.clone()
    };
    let mut input = {
        let state = state.lock().await;
        state.token_usage_by_model.capture_exec_usage()?;
        state.pending_input.items.clone()
    };
    {
        let mailbox = session.input_queue.mailbox_pending_mails.lock().await;
        input.extend(
            mailbox
                .iter()
                .map(|mail| TurnInput::InterAgentCommunication(mail.communication.clone())),
        );
    }
    input.extend(
        session
            .services
            .local_agent_runtime
            .exec_mailboxes()
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|mail| TurnInput::InterAgentCommunication(mail.communication)),
    );
    ExecPendingInputs::capture(&input).map(|_| ())
}

#[expect(
    clippy::await_holding_invalid_type,
    reason = "reserved-turn validation and ordered input installation must be atomic"
)]
pub(crate) async fn install_after_admission(
    session: &Session,
    context: &TurnContext,
) -> codex_protocol::error::Result<()> {
    use codex_protocol::error::CodexErr;
    let Some(saved) = session
        .services
        .thread_extension_data
        .get::<RestorePending>()
    else {
        return Ok(());
    };
    let valid = saved.thread == session.thread_id
        && saved.turn == context.sub_id
        && context
            .extension_data
            .get::<codex_alab_core::renewal::TurnAdmission>()
            .is_some()
        && context
            .extension_data
            .get::<crate::ExecRecoveryOptions>()
            .is_some_and(|options| {
                options.thread() == saved.thread && options.turn() == saved.turn
            });
    if !valid {
        return Err(CodexErr::Fatal(
            "accepted input lacks original-turn admission".into(),
        ));
    }
    let active = session.active_turn.lock().await;
    let state = active
        .as_ref()
        .filter(|active| active.task.is_none())
        .ok_or_else(|| {
            CodexErr::Fatal("accepted input requires a reserved recovery turn".into())
        })?;
    let mut state = state.turn_state.lock().await;
    let mut mailbox = session.input_queue.mailbox_pending_mails.lock().await;
    let prepared = saved
        .prepared
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
        .ok_or_else(|| CodexErr::Fatal("accepted input snapshot was already installed".into()))?;
    session
        .services
        .local_agent_runtime
        .restore_exec_mailboxes(prepared.controller)?;
    state.tool_calls = prepared.accounting.tool_calls;
    state.has_memory_citation = prepared.accounting.has_memory_citation;
    let baseline = prepared.accounting.token_usage_at_turn_start;
    state.token_usage_at_turn_start = baseline.clone();
    state.token_usage_by_model = prepared.accounting.token_usage_by_model;
    let has_fresh = !state.pending_input.items.is_empty();
    let mut input = prepared.turn_input;
    input.append(&mut state.pending_input.items);
    state.pending_input.items = input;
    state.pending_input.notified_activity =
        state.pending_input.notified_activity.max(prepared.notified);
    if !has_fresh {
        state.set_mailbox_delivery_phase(if prepared.accepts_mail {
            MailboxDeliveryPhase::CurrentTurn
        } else {
            MailboxDeliveryPhase::NextTurn
        });
    }
    let mut restored_mailbox = prepared
        .mailbox
        .into_iter()
        .map(
            |(communication, start_options)| PendingMailboxCommunication {
                communication,
                start_options,
                _diagnostics_guard: super::PENDING_MAILBOX_MESSAGES.track(),
            },
        )
        .collect::<std::collections::VecDeque<_>>();
    restored_mailbox.append(&mut mailbox);
    *mailbox = restored_mailbox;
    let activity = state
        .pending_input
        .notified_activity
        .or((!mailbox.is_empty()).then_some(InputQueueActivity::Mailbox));
    drop(mailbox);
    drop(state);
    drop(active);
    session
        .services
        .thread_extension_data
        .remove::<RestorePending>();
    session
        .services
        .thread_extension_data
        .insert(InputsInstalled {
            turn: context.sub_id.clone(),
            baseline,
        });
    if let Some(activity) = activity {
        session.input_queue.activity_tx.send_replace(activity);
    }
    Ok(())
}

/// Continue the original accounting interval rather than treating replacement
/// as the start of a new turn. Called before startup consumes its one-use marker.
pub(crate) async fn startup_token_baseline(
    session: &Session,
    context: &TurnContext,
) -> codex_protocol::protocol::TokenUsage {
    if let Some(installed) = session
        .services
        .thread_extension_data
        .get::<InputsInstalled>()
        .filter(|installed| installed.turn == context.sub_id)
    {
        return installed.baseline.clone();
    }
    session.total_token_usage().await.unwrap_or_default()
}

/// Native recovery continues a turn, so its mailbox must obey the restored
/// delivery phase instead of being drained by ordinary new-task startup.
pub(crate) async fn startup_mailbox(
    session: &Session,
    context: &TurnContext,
) -> (Vec<TurnInput>, TurnStartOptions) {
    if session
        .services
        .thread_extension_data
        .get::<InputsInstalled>()
        .is_some_and(|installed| installed.turn == context.sub_id)
    {
        session
            .services
            .thread_extension_data
            .remove::<InputsInstalled>();
        return (Vec::new(), TurnStartOptions::default());
    }
    session.input_queue.drain_mailbox_input_items().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_protocol::AgentPath;
    use codex_protocol::protocol::InterAgentCommunication;

    #[test]
    fn root_and_controller_mail_share_one_publication_after_round_trip() {
        use crate::agent_communication::ExecPublication;
        use crate::agent_communication::MessageDelivery;
        let recipient = ThreadId::new();
        let delivery = MessageDelivery::publish(ThreadId::new(), &[]);
        let mail = crate::PendingAgentMessage::new(
            InterAgentCommunication::new(
                AgentPath::root(),
                AgentPath::root(),
                Vec::new(),
                "shared mail".into(),
                false,
            ),
            Some(delivery.clone()),
        );
        let items = vec![
            TurnInput::InterAgentCommunication(mail.clone()),
            TurnInput::InterAgentCommunication(mail),
        ];
        let snapshot = ExecInputSnapshot {
            version: 4,
            thread: recipient,
            turn: "original".into(),
            input: ExecPendingInputs::capture(&items).unwrap(),
            turn_count: 1,
            notified: None,
            accepts_mail: true,
            mailbox_options: Vec::new(),
            accounting: TurnAccounting::default().capture().unwrap(),
            controller_routes: vec![ControllerRoute {
                thread: recipient,
                receive_id: Some("received".into()),
            }],
        };
        let decoded: ExecInputSnapshot =
            serde_json::from_slice(&serde_json::to_vec(&snapshot).unwrap()).unwrap();
        let restored = decoded.decode(recipient, "original").unwrap();
        assert_eq!(
            restored.controller[0].receive_id.as_deref(),
            Some("received")
        );
        let TurnInput::InterAgentCommunication(root) = &restored.turn_input[0] else {
            panic!("root mail")
        };
        root.delivery.as_ref().unwrap().consumed(recipient);
        assert!(matches!(
            restored.controller[0]
                .communication
                .delivery
                .as_ref()
                .unwrap()
                .capture_exec()
                .unwrap()
                .1,
            ExecPublication::Consumed
        ));
        assert!(matches!(
            delivery.capture_exec().unwrap().1,
            ExecPublication::Pending { .. }
        ));
        let mut invalid = decoded;
        invalid.controller_routes.clear();
        assert!(invalid.decode(recipient, "original").is_err());
    }

    #[tokio::test]
    async fn admitted_recovery_retains_accounting_controller_mail_and_original_startup_baseline() {
        use codex_protocol::protocol::TokenUsage;
        let (mut session, context) = crate::session::tests::make_session_and_context().await;
        session.services.thread_extension_data.insert(HeadlessExec);
        let recipient = ThreadId::new();
        let retained = crate::PendingAgentMessage::from(InterAgentCommunication::new(
            AgentPath::root(),
            AgentPath::root(),
            Vec::new(),
            "unloaded child mail".into(),
            false,
        ));
        session
            .services
            .local_agent_runtime
            .restore_exec_mailboxes(vec![crate::agent::control::ExecMailboxMessage {
                thread: recipient,
                receive_id: Some("old-receive".into()),
                communication: retained.clone(),
            }])
            .unwrap();
        let previous = ActiveTurn::default();
        let baseline = TokenUsage {
            input_tokens: 17,
            total_tokens: 17,
            ..Default::default()
        };
        {
            let mut state = previous.turn_state.lock().await;
            state.tool_calls = 9;
            state.has_memory_citation = true;
            state.token_usage_at_turn_start = baseline.clone();
            state.token_usage_by_model.record(
                "original",
                context.session_telemetry.clone(),
                &TokenUsage {
                    input_tokens: 7,
                    total_tokens: 7,
                    ..Default::default()
                },
            );
        }
        freeze(&session, &previous, &context.sub_id).await;
        let thread = session.thread_id;
        let frozen = session
            .services
            .thread_extension_data
            .get::<FrozenTurn>()
            .unwrap();
        let controller = frozen
            .controller
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap()
            .snapshot();
        let snapshot = ExecInputSnapshot {
            version: 4,
            thread,
            turn: context.sub_id.clone(),
            input: ExecPendingInputs::capture(
                &controller
                    .iter()
                    .map(|mail| TurnInput::InterAgentCommunication(mail.communication.clone()))
                    .collect::<Vec<_>>(),
            )
            .unwrap(),
            turn_count: 0,
            notified: None,
            accepts_mail: true,
            mailbox_options: Vec::new(),
            accounting: frozen.accounting.capture().unwrap(),
            controller_routes: controller
                .iter()
                .map(|mail| ControllerRoute {
                    thread: mail.thread,
                    receive_id: mail.receive_id.clone(),
                })
                .collect(),
        };
        let encoded = serde_json::to_vec(&snapshot).unwrap();
        let decoded: ExecInputSnapshot = serde_json::from_slice(&encoded).unwrap();
        // The replacement has a fresh shared controller; the old one remains sealed.
        session.services.local_agent_runtime =
            crate::agent::control::LocalAgentControl::default().runtime;
        *session.active_turn.lock().await = Some(ActiveTurn::default());
        session
            .services
            .thread_extension_data
            .insert(RestorePending {
                thread,
                turn: context.sub_id.clone(),
                prepared: Mutex::new(Some(decoded.decode(thread, &context.sub_id).unwrap())),
            });
        assert!(install_after_admission(&session, &context).await.is_err());
        assert!(
            session
                .services
                .local_agent_runtime
                .exec_mailboxes()
                .unwrap()
                .is_empty()
        );
        {
            let state = session
                .active_turn
                .lock()
                .await
                .as_ref()
                .unwrap()
                .turn_state
                .clone();
            assert_eq!(state.lock().await.tool_calls, 0);
        }
        let settings = context.next_step_settings.load();
        let recovery: crate::ExecRecoveryOptions = serde_json::from_value(serde_json::json!({
            "thread": thread, "turn": context.sub_id, "schema": null, "review": false,
            "service_tier": null, "cyber_access_program": null,
            "step": {"collaboration_mode": settings.effective_collaboration_mode(),
                "summary": settings.reasoning_summary, "approval_policy": settings.approval_policy(),
                "disabled_plugin_ids": []}
        })).unwrap();
        drop(settings);
        context.extension_data.insert(recovery);
        let admission =
            codex_alab_core::renewal::TurnAdmission::new(|_| Box::pin(async { Ok(()) }));
        admission.admit(context.sub_id.clone()).await.unwrap();
        context.extension_data.insert(admission);
        install_after_admission(&session, &context).await.unwrap();
        let restored = session
            .services
            .local_agent_runtime
            .exec_mailboxes()
            .unwrap();
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].thread, recipient);
        assert_eq!(restored[0].receive_id.as_deref(), Some("old-receive"));
        assert_eq!(restored[0].communication, retained);
        install_after_admission(&session, &context).await.unwrap();
        assert_eq!(
            session
                .services
                .local_agent_runtime
                .exec_mailboxes()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(startup_token_baseline(&session, &context).await, baseline);
        {
            let state = session
                .active_turn
                .lock()
                .await
                .as_ref()
                .unwrap()
                .turn_state
                .clone();
            let mut state = state.lock().await;
            assert_eq!(state.tool_calls, 9);
            assert!(state.has_memory_citation);
            state.token_usage_by_model.record(
                "original",
                context.session_telemetry.clone(),
                &TokenUsage {
                    input_tokens: 3,
                    total_tokens: 3,
                    ..Default::default()
                },
            );
            let usage = state.token_usage_by_model.capture_exec_usage().unwrap();
            assert_eq!(usage["models"][0]["usage"]["total_tokens"], 10);
        }
        startup_mailbox(&session, &context).await;
        assert_ne!(startup_token_baseline(&session, &context).await, baseline);
    }

    #[tokio::test]
    async fn recovered_start_leaves_deferred_mail_for_the_original_delivery_phase() {
        let (session, context) = crate::session::tests::make_session_and_context().await;
        let active = ActiveTurn::default();
        active
            .turn_state
            .lock()
            .await
            .set_mailbox_delivery_phase(MailboxDeliveryPhase::NextTurn);
        let turn_state = active.turn_state.clone();
        *session.active_turn.lock().await = Some(active);
        session
            .input_queue
            .enqueue_mailbox_communication(
                InterAgentCommunication::new(
                    AgentPath::root(),
                    AgentPath::root(),
                    Vec::new(),
                    "deferred original mail".into(),
                    true,
                )
                .into(),
                TurnStartOptions {
                    service_tier: Some("priority".into()),
                    ..Default::default()
                },
            )
            .await;
        session
            .services
            .thread_extension_data
            .insert(InputsInstalled {
                turn: context.sub_id.clone(),
                baseline: Default::default(),
            });
        let (early, _) = startup_mailbox(&session, &context).await;
        assert!(
            early.is_empty(),
            "recovered startup must not drain the deferred mailbox"
        );
        assert!(
            session
                .services
                .thread_extension_data
                .get::<InputsInstalled>()
                .is_none()
        );
        let (deferred, _) = session
            .input_queue
            .get_pending_input(&session.active_turn)
            .await;
        assert!(
            deferred.is_empty(),
            "the original next-turn phase still holds"
        );
        assert!(session.input_queue.has_pending_mailbox_items().await);
        turn_state
            .lock()
            .await
            .accept_mailbox_delivery_for_current_turn();
        let (delivered, options) = session
            .input_queue
            .get_pending_input(&session.active_turn)
            .await;
        assert_eq!(delivered.len(), 1);
        let TurnInput::InterAgentCommunication(mail) = &delivered[0] else {
            panic!("expected deferred mail")
        };
        assert_eq!(mail.content, "deferred original mail");
        assert_eq!(options.service_tier.as_deref(), Some("priority"));
        assert!(!session.input_queue.has_pending_mailbox_items().await);
    }

    #[test]
    fn mailbox_order_phase_and_start_metadata_survive_the_snapshot() {
        let thread = ThreadId::new();
        let mail = |text: &str| {
            TurnInput::InterAgentCommunication(
                InterAgentCommunication::new(
                    AgentPath::root(),
                    AgentPath::root(),
                    Vec::new(),
                    text.into(),
                    true,
                )
                .into(),
            )
        };
        let original = vec![mail("first accepted mail"), mail("second accepted mail")];
        let start = SavedStart::from(TurnStartOptions {
            turn_trigger: Some("original-trigger".into()),
            final_output_json_schema: Some(serde_json::json!({"type":"object"})),
            direct_only_tool_namespaces: Some(vec!["fixture_namespace".into()]),
            service_tier: Some("priority".into()),
            prompt_cache_ttl: Some(codex_protocol::turn_input::PromptCacheTtl::TwelveHours),
            parent_turn_id: Some("parent".into()),
            initiating_agent_path: Some(AgentPath::root()),
            root_turn_id: Some("root".into()),
            cyber_access_program: Some(CyberAccessProgram::Standard),
        });
        let snapshot = ExecInputSnapshot {
            version: 4,
            thread,
            turn: "original-turn".into(),
            input: ExecPendingInputs::capture(&original).unwrap(),
            turn_count: 0,
            notified: Some(Activity::Mailbox),
            accepts_mail: false,
            mailbox_options: vec![start.clone(), start.clone()],
            accounting: TurnAccounting::default().capture().unwrap(),
            controller_routes: Vec::new(),
        };
        let decoded: ExecInputSnapshot =
            serde_json::from_slice(&serde_json::to_vec(&snapshot).unwrap()).unwrap();
        assert!(decoded.validate(ThreadId::new(), "original-turn").is_err());
        assert!(decoded.validate(thread, "other-turn").is_err());
        let prepared = decoded.decode(thread, "original-turn").unwrap();
        assert!(prepared.turn_input.is_empty());
        assert_eq!(prepared.notified, Some(InputQueueActivity::Mailbox));
        assert!(!prepared.accepts_mail);
        assert_eq!(prepared.mailbox[0].0.content, "first accepted mail");
        assert_eq!(prepared.mailbox[1].0.content, "second accepted mail");
        for (_, options) in prepared.mailbox {
            assert_eq!(
                serde_json::to_value(SavedStart::from(options)).unwrap(),
                serde_json::to_value(&start).unwrap()
            );
        }
        let mut invalid = snapshot.clone();
        invalid.version = 1;
        assert!(invalid.validate(thread, "original-turn").is_err());
        let mut invalid = snapshot.clone();
        invalid.turn_count = 3;
        assert!(invalid.validate(thread, "original-turn").is_err());
        let mut invalid = snapshot;
        invalid.mailbox_options.pop();
        assert!(invalid.validate(thread, "original-turn").is_err());
    }
}
