// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Apply the saved experience mode through the native config loader.

use super::App;
use crate::app_command::AppCommand;

impl App {
    pub(super) async fn alab_reload_mode(&mut self, tui: &mut crate::tui::Tui) {
        // The person's live choice outranks even command-line defaults and survives
        // a failed preference write. Native reloads must not restore the older mode.
        self.cli_kv_overrides
            .retain(|(key, _)| key != "tui.alab_interface");
        self.cli_kv_overrides.push((
            "tui.alab_interface".into(),
            toml::Value::Boolean(codex_alab_tui::interface::enabled()),
        ));
        if !codex_alab_tui::interface::enabled() {
            self.alab_cancel_account_email_read();
        }
        let old_features: Vec<_> = codex_features::FEATURES
            .iter()
            .map(|spec| {
                (
                    spec.id,
                    self.chat_widget.config_ref().features.enabled(spec.id),
                )
            })
            .collect();
        if let Err(error) = self.refresh_in_memory_config_from_disk().await {
            self.chat_widget
                .add_error_message(format!("Could not reload experience mode: {error}"));
            return;
        }
        // Preserve live thread/model/permission choices while adopting the effective
        // preferences and updating the native caches associated with changed features.
        self.chat_widget
            .alab_adopt_tui_preferences(&self.local_settings.tui);
        let notifications = &self.local_settings.tui.notification_settings;
        tui.set_notification_settings(notifications.method, notifications.condition);
        for (feature, previous) in old_features {
            let enabled = self.config.features.enabled(feature);
            if enabled != previous {
                self.chat_widget.set_feature_enabled(feature, enabled);
            }
        }
        if let Err(error) = tui.clear_ambient_pet_image() {
            tracing::debug!(%error, "could not clear the previous experience's pet image");
        }
        self.chat_widget.refresh_status_surfaces();
        self.chat_widget.submit_op(AppCommand::reload_user_config());
        self.alab_selected_widget_replaced();
    }
}

#[cfg(test)]
mod tests {
    use crate::app::tests::make_test_app_with_channels;
    use codex_alab_tui::interface;
    use codex_utils_absolute_path::AbsolutePathBuf;

    #[tokio::test]
    async fn mode_reload_preserves_the_live_thread_model_permissions_and_draft() {
        let state = Box::leak(Box::new(interface::State::new()));
        let _scope = interface::scoped(state);
        interface::configure(false);
        let (mut app, _, _) = make_test_app_with_channels().await;
        let home = tempfile::tempdir().unwrap();
        app.config.codex_home = AbsolutePathBuf::from_absolute_path(home.path()).unwrap();
        std::fs::write(
            home.path().join("config.toml"),
            "[tui]\ndisable_paste_burst = false\nstatus_line = []\n",
        )
        .unwrap();
        app.chat_widget
            .apply_external_edit("keep the unsent draft".into());
        let model = app.chat_widget.current_model().to_owned();
        let thread = app.chat_widget.thread_id();
        let permissions = app
            .chat_widget
            .config_ref()
            .permissions
            .approval_policy
            .value();
        let mut tui = crate::tui::test_support::make_test_tui().unwrap();
        app.alab_reload_mode(&mut tui).await;
        assert_eq!(app.chat_widget.current_model(), model);
        assert_eq!(app.chat_widget.thread_id(), thread);
        assert_eq!(
            app.chat_widget
                .config_ref()
                .permissions
                .approval_policy
                .value(),
            permissions
        );
        assert_eq!(
            app.chat_widget.composer_text_with_pending(),
            "keep the unsent draft"
        );
        assert_eq!(
            app.chat_widget.local_settings.tui.status_line,
            Some(Vec::new())
        );
        assert_eq!(
            app.chat_widget.local_settings.tui.disable_paste_burst,
            Some(false)
        );
        assert_eq!(
            app.cli_kv_overrides.last().unwrap(),
            &("tui.alab_interface".into(), toml::Value::Boolean(false))
        );
    }
}
