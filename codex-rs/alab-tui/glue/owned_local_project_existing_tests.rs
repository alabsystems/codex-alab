// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual canonical data-mapper/field controls only. No public Native resource,
//! receipt, Pending, subscription success or application witness is fabricated.
use super::*;
use crate::ThreadParamsMode;
use crate::session_state::MessageHistoryMetadata;
use crate::session_state::SessionNetworkProxyRuntime;
use codex_protocol::ThreadId;
use codex_protocol::models::ActivePermissionProfile;
use codex_protocol::models::PermissionProfile;
use codex_utils_absolute_path::AbsolutePathBuf;
use std::path::PathBuf;

fn path(value: &str) -> Result<AbsolutePathBuf> {
    Ok(AbsolutePathBuf::from_absolute_path(PathBuf::from(value))?)
}
fn turn(id: &str) -> Result<codex_app_server_protocol::Turn> {
    Ok(serde_json::from_value(
        serde_json::json!({"id":id,"items":[],"status":"completed"}),
    )?)
}
pub(super) fn response(id: ThreadId, parent: ThreadId) -> Result<ThreadResumeResponse> {
    let thread = serde_json::from_value(serde_json::json!({
        "id":id.to_string(),"sessionId":ThreadId::new().to_string(),"forkedFromId":parent.to_string(),
        "preview":"current visible history","ephemeral":false,"modelProvider":"current-provider",
        "createdAt":1,"updatedAt":2,"status":{"type":"idle"},"cwd":"/current/project",
        "cliVersion":"test","source":"cli","name":"current title","path":"/current/rollout.jsonl",
        "daybreakEnabled":true,"canAcceptDirectInput":false,"turns":[]
    }))?;
    Ok(ThreadResumeResponse {
        thread,
        model: "current-model".into(),
        model_provider: "current-provider".into(),
        service_tier: Some("current-tier".into()),
        disabled_plugin_ids: vec!["disabled-original-policy-data".into()],
        cwd: path("/current/project")?,
        runtime_workspace_roots: vec![path("/current/project")?, path("/current/extra")?],
        instruction_sources: vec![codex_utils_path_uri::LegacyAppPathString::from_abs_path(
            &path("/current/AGENTS.md")?,
        )],
        approval_policy: codex_app_server_protocol::AskForApproval::Never,
        approvals_reviewer: codex_app_server_protocol::ApprovalsReviewer::AutoReview,
        sandbox: codex_app_server_protocol::SandboxPolicy::ReadOnly {
            network_access: false,
        },
        active_permission_profile: Some(codex_app_server_protocol::ActivePermissionProfile {
            id: "current-profile".into(),
            extends: Some("current-parent-profile".into()),
        }),
        reasoning_effort: Some(codex_protocol::openai_models::ReasoningEffort::High),
        collaboration_mode: Some(codex_protocol::config_types::CollaborationMode {
            mode: codex_protocol::config_types::ModeKind::Plan,
            settings: codex_protocol::config_types::Settings {
                model: "current-model".into(),
                reasoning_effort: Some(codex_protocol::openai_models::ReasoningEffort::High),
                developer_instructions: Some("current collaboration".into()),
            },
        }),
        multi_agent_mode: Default::default(),
        initial_turns_page: None,
        turns_backwards_cursor: Some("actual-response-turn-cursor-data".into()),
        items_backwards_cursor: Some("actual-response-item-cursor-data".into()),
    })
}
pub(super) fn original(current: &ThreadSessionState) -> Result<AppServerStartedThread> {
    let mut session = current.clone();
    session.daybreak_enabled = false;
    session.windows_sandbox_host = crate::app::WindowsSandboxHost::Remote;
    session.thread_name = Some("construction title".into());
    session.model = "construction-model".into();
    session.model_provider_id = "construction-provider".into();
    session.service_tier = None;
    session.approval_policy = codex_app_server_protocol::AskForApproval::OnRequest;
    session.approvals_reviewer = codex_protocol::config_types::ApprovalsReviewer::User;
    session.permission_profile = PermissionProfile::Disabled;
    session.active_permission_profile = Some(ActivePermissionProfile {
        id: "construction-profile".into(),
        extends: None,
    });
    session.cwd = path("/construction/project")?;
    session.runtime_workspace_roots = vec![path("/construction/project")?];
    session.instruction_source_paths = Vec::new();
    session.reasoning_effort = None;
    session.collaboration_mode = None;
    session.rollout_path = Some(PathBuf::from("/construction/rollout.jsonl"));
    session.fork_parent_title = Some("resolved original parent title".into());
    session.message_history = Some(MessageHistoryMetadata {
        log_id: 101,
        entry_count: 7,
    });
    session.network_proxy = Some(SessionNetworkProxyRuntime {
        http_addr: "original-http".into(),
        socks_addr: "original-socks".into(),
    });
    Ok(AppServerStartedThread {
        #[cfg(unix)]
        alab_construction: None,
        session,
        turns: vec![turn("construction-turn")?],
        blocks_direct_input: false,
        task_tools_available: true,
    })
}
pub(super) async fn canonical(current: &ThreadResumeResponse) -> Result<ThreadSessionState> {
    let home = tempfile::tempdir()?;
    let config = crate::legacy_core::config::ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .build()
        .await
        .map_err(|error| eyre!(error.to_string()))?;
    thread_session_state_from_thread_resume_response(
        current,
        &crate::local_settings::LocalSettings::from(&config),
        ThreadParamsMode::Embedded,
    )
    .await
    .map_err(|error| eyre!(error))
}
#[tokio::test]
async fn named_runtime_patch_uses_canonical_mapping_and_preserves_original_only_state() -> Result<()>
{
    let response = response(ThreadId::new(), ThreadId::new())?;
    let current = canonical(&response).await?;
    let mut original = original(&current)?;
    let mut expected = current.clone();
    expected.fork_parent_title = original.session.fork_parent_title.clone();
    expected.message_history = original.session.message_history;
    expected.network_proxy = original.session.network_proxy.clone();
    let original_turns = original.turns.clone();
    let fields = RuntimeFields { canonical: current };
    fields.validate(&original.session)?;
    apply_current_fields(fields, &mut original, &response.thread, true);
    assert_eq!(original.session, expected);
    assert!(original.blocks_direct_input);
    assert_eq!(
        original.turns, original_turns,
        "runtime patch cannot replace original history policy"
    );
    assert!(
        original.task_tools_available,
        "runtime fields cannot invent a receipt observation"
    );
    assert_eq!(
        response.turns_backwards_cursor.as_deref(),
        Some("actual-response-turn-cursor-data")
    );
    assert_eq!(
        response.items_backwards_cursor.as_deref(),
        Some("actual-response-item-cursor-data")
    );
    Ok(())
}
#[tokio::test]
async fn side_runtime_refresh_keeps_original_daybreak_and_does_not_consume_fresh_history()
-> Result<()> {
    let mut response = response(ThreadId::new(), ThreadId::new())?;
    response.thread.turns = vec![turn("current-child-output")?];
    let current = canonical(&response).await?;
    let mut original = original(&current)?;
    let history = original.turns.clone();
    let fields = RuntimeFields { canonical: current };
    fields.validate(&original.session)?;
    apply_current_fields(fields, &mut original, &response.thread, false);
    assert!(!original.session.daybreak_enabled);
    assert_eq!(original.session.model, "current-model");
    assert_eq!(original.turns, history);
    assert_eq!(response.thread.turns[0].id, "current-child-output");
    assert_eq!(
        original.session.message_history,
        Some(MessageHistoryMetadata {
            log_id: 101,
            entry_count: 7
        })
    );
    assert_eq!(
        original
            .session
            .network_proxy
            .as_ref()
            .map(|proxy| proxy.http_addr.as_str()),
        Some("original-http")
    );
    Ok(())
}
#[tokio::test]
async fn different_current_thread_or_lineage_is_denied_before_original_field_mutation() -> Result<()>
{
    let response = response(ThreadId::new(), ThreadId::new())?;
    let current = canonical(&response).await?;
    let original = original(&current)?;
    let before = original.session.clone();
    for wrong_thread in [true, false] {
        let mut wrong = current.clone();
        if wrong_thread {
            wrong.thread_id = ThreadId::new();
        } else {
            wrong.forked_from_id = Some(ThreadId::new());
        }
        assert!(
            RuntimeFields { canonical: wrong }
                .validate(&original.session)
                .is_err()
        );
        assert_eq!(original.session, before);
    }
    Ok(())
}
