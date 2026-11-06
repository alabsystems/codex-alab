// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::app_event::AppEvent;
use crate::bottom_pane::TerminalTitleItem;
use crate::chatwidget::tests::make_chatwidget_manual_with_sender;
use crate::history_cell::HistoryCell;
use crate::model_catalog::ModelCatalog;
use codex_features::Feature;
use codex_protocol::openai_models::ModelServiceTier;
use std::sync::Arc;

fn catalog(chat: &mut ChatWidget) {
    let mut model = chat.model_catalog.models.first().unwrap().clone();
    model.id = "speed-model".into();
    model.model = "speed-model".into();
    model.display_name = "Speed Model".into();
    model.default_service_tier = None;
    model.service_tiers = [
        ("priority", "priority"),
        ("ultrafast", "ultrafast"),
        ("flex", "Flex"),
        ("custom", "Batch Plus"),
    ]
    .into_iter()
    .map(|(id, name)| ModelServiceTier {
        id: id.into(),
        name: name.into(),
        description: String::new(),
    })
    .collect();
    chat.model_catalog = Arc::new(ModelCatalog::new(vec![model]));
    chat.set_model("speed-model");
    chat.set_feature_enabled(Feature::FastMode, true);
    chat.set_feature_enabled(Feature::UltrafastMode, true);
}

