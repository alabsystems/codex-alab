// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::*;
use codex_protocol::protocol::EnvironmentConfigState;
use codex_protocol::protocol::TurnEnvironmentRequest;
use codex_protocol::protocol::TurnEnvironmentRequests;
use codex_utils_path_uri::PathUri;

async fn source_settings() -> CodexThreadSettingsOverrides {
    let (session, _) = crate::session::tests::make_session_and_context().await;
    let state = session.state.lock().await;
    let config = state.session_configuration.inferred_environment_config();
    let cwd = state.session_configuration.legacy_fallback_cwd.clone();
    CodexThreadSettingsOverrides {
        environments: Some(TurnEnvironmentRequests::new(
            cwd.clone(),
            vec![TurnEnvironmentRequest {
                environment_id: "local".into(),
                cwd: PathUri::from_abs_path(&cwd),
                workspace_roots: vec![PathUri::from_abs_path(&cwd)],
                config: EnvironmentConfigState::Ready(config),
            }],
        )),
        effort: Some(None),
        service_tier: Some(None),
        model: Some("original-model".into()),
        disabled_plugin_ids: Some(vec!["kept-disabled".into()]),
        ..Default::default()
    }
}

#[tokio::test]
async fn settings_round_trip_requires_fresh_bindings_and_preserves_explicit_clears() {
    #[derive(Clone)]
    struct FreshOwner;
    let source = source_settings().await;
    let saved = Settings::capture(&source).unwrap();
    let saved: Settings = serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
    let mut fresh = source.clone();
    let mut init = codex_extension_api::ExtensionDataInit::new();
    init.insert(FreshOwner);
    fresh.turn_extension_init = Some(init);
    let restored = saved.restore(&fresh).unwrap();
    assert_eq!(restored.environments, source.environments);
    assert_eq!(restored.effort, Some(None));
    assert_eq!(restored.service_tier, Some(None));
    assert_eq!(restored.model, source.model);
    assert_eq!(restored.disabled_plugin_ids, source.disabled_plugin_ids);
    assert!(
        restored
            .turn_extension_init
            .unwrap()
            .get::<FreshOwner>()
            .is_some()
    );
    assert!(
        saved
            .restore(&CodexThreadSettingsOverrides::default())
            .is_err()
    );
    assert!(Settings::capture(&fresh).is_err());
}

#[tokio::test]
async fn changed_environment_policy_or_identity_cannot_reuse_the_old_request() {
    let source = source_settings().await;
    let saved = Settings::capture(&source).unwrap();
    let mut changed = source.clone();
    let EnvironmentConfigState::Ready(config) =
        &mut changed.environments.as_mut().unwrap().environment_requests[0].config
    else {
        panic!("ready")
    };
    config.allow_login_shell = !config.allow_login_shell;
    assert!(saved.restore(&changed).is_err());
    let mut changed = source.clone();
    changed.environments.as_mut().unwrap().environment_requests[0].environment_id =
        "another-owner".into();
    assert!(saved.restore(&changed).is_err());
    let mut changed = source.clone();
    changed.environments.as_mut().unwrap().environment_requests[0]
        .workspace_roots
        .clear();
    assert!(saved.restore(&changed).is_err());
    let mut changed = source.clone();
    changed.environments.as_mut().unwrap().environment_requests[0].config =
        EnvironmentConfigState::Pending;
    assert!(saved.restore(&changed).is_err());
    let mut changed = source;
    let duplicate = changed.environments.as_ref().unwrap().environment_requests[0].clone();
    changed
        .environments
        .as_mut()
        .unwrap()
        .environment_requests
        .push(duplicate);
    assert!(Settings::capture(&changed).is_err());
}

#[tokio::test]
async fn managed_mcp_matchers_are_part_of_the_binding_check() {
    use codex_protocol::mcp_policy::*;
    let mut source = source_settings().await;
    let EnvironmentConfigState::Ready(config) =
        &mut source.environments.as_mut().unwrap().environment_requests[0].config
    else {
        panic!("ready")
    };
    config.mcp_policy = Some(EnvironmentMcpPolicy {
        servers: Some(std::collections::BTreeMap::from([(
            "worker".into(),
            McpServerRequirement::Command(McpServerCommandMatcher {
                executable: "worker-cli".into(),
                args: vec![McpServerValueMatcher::Prefix {
                    value: "--local=".into(),
                }],
            }),
        )])),
        plugins: None,
    });
    let saved = Settings::capture(&source).unwrap();
    assert!(saved.restore(&source).is_ok());
    let EnvironmentConfigState::Ready(config) =
        &mut source.environments.as_mut().unwrap().environment_requests[0].config
    else {
        panic!("ready")
    };
    config
        .mcp_policy
        .as_mut()
        .unwrap()
        .servers
        .as_mut()
        .unwrap()
        .clear();
    assert!(saved.restore(&source).is_err());
}

#[test]
fn history_evidence_keeps_private_mail_state_and_never_reuses_a_store_revision() {
    let thread = ThreadId::new();
    let history = codex_thread_store::StoredThreadHistory {
        thread_id: thread,
        revision: Some("old-process-revision".into()),
        items: vec![
            codex_history::RolloutItem::InterAgentCommunicationMetadata { trigger_turn: true },
        ],
    };
    let saved = HistoryEvidence::capture(&history).unwrap();
    let saved: HistoryEvidence =
        serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
    let restored = saved.restore(thread).unwrap();
    assert_eq!(restored.revision, None);
    assert!(matches!(
        &restored.items[0],
        codex_history::RolloutItem::InterAgentCommunicationMetadata { trigger_turn: true }
    ));
    assert!(saved.verify(&history).is_ok());
    assert!(saved.restore(ThreadId::new()).is_err());
    let mut changed = history;
    changed.items.clear();
    assert!(saved.verify(&changed).is_err());
}

#[tokio::test]
async fn malformed_optional_overrides_are_refused() {
    let source = source_settings().await;
    let mut value = serde_json::to_value(Settings::capture(&source).unwrap()).unwrap();
    value["effort"] = serde_json::json!([null, null]);
    assert!(serde_json::from_value::<Settings>(value).is_err());
}

#[tokio::test]
async fn capability_alias_order_must_match_the_new_owner() {
    use codex_protocol::capabilities::CapabilityRootLocation;
    use codex_protocol::capabilities::SelectedCapabilityRoot;
    let source = source_settings().await;
    let (session, _) = crate::session::tests::make_session_and_context().await;
    let config = session.get_config().await;
    let root = |id: &str| SelectedCapabilityRoot {
        id: id.into(),
        location: CapabilityRootLocation::Environment {
            environment_id: "local".into(),
            path: PathUri::from_abs_path(&config.cwd),
        },
    };
    let roots = vec![root("first"), root("second")];
    let node = ExecThreadSnapshot {
        review: None,
        thread: session.thread_id,
        parent: None,
        source: SessionSource::Exec,
        root_capabilities_restricted: false,
        configuration: ExecConfiguration::capture(&config, &Default::default()).unwrap(),
        settings: Settings::capture(&source).unwrap(),
        tools: Vec::new(),
        capability_roots: roots.clone(),
        ephemeral: None,
        helper: None,
        recovery: None,
        history: HistoryEvidence::capture(&codex_thread_store::StoredThreadHistory {
            thread_id: session.thread_id,
            items: Vec::new(),
            revision: None,
        })
        .unwrap(),
    };
    assert!(node.restore_settings(&source, &roots).is_ok());
    let mut reordered = roots;
    reordered.reverse();
    assert!(node.restore_settings(&source, &reordered).is_err());
}
