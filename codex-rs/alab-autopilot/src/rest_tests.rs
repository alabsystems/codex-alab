// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The record's rest around a turn (CONTRACT §4): a turn's start is counted as running and
//! written through to the record BEFORE the turn runs (the start hook returns only once the
//! record's write-through answered), and its stop, its abort or the thread's stop ends it.

use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use codex_alab_aterm::Options as ClientOptions;
use codex_alab_core::session::Shared;
use codex_alab_core::session::Unrest;
use codex_extension_api::ExtensionData;
use codex_extension_api::ThreadLifecycleContributor;
use codex_extension_api::ThreadStopInput;
use codex_extension_api::TurnAbortInput;
use codex_extension_api::TurnLifecycleContributor;
use codex_extension_api::TurnStartInput;
use codex_extension_api::TurnStopInput;
use codex_protocol::ThreadId;
use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::ModeKind;
use codex_protocol::config_types::Settings;
use codex_protocol::items::AgentMessageContent;
use codex_protocol::items::AgentMessageItem;
use codex_protocol::items::TurnItem;
use codex_protocol::models::MessagePhase;
use codex_protocol::protocol::TurnAbortReason;

use crate::Autopilot;
use crate::Supervisor;
use crate::SupervisorOptions;
use crate::decide::ThreadState;
use crate::decide::Timing;
use crate::extension::ThreadRt;

/// What each flush of the record would have said (the unrest it saw).
type Flushes = Arc<Mutex<Vec<Option<Unrest>>>>;

/// An autopilot over a fresh shared state whose write-through notes what the record would
/// have said at each flush (the unrest it saw).
fn autopilot() -> (Arc<Autopilot>, &'static Shared, Flushes) {
    codex_alab_core::activate();
    let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
    let flushes = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&flushes);
    assert!(shared.set_flush(Box::new(move || {
        seen.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(shared.unrest());
        true
    })));
    let sup = Supervisor::new(SupervisorOptions {
        shared,
        endpoint: Arc::new(|| None),
        client: ClientOptions::default(),
    });
    let autopilot = Autopilot::new(sup, None, None, Timing::default(), |_| true);
    (autopilot, shared, flushes)
}

fn thread() -> (ExtensionData, Arc<ThreadRt>) {
    let id = ThreadId::new();
    let store = ExtensionData::new(id.to_string());
    store.insert(ThreadRt::new(id, None, false, Duration::ZERO));
    let rt = store.get::<ThreadRt>().expect("inserted");
    (store, rt)
}

async fn start(autopilot: &Autopilot, thread_store: &ExtensionData, turn: &ExtensionData) {
    if let Some(rt) = thread_store.get::<ThreadRt>()
        && autopilot.thread_rt(rt.key()).is_none()
    {
        autopilot.keep_thread(&rt);
    }
    let mode = CollaborationMode {
        mode: ModeKind::Default,
        settings: Settings {
            model: "gpt-5".to_string(),
            reasoning_effort: None,
            developer_instructions: None,
        },
    };
    let session = ExtensionData::new("session");
    autopilot
        .on_turn_start(TurnStartInput {
            turn_id: turn.level_id(),
            collaboration_mode: &mode,
            token_usage_at_turn_start: None,
            session_store: &session,
            thread_store,
            turn_store: turn,
        })
        .await;
}

