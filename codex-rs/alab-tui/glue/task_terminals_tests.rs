// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::app_command::AppCommand;
use crate::chatwidget::tests::make_chatwidget_manual_with_sender;
use crate::chatwidget::tests::render_bottom_popup;
use crate::slash_command::SlashCommand;
use codex_app_server_protocol::CommandExecutionSource;
use codex_app_server_protocol::CommandExecutionStatus;
use codex_app_server_protocol::ItemCompletedNotification;
use codex_app_server_protocol::ItemStartedNotification;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadItemPayload;

const THREAD: &str = "thread-terminals";
const TURN: &str = "turn-1";

/// The one command every terminal in this test runs: the footer counts processes, not
/// commands.
const COMMAND: &str = "sleep 600";

/// A unified exec startup item for `process`, as core reports one: `InProgress` at its item
/// start, `Completed` with exit code 0 at its completion.
fn startup(chat: &ChatWidget, process: &str, status: CommandExecutionStatus) -> ThreadItemPayload {
    let completed = status == CommandExecutionStatus::Completed;
    ThreadItemPayload::CommandExecution {
        model_context: None,
        sandbox_type: None,
        id: format!("call-{process}"),
        command: COMMAND.to_owned(),
        cwd: chat.config_ref().cwd.clone().into(),
        process_id: Some(process.to_owned()),
        plugin_id: None,
        script_path: None,
        source: CommandExecutionSource::UnifiedExecStartup,
        status,
        command_actions: Vec::new(),
        aggregated_output: None,
        exit_code: completed.then_some(0),
        duration_ms: None,
    }
}

fn start(chat: &mut ChatWidget, process: &str) {
    let item = startup(chat, process, CommandExecutionStatus::InProgress);
    chat.handle_server_notification(
        ServerNotification::ItemStarted(ItemStartedNotification {
            thread_id: THREAD.to_owned(),
            turn_id: TURN.to_owned(),
            started_at_ms: 0,
            item: ThreadItem::untimed(item),
        }),
        /*replay_kind*/ None,
    );
}

fn end(chat: &mut ChatWidget, process: &str) {
    let item = startup(chat, process, CommandExecutionStatus::Completed);
    chat.handle_server_notification(
        ServerNotification::ItemCompleted(ItemCompletedNotification::new(
            THREAD.to_owned(),
            TURN.to_owned(),
            item,
            /*started_at_ms*/ None,
            /*completed_at_ms*/ 0,
        )),
        /*replay_kind*/ None,
    );
}

/// What the chat widget draws: the footer's summary is on its own row, or inline in the status
/// row while the status indicator shows.
fn drawn(chat: &ChatWidget) -> String {
    render_bottom_popup(chat, /*width*/ 120)
}

/// The record's `backgroundTerminals` is the footer's count: every unified exec process the
/// chat widget holds (two of one command are two), a start counted from its item start whether
/// or not its call has yielded (the TUI sees no yield), one gone at its item's completion, and
/// all gone at `/stop`. The footer and the Tasks view read the one list this counts.
#[tokio::test]
async fn the_background_terminal_count_is_the_footers() {
    let (mut chat, _tx, _rx, mut op_rx) = make_chatwidget_manual_with_sender().await;
    assert_eq!(chat.alab_background_terminals(), 0);
    assert!(
        !drawn(&chat).contains("background terminal"),
        "no footer yet"
    );

    start(&mut chat, "proc-a");
    assert_eq!(chat.alab_background_terminals(), 1);
    start(&mut chat, "proc-b");
    assert_eq!(
        chat.alab_background_terminals(),
        2,
        "two processes of one command are two terminals"
    );
    assert!(
        drawn(&chat).contains("2 background terminals running"),
        "the footer counts the same two: {}",
        drawn(&chat)
    );
    assert_eq!(
        chat.alab_terminal_tasks().len(),
        2,
        "the footer and the Tasks view read one list"
    );

    end(&mut chat, "proc-a");
    assert_eq!(
        chat.alab_background_terminals(),
        1,
        "an end takes its terminal off the count"
    );
    assert!(drawn(&chat).contains("1 background terminal running"));
    assert_eq!(chat.alab_terminal_tasks().len(), 1);
    end(&mut chat, "proc-b");
    assert_eq!(chat.alab_background_terminals(), 0);
    assert!(!drawn(&chat).contains("background terminal"));

    start(&mut chat, "proc-c");
    start(&mut chat, "proc-d");
    assert_eq!(chat.alab_background_terminals(), 2);
    chat.dispatch_command(SlashCommand::Stop);
    assert!(matches!(
        op_rx.try_recv(),
        Ok(AppCommand::CleanBackgroundTerminals)
    ));
    assert_eq!(
        chat.alab_background_terminals(),
        0,
        "/stop clears the count"
    );
    assert!(chat.alab_terminal_tasks().is_empty());
}
