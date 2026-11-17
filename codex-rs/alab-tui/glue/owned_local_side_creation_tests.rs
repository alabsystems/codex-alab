// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Whole UI DATA/ownership controls only. No Native/Source grant is made.
use super::*;
fn original() -> Result<AppServerStartedThread> {
    Ok(AppServerStartedThread {
        alab_construction: None,
        session: crate::session_state::ThreadSessionState {
            daybreak_enabled: false,
            windows_sandbox_host: crate::app::WindowsSandboxHost::Mixed,
            thread_id: codex_protocol::ThreadId::new(),
            forked_from_id: Some(codex_protocol::ThreadId::new()),
            fork_parent_title: Some("actual original parent label".into()),
            thread_name: Some("original child".into()),
            model: "original-model".into(),
            model_provider_id: "original-provider".into(),
            service_tier: Some("original-tier".into()),
            approval_policy: codex_app_server_protocol::AskForApproval::OnRequest,
            approvals_reviewer: codex_protocol::config_types::ApprovalsReviewer::User,
            permission_profile: codex_protocol::models::PermissionProfile::Disabled,
            active_permission_profile: Some(codex_protocol::models::ActivePermissionProfile {
                id: "actual-profile-data".into(),
                extends: None,
            }),
            cwd: codex_utils_absolute_path::AbsolutePathBuf::from_absolute_path("/tmp/original")?,
            runtime_workspace_roots: vec![
                codex_utils_absolute_path::AbsolutePathBuf::from_absolute_path("/tmp/original")?,
            ],
            instruction_source_paths: Vec::new(),
            reasoning_effort: Some(codex_protocol::openai_models::ReasoningEffort::High),
            collaboration_mode: None,
            message_history: Some(crate::session_state::MessageHistoryMetadata {
                log_id: u64::MAX,
                entry_count: usize::MAX,
            }),
            network_proxy: Some(crate::session_state::SessionNetworkProxyRuntime {
                http_addr: "original-http".into(),
                socks_addr: "original-socks".into(),
            }),
            rollout_path: Some(std::path::PathBuf::from("/tmp/original-rollout")),
        },
        turns: vec![serde_json::from_value(
            serde_json::json!({"id":"original-turn","items":[],"status":"completed"}),
        )?],
        blocks_direct_input: true,
        task_tools_available: false,
    })
}
#[test]
fn exact_original_started_ui_data_roundtrips_without_a_resource_backlink_or_numeric_loss()
-> Result<()> {
    let original = original()?;
    let data = StartedData::capture(&original);
    let wire = serde_json::to_value(data)?;
    let restored: StartedData = serde_json::from_value(wire.clone())?;
    let restored = restored.restore()?;
    assert_eq!(original.session, restored.session);
    assert_eq!(original.turns, restored.turns);
    assert_eq!(original.blocks_direct_input, restored.blocks_direct_input);
    assert_eq!(original.task_tools_available, restored.task_tools_available);
    assert!(restored.alab_construction.is_none());
    let mut foreign = wire;
    foreign["controller"] = serde_json::json!("not a resource");
    assert!(serde_json::from_value::<StartedData>(foreign).is_err());
    Ok(())
}
#[test]
fn same_record_retains_side_fields_errors_and_no_strong_backlink_cycle() -> Result<()> {
    let (target, record) =
        crate::owned_local_frontend::ConstructionRecord::side_preparation_fixture()?;
    let original = original()?;
    let items = vec![
        serde_json::json!({"type":"message","role":"user","content":[{"type":"input_text","text":"actual original policy data"}]}),
    ];
    let actual = record.capture_side_creation(
        original.clone(),
        SideChatStorage::Temporary,
        true,
        true,
        items.clone(),
    )?;
    let same = record.capture_side_creation(
        original.clone(),
        SideChatStorage::Temporary,
        true,
        true,
        items.clone(),
    )?;
    assert!(Arc::ptr_eq(&actual, &same));
    drop(same);
    assert!(
        record
            .capture_side_creation(
                original.clone(),
                SideChatStorage::Attached,
                true,
                true,
                items
            )
            .is_err()
    );
    actual.sidebar(&Err("exact original boundary failure".into()));
    let whole = actual
        .observed_sidebar()?
        .ok_or_else(|| eyre!("whole original object lost"))?;
    assert_eq!(whole.thread.session, original.session);
    assert_eq!(whole.thread.turns, original.turns);
    assert_eq!(
        whole.boundary,
        Err("exact original boundary failure".into())
    );
    assert_eq!(whole.storage, SideChatStorage::Temporary);
    assert!(whole.inherited_profile && whole.legacy);
    drop(whole);
    let weak = Arc::downgrade(&record);
    drop(record);
    drop(target);
    assert!(weak.upgrade().is_none());
    assert!(
        actual.observed_sidebar().is_err(),
        "checkpoint DATA reconstructed the dropped actual caller"
    );
    Ok(())
}
#[test]
fn pending_rollback_requires_observed_original_operation_before_phase_advance() -> Result<()> {
    let (_target, record) =
        crate::owned_local_frontend::ConstructionRecord::side_preparation_fixture()?;
    let actual = record.capture_side_creation(
        original()?,
        SideChatStorage::Attached,
        false,
        false,
        Vec::new(),
    )?;
    assert!(
        actual
            .begin_rollback(&Err("original boundary failure".into()))
            .is_err(),
        "lower UI fixture fabricated an observed client rollback"
    );
    let value = serde_json::to_value(actual.checkpoint())?;
    assert_eq!(value["phase"]["phase"], "injecting");
    assert!(value["rollback"].is_null());
    assert!(actual.observed_sidebar()?.is_none());
    let _: SideCreationCheckpoint = serde_json::from_value(value)?;
    Ok(())
}

#[test]
fn side_phase_rejects_extra_unit_fields_and_storage_requires_an_actual_string() -> Result<()> {
    for phase in ["injecting", "unknown"] {
        let _: Phase = serde_json::from_value(serde_json::json!({"phase":phase}))?;
        assert!(
            serde_json::from_value::<Phase>(
                serde_json::json!({"phase":phase,"nativeCurrent":true})
            )
            .is_err()
        );
    }
    for value in ["temporary", "attached"] {
        let _: Storage = serde_json::from_value(serde_json::json!(value))?;
    }
    assert!(serde_json::from_value::<Storage>(serde_json::json!({"temporary":null})).is_err());
    let mut wire = serde_json::to_value(StartedData::capture(&original()?))?;
    wire["session"]["windowsSandboxHost"] = serde_json::json!({"mixed":null});
    assert!(serde_json::from_value::<StartedData>(wire).is_err());
    Ok(())
}
