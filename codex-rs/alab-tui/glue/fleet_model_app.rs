// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::App;
use crate::app_event::AppEvent;
use crate::app_server_session::AppServerSession;
use crate::app_server_session::ThreadParamsMode;
use codex_alab_tui::model_policy::Delivery;
use codex_alab_tui::model_policy::Endpoint;
use codex_alab_tui::model_policy::Outcome;
use codex_alab_tui::model_policy::{self};
use codex_app_server_protocol::ThreadSettingsUpdateParams;
use codex_protocol::openai_models::ReasoningEffort;

pub(super) fn listen(app: &App) -> Option<Endpoint> {
    if !codex_alab_core::is_alab_binary() {
        return None;
    }
    if let Err(error) = codex_alab_tui::model_homes::register(&app.config.codex_home) {
        tracing::warn!(%error,"model settings home registration unavailable");
    }
    let tx = app.app_event_tx.clone();
    model_policy::listen(
        &app.config.codex_home,
        uuid::Uuid::new_v4().as_u64_pair().0,
        move |d| tx.send(AppEvent::AlabModelPolicy(d)),
    )
    .inspect_err(|error| tracing::warn!(%error, "model policy endpoint unavailable"))
    .ok()
}

pub(super) async fn apply(app: &mut App, server: &mut AppServerSession, delivery: Delivery) {
    let (apply, reply) = match delivery {
        Delivery::Identify(reply) => {
            let _ = reply.send(
                app.primary_thread_id
                    .or(app.active_thread_id)
                    .map(|id| id.to_string()),
            );
            return;
        }
        Delivery::Apply { apply, reply } => (apply, reply),
    };
    if reply.is_closed() {
        return;
    }
    let target = &apply.target;
    let outcome = if server.thread_params_mode() != ThreadParamsMode::Embedded
        || !app.config.alab_uses_sol_default()
    {
        Outcome::Unsupported
    } else if !app.active_thread_id.is_some_and(|id| {
        id.to_string() == target.thread && Some(id) == app.chat_widget.thread_id()
    }) || app
        .primary_thread_id
        .is_some_and(|id| id.to_string() != target.thread)
    {
        Outcome::Held
    } else {
        let Ok(models) = server.request_model_list().await else {
            let _ = reply.send(Outcome::Pending);
            return;
        };
        let Some(index) = codex_models_manager::sol_family::newest_eligible_sol(
            &models,
            Some(&ReasoningEffort::Ultra),
        ) else {
            let _ = reply.send(Outcome::Unsupported);
            return;
        };
        if reply.is_closed() {
            return;
        }
        // A tab's stated hold also fences a settings change issued by Fleet Admin.
        if codex_alab_tui::session::shared()
            .launch()
            .is_some_and(codex_alab_tui::session::inside_aterm)
        {
            let open = match codex_alab_tui::aterm::Client::connect_self().await {
                Ok(Some(client)) => client.status().await.is_ok_and(|status| !status.hold),
                _ => false,
            };
            if !open {
                let _ = reply.send(Outcome::Held);
                return;
            }
        }
        if reply.is_closed() {
            return;
        }
        let model = models[index].model.clone();
        let effort = Some(ReasoningEffort::Ultra);
        let mode = app.chat_widget.effective_collaboration_mode().with_updates(
            Some(model.clone()),
            Some(effort.clone()),
            None,
        );
        let params = ThreadSettingsUpdateParams {
            thread_id: target.thread.clone(),
            model: Some(model.clone()),
            effort,
            collaboration_mode: Some(mode),
            ..Default::default()
        };
        // This RPC awaits the core operation. Its ordinary notification updates the TUI.
        match server.thread_settings_update(params).await {
            Ok(true) => Outcome::Applied {
                model,
                effort: "ultra".into(),
            },
            Ok(false) => Outcome::Unsupported,
            Err(_) => Outcome::Held,
        }
    };
    let _ = reply.send(outcome);
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_alab_tui::model_policy::Apply;
    use codex_alab_tui::model_policy::Selection;
    use codex_alab_tui::model_policy::Target;
    use codex_app_server_client::AppServerEvent;
    use codex_app_server_protocol::ServerNotification;
    use codex_app_server_protocol::ThreadSettings;
    use codex_protocol::config_types::ReasoningSummary;

    #[test]
    fn native_update_keeps_thread_draft_and_requires_the_original_thread() -> color_eyre::Result<()>
    {
        run_native_update(
            concat!(
                module_path!(),
                "::native_update_keeps_thread_draft_and_requires_the_original_thread"
            ),
            /*cancel_before_dispatch*/ false,
        )
    }

    #[test]
    fn canceled_fleet_delivery_cannot_change_the_serving_threads_settings() -> color_eyre::Result<()>
    {
        run_native_update(
            concat!(
                module_path!(),
                "::canceled_fleet_delivery_cannot_change_the_serving_threads_settings"
            ),
            /*cancel_before_dispatch*/ true,
        )
    }

    fn run_native_update(name: &str, cancel_before_dispatch: bool) -> color_eyre::Result<()> {
        if !core_test_support::alab_test_process::child(name) {
            return Ok(());
        }
        std::thread::Builder::new()
            .name("fleet-model-thread".into())
            .stack_size(8 * 1024 * 1024)
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(1)
                    .thread_stack_size(8 * 1024 * 1024)
                    .enable_all()
                    .build()?;
                runtime.block_on(Box::pin(check_native_update(cancel_before_dispatch)))
            })?
            .join()
            .expect("model settings test thread")
    }

    async fn check_native_update(cancel_before_dispatch: bool) -> color_eyre::Result<()> {
        codex_alab_core::activate();
        let (mut app, _events, _ops) =
            Box::pin(crate::app::tests::make_test_app_with_channels()).await;
        let mut server = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
        let started = server.start_thread(&app.config).await?;
        let thread = started.session.thread_id;
        app.enqueue_primary_thread_session(started.session, started.turns)
            .await?;
        let models = codex_models_manager::bundled_models_response()?
            .models
            .into_iter()
            .map(codex_protocol::openai_models::ModelPreset::from)
            .collect::<Vec<_>>();
        let i = codex_models_manager::sol_family::newest_eligible_sol(
            &models,
            Some(&ReasoningEffort::Ultra),
        )
        .expect("released Sol");
        let expected = models[i].model.clone();
        app.chat_widget
            .apply_external_edit("this is an unsent draft".into());
        let mut target = Target {
            pid: std::process::id(),
            identity: (&codex_alab_tui::identity::current()?).into(),
            generation: 1,
            thread: thread.to_string(),
        };
        let request = |target| {
            let (reply, rx) = tokio::sync::oneshot::channel();
            (
                Delivery::Apply {
                    apply: Apply {
                        operation: "aa".repeat(16),
                        target,
                        selection: Selection::LatestSolUltra,
                    },
                    reply,
                },
                rx,
            )
        };
        let mut tui = crate::tui::test_support::make_test_tui()?;
        if cancel_before_dispatch {
            assert!(
                server
                    .thread_settings_update(ThreadSettingsUpdateParams {
                        thread_id: target.thread.clone(),
                        effort: Some(ReasoningEffort::Low),
                        summary: Some(ReasoningSummary::Concise),
                        ..Default::default()
                    })
                    .await?
            );
            let before = next_settings(&mut server, &target.thread).await?;
            assert_eq!(before.effort, Some(ReasoningEffort::Low));
            assert_eq!(before.summary, Some(ReasoningSummary::Concise));

            let (delivery, receipt) = request(target.clone());
            drop(receipt);
            Box::pin(app.handle_event(&mut tui, &mut server, AppEvent::AlabModelPolicy(delivery)))
                .await?;

            // This independent, awaited write makes a forbidden Fleet mutation observable,
            // even when its notification is still queued. It preserves the model and effort.
            assert!(
                server
                    .thread_settings_update(ThreadSettingsUpdateParams {
                        thread_id: target.thread.clone(),
                        summary: Some(ReasoningSummary::Detailed),
                        ..Default::default()
                    })
                    .await?
            );
            let after = next_settings(&mut server, &target.thread).await?;
            assert_eq!(after.summary, Some(ReasoningSummary::Detailed));
            assert_eq!(after.model, before.model);
            assert_eq!(after.effort, before.effort);
        }
        let (event, receipt) = request(target.clone());
        Box::pin(app.handle_event(&mut tui, &mut server, AppEvent::AlabModelPolicy(event))).await?;
        assert_eq!(
            receipt.await?,
            Outcome::Applied {
                model: expected.clone(),
                effort: "ultra".into()
            }
        );
        let settings = next_settings(&mut server, &target.thread).await?;
        assert_eq!(settings.model, expected);
        assert_eq!(settings.effort, Some(ReasoningEffort::Ultra));
        assert_eq!(
            app.chat_widget.composer_text_with_pending(),
            "this is an unsent draft"
        );
        target.thread = "replaced-thread".into();
        let (event, receipt) = request(target);
        Box::pin(app.handle_event(&mut tui, &mut server, AppEvent::AlabModelPolicy(event))).await?;
        assert_eq!(receipt.await?, Outcome::Held);
        server.shutdown().await?;
        Ok(())
    }

    async fn next_settings(
        server: &mut AppServerSession,
        thread: &str,
    ) -> color_eyre::Result<ThreadSettings> {
        Ok(
            tokio::time::timeout(std::time::Duration::from_secs(8), async {
                loop {
                    let Some(event) = server.next_event().await else {
                        panic!("server closed")
                    };
                    if let AppServerEvent::ServerNotification(n) = event.event
                        && let ServerNotification::ThreadSettingsUpdated(s) = *n
                        && s.thread_id == thread
                    {
                        break s.thread_settings;
                    }
                }
            })
            .await?,
        )
    }
}