fn flushed(flushes: &Mutex<Vec<Option<Unrest>>>) -> Vec<Option<Unrest>> {
    flushes
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turns_start_is_written_busy_before_it_runs_and_its_stop_ends_it() {
    let (autopilot, shared, flushes) = autopilot();
    let (store, _rt) = thread();
    let session = ExtensionData::new("session");
    let turn = ExtensionData::new("turn-1");
    start(&autopilot, &store, &turn).await;
    // The start hook returned: the write-through already ran, and saw the turn running.
    assert_eq!(flushed(&flushes), vec![Some(Unrest::TurnRunning)]);
    assert!(shared.turn_running());
    autopilot
        .on_turn_stop(TurnStopInput {
            session_store: &session,
            thread_store: &store,
            turn_store: &turn,
        })
        .await;
    assert!(!shared.turn_running(), "the stop ends it");
    assert_eq!(shared.unrest(), None);

    // An abort ends a turn as a stop does.
    let aborted = ExtensionData::new("turn-2");
    start(&autopilot, &store, &aborted).await;
    assert!(shared.turn_running());
    autopilot
        .on_turn_abort(TurnAbortInput {
            reason: TurnAbortReason::Interrupted,
            session_store: &session,
            thread_store: &store,
            turn_store: &aborted,
        })
        .await;
    assert!(!shared.turn_running(), "the abort ends it");

    // The thread's stop ends a turn whose own end never came.
    let stranded = ExtensionData::new("turn-3");
    start(&autopilot, &store, &stranded).await;
    assert!(shared.turn_running());
    autopilot
        .on_thread_stop(ThreadStopInput {
            session_store: &session,
            thread_store: &store,
        })
        .await;
    assert!(!shared.turn_running(), "the thread's stop ends it");
    assert_eq!(flushed(&flushes).len(), 3, "one write-through per start");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_thread_that_is_not_the_autopilots_leaves_the_record_alone() {
    // Negative control: a thread with no ThreadRt (a sub-agent, exec) counts no turn and
    // writes nothing. It still invalidates an upgrade's asynchronous loaded-thread scan.
    let (autopilot, shared, flushes) = autopilot();
    let store = ExtensionData::new(ThreadId::new().to_string());
    let turn = ExtensionData::new("turn-1");
    let scanned = shared.upgrade_generation();
    start(&autopilot, &store, &turn).await;
    assert!(!shared.turn_running());
    assert!(flushed(&flushes).is_empty());
    assert_ne!(shared.upgrade_generation(), scanned);

    let scanned = shared.upgrade_generation();
    let session = ExtensionData::new("session");
    autopilot
        .on_turn_stop(TurnStopInput {
            session_store: &session,
            thread_store: &store,
            turn_store: &turn,
        })
        .await;
    assert!(!shared.turn_running());
    assert!(flushed(&flushes).is_empty());
    assert_ne!(shared.upgrade_generation(), scanned);
}

/// The worker's final reply `text`, as the turn's item stream completes it.
async fn reply(
    autopilot: &Autopilot,
    thread_store: &ExtensionData,
    turn: &ExtensionData,
    text: &str,
) {
    let item = TurnItem::AgentMessage(AgentMessageItem {
        moderation: None,
        id: "msg-1".to_string(),
        content: vec![AgentMessageContent::Text {
            text: text.to_string(),
        }],
        phase: Some(MessagePhase::FinalAnswer),
        memory_citation: None,
        delivery: None,
        questions: None,
    });
    autopilot.on_item_completed(thread_store, turn, &item).await;
}

/// The TUI's own root thread: registered by a frame, and kept by the autopilot.
fn root(shared: &Shared, rt: &ThreadRt) {
    shared.observe(
        &codex_alab_core::session::Frame {
            primary_thread: Some(rt.key()),
            displayed_thread: Some(rt.key()),
            rollout_path: None,
            cwd: std::path::Path::new("/work"),
            model: "gpt-5",
            codex_home: std::path::Path::new("/h/.codex"),
            embedded: true,
            state: codex_alab_core::session::ProgramState::Idle,
            draft_empty: true,
            queued_input: false,
            popup_open: false,
            editor_open: false,
            pending_user_turn: false,
        },
        1,
    );
    shared.note_root_thread(rt.key());
    assert!(shared.is_root_thread(rt.key()));
}

/// The opening of a turn codex-alab typed (its submission's `client_id`), or a person's.
async fn opened(
    autopilot: &Autopilot,
    thread_store: &ExtensionData,
    turn: &ExtensionData,
    ours: bool,
) {
    let item = TurnItem::UserMessage(codex_protocol::items::UserMessageItem {
        id: "user-1".to_string(),
        client_id: ours.then(|| format!("{}1", crate::extension::CLIENT_PREFIX)),
        content: vec![codex_protocol::user_input::UserInput::Text {
            text: "keep going".to_string(),
            text_elements: Vec::new(),
        }],
    });
    autopilot.on_item_completed(thread_store, turn, &item).await;
}

/// One whole turn on `store`: started, opened (codex-alab's own where `ours`), replied
/// `text`, then stopped (or aborted where `aborted`).
async fn turn_of(
    autopilot: &Autopilot,
    store: &ExtensionData,
    id: &str,
    ours: bool,
    text: &str,
    aborted: bool,
) {
    let session = ExtensionData::new("session");
    let turn = ExtensionData::new(id);
    start(autopilot, store, &turn).await;
    opened(autopilot, store, &turn, ours).await;
    reply(autopilot, store, &turn, text).await;
    if aborted {
        autopilot
            .on_turn_abort(TurnAbortInput {
                reason: TurnAbortReason::Interrupted,
                session_store: &session,
                thread_store: store,
                turn_store: &turn,
            })
            .await;
    } else {
        autopilot
            .on_turn_stop(TurnStopInput {
                session_store: &session,
                thread_store: store,
                turn_store: &turn,
            })
            .await;
    }
}

/// An autopilot whose turns count as real work after `progress` (`Timing::progress`).
fn autopilot_working(progress: Duration) -> (Arc<Autopilot>, &'static Shared) {
    let (base, shared, _flushes) = autopilot();
    let autopilot = Autopilot::new(
        Arc::clone(base.supervisor()),
        None,
        None,
        Timing {
            progress,
            ..Timing::default()
        },
        |_| true,
    );
    (autopilot, shared)
}

/// THE CARRY-ON CHAIN (CONTRACT §5 carry-on, amended 2026-10-19 for the aterm owner's FOREVER
/// CRUISE ruling: the worker's words never end a task, so its done no longer ends the chain):
/// it ends at a person's turn, or at a turn codex-alab typed that worked for real (at least
/// `Timing::progress`). NEGATIVE CONTROLS, under a progress no test turn reaches: a turn cut
/// short (the stall's restart aborts it), a clean end, a done phrase and the retired token
/// each leave the count standing, so relaunch, carry-on, stall again waits for the person.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_work_or_a_persons_turn_ends_the_carry_on_chain() {
    let (autopilot, shared) = autopilot_working(Duration::from_secs(3600));
    let (store, rt) = thread();
    root(shared, &rt);
    rt.with_state(|s| s.observe_resume(true, Some("Working through stage 2."), false));
    shared.set_carried_on(1);
    for (id, text, aborted) in [
        ("turn-1", "Working on it.", true),
        ("turn-2", "Stage 2 is half done; continuing next.", false),
        (
            "turn-3",
            "Build completed successfully; nothing left to do.",
            false,
        ),
        ("turn-4", "All landed.\nATERM-TASK-DONE", false),
    ] {
        turn_of(&autopilot, &store, id, true, text, aborted).await;
        assert_eq!(shared.carried_on(), 1, "{id}: {text:?} ends no chain");
    }
    // A person's turn ends it.
    turn_of(&autopilot, &store, "turn-5", false, "Done.", false).await;
    assert_eq!(shared.carried_on(), 0, "a person's turn ends the chain");

    // Real work: a turn codex-alab typed that worked at least the progress ends it.
    let (autopilot, shared) = autopilot_working(Duration::ZERO);
    let (store, rt) = thread();
    root(shared, &rt);
    rt.with_state(|s| s.observe_resume(true, Some("Working through stage 2."), false));
    shared.set_carried_on(1);
    // NEGATIVE CONTROL: the same turn cut short is no real work.
    turn_of(&autopilot, &store, "turn-1", true, "Working on it.", true).await;
    assert_eq!(shared.carried_on(), 1, "an aborted turn is no real work");
    turn_of(&autopilot, &store, "turn-2", true, "Stage 3 is in.", false).await;
    assert_eq!(shared.carried_on(), 0, "real work ends the chain");
}

/// Negative control: a side thread's turn (a title, a memory) is not the session's and ends
/// no chain, even one of real work codex-alab typed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_side_threads_turn_ends_no_carry_on_chain() {
    let (autopilot, shared) = autopilot_working(Duration::ZERO);
    let (store, _rt) = thread();
    shared.set_carried_on(1);
    turn_of(&autopilot, &store, "turn-1", true, "ATERM-TASK-DONE", false).await;
    assert_eq!(shared.carried_on(), 1);
}

