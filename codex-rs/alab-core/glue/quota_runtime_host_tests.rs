// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionRegistry;
use codex_extension_api::TurnStartPhase;
use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::ModeKind;
use codex_protocol::config_types::Settings;
use codex_protocol::protocol::InternalSessionSource;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::SubAgentSource;

fn registry(runtime_is_exec: bool) -> ExtensionRegistry<()> {
    let mut builder = ExtensionRegistryBuilder::new();
    install_for(true, &mut builder, runtime_is_exec);
    builder.build()
}

#[tokio::test]
async fn restored_member_provenance_is_retained_only_in_an_exec_host() {
    let session = ExtensionData::new("session");
    let source = SessionSource::Internal(InternalSessionSource::MemoryConsolidation);
    for runtime_is_exec in [false, true] {
        let registry = registry(runtime_is_exec);
        let thread = ExtensionData::new("restored-child");
        let turn = ExtensionData::new("original-turn");
        thread.insert(codex_core::ExecRestoredMember);
        thread.insert(HeadlessExec);
        open(&registry, &source, &session, &thread).await;
        start(&registry, &session, &thread, &turn).await;
        assert_eq!(thread.get::<HeadlessExec>().is_some(), runtime_is_exec);
        assert_eq!(turn.get::<HeadlessExec>().is_some(), runtime_is_exec);
    }
}

async fn open(
    registry: &ExtensionRegistry<()>,
    source: &SessionSource,
    session: &ExtensionData,
    thread: &ExtensionData,
) {
    for contributor in registry.thread_lifecycle_contributors() {
        contributor
            .on_thread_start(ThreadStartInput {
                config: &(),
                session_source: source,
                persistent_thread_state_available: true,
                environments: &[],
                mcp_resource_client: None,
                extension_metrics: None,
                session_store: session,
                thread_store: thread,
            })
            .await;
    }
}

async fn start(
    registry: &ExtensionRegistry<()>,
    session: &ExtensionData,
    thread: &ExtensionData,
    turn: &ExtensionData,
) {
    let mode = CollaborationMode {
        mode: ModeKind::Default,
        settings: Settings {
            model: "fixture".into(),
            reasoning_effort: None,
            developer_instructions: None,
        },
    };
    for contributor in registry.turn_lifecycle_contributors() {
        assert_eq!(
            contributor.turn_start_phase(thread),
            TurnStartPhase::BeforeTaskRegistration
        );
        assert!(!contributor.requires_mcp_runtime(thread));
        contributor
            .on_turn_start(TurnStartInput {
                turn_id: turn.level_id(),
                collaboration_mode: &mode,
                token_usage_at_turn_start: None,
                session_store: session,
                thread_store: thread,
                turn_store: turn,
            })
            .await;
    }
}

#[tokio::test]
async fn exec_host_admits_resumed_root_origins_without_changing_them() {
    let registry = registry(true);
    let session = ExtensionData::new("exec-session");
    for source in [
        SessionSource::Cli,
        SessionSource::VSCode,
        SessionSource::Exec,
    ] {
        let saved = source.clone();
        let thread = ExtensionData::new("resumed-thread");
        let turn = ExtensionData::new("resumed-turn");
        open(&registry, &source, &session, &thread).await;
        assert!(thread.get::<HeadlessExec>().is_some());
        assert!(turn.get::<HeadlessExec>().is_none());
        start(&registry, &session, &thread, &turn).await;
        assert!(turn.get::<HeadlessExec>().is_some());
        assert_eq!(source, saved, "admission must not rewrite saved provenance");
    }
}

#[tokio::test]
async fn embedded_exec_and_non_exec_registries_do_not_share_admission() {
    let exec = registry(true);
    let interactive = registry(false);
    let exec_session = ExtensionData::new("exec-session");
    let interactive_session = ExtensionData::new("interactive-session");
    let exec_thread = ExtensionData::new("exec-thread");
    let interactive_thread = ExtensionData::new("interactive-thread");
    let exec_turn = ExtensionData::new("exec-turn");
    let interactive_turn = ExtensionData::new("interactive-turn");
    let stored_exec = SessionSource::Exec;
    let stored_cli = SessionSource::Cli;
    // Stale or inherited attachments cannot confer another host's admission.
    interactive_thread.insert(HeadlessExec);
    interactive_turn.insert(HeadlessExec);
    interactive_thread.insert(ExecQuotaMember);
    interactive_turn.insert(ExecQuotaMember);
    tokio::join!(
        open(&exec, &stored_cli, &exec_session, &exec_thread),
        open(
            &interactive,
            &stored_exec,
            &interactive_session,
            &interactive_thread
        ),
    );
    tokio::join!(
        start(&exec, &exec_session, &exec_thread, &exec_turn),
        start(
            &interactive,
            &interactive_session,
            &interactive_thread,
            &interactive_turn
        ),
    );
    assert!(exec_turn.get::<HeadlessExec>().is_some());
    assert!(interactive_thread.get::<HeadlessExec>().is_none());
    assert!(interactive_turn.get::<HeadlessExec>().is_none());
    assert!(interactive_thread.get::<ExecQuotaMember>().is_none());
    assert!(interactive_turn.get::<ExecQuotaMember>().is_none());
}

#[tokio::test]
async fn internal_and_child_threads_keep_their_existing_recovery_policy() {
    let registry = registry(true);
    let session = ExtensionData::new("exec-session");
    for source in [
        SessionSource::Internal(InternalSessionSource::Guardian),
        SessionSource::SubAgent(SubAgentSource::Review),
    ] {
        let thread = ExtensionData::new("child-thread");
        let turn = ExtensionData::new("child-turn");
        thread.insert(HeadlessExec);
        turn.insert(HeadlessExec);
        open(&registry, &source, &session, &thread).await;
        start(&registry, &session, &thread, &turn).await;
        assert!(thread.get::<HeadlessExec>().is_none());
        assert!(turn.get::<HeadlessExec>().is_none());
        assert!(thread.get::<ExecQuotaMember>().is_some());
        assert!(turn.get::<ExecQuotaMember>().is_some());
    }
}

#[test]
fn inactive_consumers_register_nothing() {
    let mut builder = ExtensionRegistryBuilder::<()>::new();
    install_for(false, &mut builder, true);
    let registry = builder.build();
    assert!(registry.thread_lifecycle_contributors().is_empty());
    assert!(registry.turn_lifecycle_contributors().is_empty());
}