fn text(cell: &dyn HistoryCell) -> String {
    cell.display_lines(160)
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn supported_speed_tiers_reach_model_status_title_and_header_surfaces() {
    let _scope = interface::scoped(Box::leak(Box::new(interface::State::new())));
    let (mut chat, _tx, mut events, _ops) = make_chatwidget_manual_with_sender().await;
    catalog(&mut chat);
    for (id, label) in [("priority", "Fast"), ("ultrafast", "Ultrafast")] {
        chat.set_service_tier(Some(id.into()));
        assert_eq!(chat.current_service_tier(), Some(id));
        assert_eq!(
            chat.status_line_value_for_item(StatusLineItem::FastMode)
                .as_deref(),
            Some(label)
        );
        assert!(
            chat.status_line_value_for_item(StatusLineItem::ModelWithReasoning)
                .unwrap()
                .ends_with(label)
        );
        assert_eq!(
            chat.terminal_title_value_for_item(
                TerminalTitleItem::FastMode,
                std::time::Instant::now()
            )
            .as_deref(),
            Some(label)
        );
        assert!(
            chat.terminal_title_value_for_item(
                TerminalTitleItem::ModelWithReasoning,
                std::time::Instant::now()
            )
            .unwrap()
            .ends_with(label)
        );
        let model = format!("Speed Model · {label}");
        assert_eq!(chat.alab_model_with_service_tier(), model);

        while events.try_recv().is_ok() {}
        chat.add_status_output(false, None);
        let status = std::iter::from_fn(|| events.try_recv().ok())
            .find_map(|event| match event {
                AppEvent::InsertHistoryCell(cell) => Some(text(cell.as_ref())),
                _ => None,
            })
            .expect("status history cell");
        assert!(status.contains(&model), "{status}");

        let header = crate::history_cell::SessionHeaderHistoryCell::new(
            chat.alab_model_with_service_tier(),
            chat.current_reasoning_effort(),
            chat.config.cwd.to_path_buf(),
            "test-version",
        );
        assert!(text(&header).contains(&model));
    }
}

#[tokio::test]
async fn labels_follow_independent_speed_gates_and_do_not_invent_support() {
    let _scope = interface::scoped(Box::leak(Box::new(interface::State::new())));
    let (mut chat, _tx, _events, _ops) = make_chatwidget_manual_with_sender().await;
    catalog(&mut chat);
    chat.set_feature_enabled(Feature::FastMode, false);
    chat.set_service_tier(Some("ultrafast".into()));
    assert_eq!(chat.processing_tier_display_name(), Some("Ultrafast"));
    chat.set_feature_enabled(Feature::UltrafastMode, false);
    assert_eq!(chat.processing_tier_display_name(), None);
    assert_eq!(chat.alab_model_with_service_tier(), "Speed Model");

    chat.set_feature_enabled(Feature::FastMode, true);
    chat.set_service_tier(Some("custom".into()));
    assert_eq!(chat.processing_tier_display_name(), Some("Batch Plus"));
    chat.set_service_tier(Some("flex".into()));
    assert_eq!(chat.processing_tier_display_name(), Some("Flex"));
    chat.set_service_tier(Some("default".into()));
    assert_eq!(chat.processing_tier_display_name(), None);
    chat.set_service_tier(Some("unsupported".into()));
    assert_eq!(chat.processing_tier_display_name(), None);
    assert_eq!(chat.alab_model_with_service_tier(), "Speed Model");

    chat.set_model("uncatalogued-model");
    chat.set_service_tier(Some("priority".into()));
    assert_eq!(
        chat.status_line_value_for_item(StatusLineItem::FastMode)
            .as_deref(),
        Some("Standard")
    );
    assert_eq!(chat.alab_model_with_service_tier(), "uncatalogued-model");
}

#[tokio::test]
async fn startup_header_keeps_saved_model_and_reasoning_when_plan_model_differs() {
    use codex_app_server_protocol::AskForApproval;
    use codex_protocol::ThreadId;
    use codex_protocol::config_types::ApprovalsReviewer;
    use codex_protocol::config_types::CollaborationMode;
    use codex_protocol::config_types::ModeKind;
    use codex_protocol::config_types::Settings;
    use codex_protocol::models::PermissionProfile;
    use codex_protocol::openai_models::ReasoningEffort;

    for (enhanced, tier) in [
        (true, Some("priority")),
        (false, Some("priority")),
        (true, None),
    ] {
        let _scope = interface::scoped(Box::leak(Box::new(interface::State::new())));
        interface::configure(enhanced);
        let (mut chat, _tx, mut events, _ops) = make_chatwidget_manual_with_sender().await;
        catalog(&mut chat);
        Arc::make_mut(&mut chat.model_catalog).models[0].default_service_tier =
            Some("priority".into());
        let mut plan = chat.model_catalog.models[0].clone();
        plan.id = "plan-model".into();
        plan.model = "plan-model".into();
        plan.display_name = "Plan Model".into();
        plan.service_tiers.retain(|tier| tier.id == "ultrafast");
        Arc::make_mut(&mut chat.model_catalog).models.push(plan);
        chat.local_settings.tui.show_tooltips = false;
        while events.try_recv().is_ok() {}
        chat.handle_thread_session(crate::session_state::ThreadSessionState {
            daybreak_enabled: false,
            windows_sandbox_host: crate::app::WindowsSandboxHost::Local,
            thread_id: ThreadId::new(),
            forked_from_id: None,
            fork_parent_title: None,
            thread_name: None,
            model: "speed-model".into(),
            model_provider_id: "openai".into(),
            service_tier: tier.map(str::to_owned),
            approval_policy: AskForApproval::Never,
            approvals_reviewer: ApprovalsReviewer::User,
            permission_profile: PermissionProfile::read_only(),
            active_permission_profile: None,
            cwd: chat.config.cwd.clone(),
            runtime_workspace_roots: vec![chat.config.cwd.clone()],
            instruction_source_paths: Vec::new(),
            reasoning_effort: Some(ReasoningEffort::Low),
            collaboration_mode: Some(Box::new(CollaborationMode {
                mode: ModeKind::Plan,
                settings: Settings {
                    model: "plan-model".into(),
                    reasoning_effort: Some(ReasoningEffort::High),
                    developer_instructions: None,
                },
            })),
            message_history: None,
            network_proxy: None,
            rollout_path: None,
        });
        assert_eq!(chat.current_model(), "plan-model");
        let cells = std::iter::from_fn(|| events.try_recv().ok())
            .filter_map(|event| match event {
                AppEvent::InsertHistoryCell(cell) => Some(cell),
                _ => None,
            })
            .collect::<Vec<_>>();
        let rendered = cells
            .iter()
            .map(|cell| text(cell.as_ref()))
            .collect::<Vec<_>>()
            .join("\n");
        let raw = cells
            .iter()
            .flat_map(|cell| cell.raw_lines())
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(raw.contains("Speed Model"), "{raw}");
        assert!(raw.contains("low"), "{raw}");
        assert!(!raw.contains("Plan Model"), "{raw}");
        assert_eq!(rendered.contains("Speed Model"), enhanced, "{rendered}");
        assert_eq!(
            raw.contains("Speed Model · Fast"),
            enhanced && tier.is_some(),
            "{raw}"
        );
        assert_eq!(
            rendered.contains("Speed Model · Fast"),
            enhanced && tier.is_some(),
            "{rendered}"
        );
    }
}

#[test]
fn enhanced_header_speed_row_stays_inside_narrow_terminal_widths() {
    let _scope = interface::scoped(Box::leak(Box::new(interface::State::new())));
    let header = crate::history_cell::SessionHeaderHistoryCell::new(
        "模型 e\u{301} processing · Ultrafast".into(),
        Some(codex_protocol::openai_models::ReasoningEffort::High),
        std::path::PathBuf::from("project"),
        "test-version",
    );
    for width in [0, 1, 20, 44] {
        for line in header.display_lines(width) {
            assert!(crate::line_truncation::line_width(&line) <= usize::from(width));
        }
    }
}