/// One command the thread `store` holds runs in `dir` (`on_command_start`).
pub(crate) async fn command_in(autopilot: &Autopilot, store: &ExtensionData, dir: &str) {
    use codex_extension_api::CommandStartInput;
    use codex_extension_api::ToolLifecycleContributor;
    let cwd = codex_utils_path_uri::PathUri::parse(&format!("file://{dir}"))
        .unwrap_or_else(|err| panic!("{dir}: {err}"));
    let session = ExtensionData::new("session");
    let turn = ExtensionData::new("turn-1");
    let command = ["git".to_string(), "status".to_string()];
    autopilot
        .on_command_start(CommandStartInput {
            session_store: &session,
            thread_store: store,
            turn_store: &turn,
            turn_id: "turn-1",
            call_id: "call-1",
            command: &command,
            cwd: &cwd,
            file_system: codex_exec_server::LOCAL_FS.as_ref(),
        })
        .await;
}

/// THE WORKER'S FOLDER IS THE ONE ITS LAST COMMAND RAN IN (review of 2a9241bbf, finding 6):
/// each command the thread runs moves it, and the repository probe reads it before the
/// thread's own folder. NEGATIVE CONTROL: another thread's command leaves it.
#[tokio::test]
async fn the_workers_folder_is_where_its_last_command_ran() {
    let (autopilot, _shared, _flushes) = autopilot();
    let (store, rt) = thread();
    let (other, _other_rt) = thread();
    assert_eq!(rt.workdir(), None);
    for dir in ["/work/feat", "/work/main"] {
        command_in(&autopilot, &store, dir).await;
        assert_eq!(rt.workdir(), Some(std::path::PathBuf::from(dir)));
    }
    // NEGATIVE CONTROL: another thread's command.
    command_in(&autopilot, &other, "/elsewhere").await;
    assert_eq!(rt.workdir(), Some(std::path::PathBuf::from("/work/main")));
}

/// No conversation history (the tool-start hook's snapshot, which the autopilot never reads).
struct NoHistory;

impl codex_extension_api::ConversationHistorySnapshot for NoHistory {
    fn history_version(&self) -> u64 {
        0
    }
    fn user_message_revision(&self) -> u64 {
        0
    }
    fn items(&self) -> Box<dyn Iterator<Item = &codex_extension_api::ResponseItem> + Send + '_> {
        Box::new(std::iter::empty())
    }
}

/// One tool named `name` starts in `turn` (`on_tool_start`).
async fn tool_in(autopilot: &Autopilot, store: &ExtensionData, turn: &ExtensionData, name: &str) {
    // `ns/name` is a namespaced tool (an MCP server's), a bare name a plain one, which core's
    // router names in the default namespace (`ToolName::with_default_namespace`), as here.
    let tool_name = match name.split_once('/') {
        Some((namespace, name)) => codex_extension_api::ToolName::namespaced(namespace, name),
        None => codex_extension_api::ToolName::plain(name).with_default_namespace(),
    };
    tool_named(autopilot, store, turn, &tool_name, "{}").await;
}

/// The tool `tool_name` starts in `turn` with `arguments` (`on_tool_start`).
async fn tool_named(
    autopilot: &Autopilot,
    store: &ExtensionData,
    turn: &ExtensionData,
    tool_name: &codex_extension_api::ToolName,
    arguments: &str,
) {
    use codex_extension_api::ToolLifecycleContributor;
    use codex_extension_api::ToolStartInput;
    let session = ExtensionData::new("session");
    let payload = codex_extension_api::ToolPayload::Function {
        arguments: arguments.to_string(),
    };
    autopilot
        .on_tool_start(ToolStartInput {
            serving_assessment: None,
            session_store: &session,
            thread_store: store,
            turn_store: turn,
            turn_id: turn.level_id(),
            root_turn_id: None,
            call_id: "call-1",
            originating_item_id: None,
            tool_name,
            mcp_tool: None,
            permissions: Box::pin(async { None }),
            payload: &payload,
            conversation_history: Arc::new(NoHistory),
            source: codex_extension_api::ToolCallSource::Direct,
        })
        .await;
}

/// A TURN THAT STARTED A TOOL RAN TOOLS (ruling F's real work, `TurnEnd::ran_tools`): a
/// command's tool start marks the turn, and its end carries it; so does a namespaced tool
/// (an MCP server's) that happens to be named as the plan's, and a hosted web search or image
/// generation, which reach the autopilot as items and never start a tool (aterm counts Claude
/// Code's `⏺ Web Search(…)` row as a call). NEGATIVE CONTROLS: a turn whose only tools are
/// the plan's and a question's, either kind (which aterm's readers draw no call row for), ran
/// none, and so does a turn with no tool.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_that_started_a_tool_ran_tools_but_a_plan_or_a_question_is_none() {
    let (autopilot, shared, _flushes) = autopilot();
    let searched = TurnItem::WebSearch(codex_protocol::items::WebSearchItem {
        id: "search-1".to_string(),
        query: "rust lifetimes".to_string(),
        action: codex_protocol::models::WebSearchAction::Search {
            query: Some("rust lifetimes".to_string()),
            queries: None,
        },
        results: None,
    });
    let drew = TurnItem::ImageGeneration(codex_protocol::items::ImageGenerationItem {
        id: "image-1".to_string(),
        status: "completed".to_string(),
        revised_prompt: None,
        result: String::new(),
        saved_path: None,
    });
    for (id, tools, hosted, want) in [
        ("turn-1", &["update_plan", "exec_command"][..], None, true),
        (
            "turn-2",
            &["update_plan", "request_user_input"][..],
            None,
            false,
        ),
        ("turn-3", &[][..], None, false),
        ("turn-4", &["request_user_input_async"][..], None, false),
        ("turn-5", &["mcp_plans/update_plan"][..], None, true),
        ("turn-6", &["update_plan"][..], Some(&searched), true),
        ("turn-7", &[][..], Some(&drew), true),
    ] {
        let (store, rt) = thread();
        root(shared, &rt);
        rt.with_state(ThreadState::observe_person_turn);
        let session = ExtensionData::new("session");
        let turn = ExtensionData::new(id);
        start(&autopilot, &store, &turn).await;
        opened(&autopilot, &store, &turn, true).await;
        for name in tools {
            tool_in(&autopilot, &store, &turn, name).await;
        }
        if let Some(item) = hosted {
            autopilot.on_item_completed(&store, &turn, item).await;
        }
        reply(&autopilot, &store, &turn, "Stage 2 is in.").await;
        autopilot
            .on_turn_stop(TurnStopInput {
                session_store: &session,
                thread_store: &store,
                turn_store: &turn,
            })
            .await;
        let point = rt
            .state()
            .point
            .unwrap_or_else(|| panic!("{id}: its point"));
        assert_eq!(point.ran_tools, want, "{id}: {tools:?}");
    }
}

/// A PLAIN TOOL IS ONE IN CORE'S DEFAULT NAMESPACE (review of the forever-cruise lane, finding
/// 1): core's router names every plain tool `functions` (`ToolName::with_default_namespace`,
/// upstream f21dc46388), so a hook that took only a namespace of none for plain read no plain
/// tool at all: a goal tool never marked its turn ([`crate::extension::wall_goal_after`] then
/// bound a goal the model blocked at a person's request, and the wall's try resumed it), the
/// plan's step never reached the description, and the plan's and a question's tools counted as
/// work ([`a_turn_that_started_a_tool_ran_tools_but_a_plan_or_a_question_is_none`], whose tools
/// are now named as core names them). Each spelling of the default namespace is plain.
/// NEGATIVE CONTROLS: an MCP server's tools of the same names mark nothing and set no step, and
/// another plain tool is no goal tool.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_plain_tool_core_names_in_its_default_namespace_is_read_as_plain() {
    use codex_extension_api::ToolName;
    let (autopilot, shared, _flushes) = autopilot();
    let plan = r#"{"plan":[{"step":"write the parser","status":"in_progress"}]}"#;
    for (namespace, plain) in [
        (Some("functions"), true),
        (None, true),
        (Some(""), true),
        (Some("mcp_goals"), false),
    ] {
        let (store, rt) = thread();
        root(shared, &rt);
        let turn = ExtensionData::new("turn-1");
        start(&autopilot, &store, &turn).await;
        let named = |name: &str| ToolName::new(namespace.map(str::to_string), name);
        tool_named(&autopilot, &store, &turn, &named("exec_command"), "{}").await;
        assert!(
            !crate::extension::turn_called_goal_tool(&turn),
            "{namespace:?}: another tool is no goal tool"
        );
        tool_named(&autopilot, &store, &turn, &named("update_plan"), plan).await;
        assert_eq!(
            rt.plan_step(),
            plain.then(|| "write the parser".to_string()),
            "{namespace:?}"
        );
        for goal_tool in [
            codex_goal_extension::UPDATE_GOAL_TOOL_NAME,
            codex_goal_extension::CREATE_GOAL_TOOL_NAME,
        ] {
            let (store, rt) = thread();
            root(shared, &rt);
            let turn = ExtensionData::new("turn-2");
            start(&autopilot, &store, &turn).await;
            tool_named(&autopilot, &store, &turn, &named(goal_tool), "{}").await;
            assert_eq!(
                crate::extension::turn_called_goal_tool(&turn),
                plain,
                "{namespace:?} {goal_tool}"
            );
        }
    }
}
