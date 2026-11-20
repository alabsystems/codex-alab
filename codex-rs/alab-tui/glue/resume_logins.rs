// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The enhanced startup picker opens all local history. Each page uses its own
//! backend's workspace ownership and the login choice from its listing generation.

use super::AppServerPickerBackend;
use super::PickerState;
use super::SessionPickerLaunchContext;
use super::SessionSelection;
use super::ToolbarControl;
use super::toolbar_value;
use crate::app_server_session::AppServerSession;
use crate::app_server_session::ThreadParamsMode;
use crate::custom_terminal::Frame;
use crate::keymap::ListAction;
use crate::named_session_lookup;
use crate::style::secondary_text_style;
use codex_app_server_protocol::ThreadListParams;
use ratatui::layout::Rect;
use ratatui::style::Styled;
use ratatui::text::Line;
use ratatui::text::Span;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum LoginScope {
    Unavailable,
    Current,
    AllLocal,
}

pub(super) fn eligible_backend(backend: &AppServerPickerBackend) -> bool {
    local_history(
        backend.session.thread_params_mode(),
        backend.uses_remote_filesystem,
    )
}

fn local_history(mode: ThreadParamsMode, remote_filesystem: bool) -> bool {
    mode == ThreadParamsMode::Embedded && !remote_filesystem
}

impl AppServerSession {
    /// A startup label must be unique across the same local conversations its picker shows.
    pub(crate) async fn alab_startup_resume_target(
        &mut self,
        config: &crate::Config,
        id_or_name: &str,
        remote_filesystem: bool,
    ) -> color_eyre::Result<Option<super::SessionTarget>> {
        if uuid::Uuid::parse_str(id_or_name).is_ok()
            || !codex_alab_core::active()
            || !local_history(self.thread_params_mode(), remote_filesystem)
        {
            return crate::lookup_session_target_with_app_server(
                super::SessionPickerAction::Resume,
                self,
                config,
                id_or_name,
            )
            .await;
        }

        codex_alab_tui::LocalThreadStore::repair_local_resume_index(config, /*archived*/ false).await?;
        let thread = named_session_lookup::lookup(
            self,
            config.codex_home.as_path(),
            id_or_name,
            &[named_session_lookup::SessionCollection::Active],
            &[crate::resume_source_kinds(
                /*include_non_interactive*/ true,
            )],
            Some(Vec::new()),
        )
        .await?;
        Ok(thread.and_then(|thread| {
            crate::session_target_from_app_server_thread(self.backend_id(), thread)
        }))
    }
}

impl LoginScope {
    pub(super) fn for_resume(
        launch: SessionPickerLaunchContext,
        backends: &[AppServerPickerBackend],
        show_all: bool,
    ) -> (Self, bool) {
        Self::initial_filters(
            codex_alab_core::active(),
            launch,
            backends.iter().any(eligible_backend),
            show_all,
        )
    }

    fn initial_filters(
        enhanced: bool,
        launch: SessionPickerLaunchContext,
        local: bool,
        show_all: bool,
    ) -> (Self, bool) {
        if enhanced && launch == SessionPickerLaunchContext::Startup && local {
            (Self::AllLocal, true)
        } else {
            (Self::Unavailable, show_all)
        }
    }

    pub(super) fn available(self) -> bool {
        self != Self::Unavailable
    }

    pub(super) async fn repair_initial_page(
        self,
        params: &ThreadListParams,
        local_backend: bool,
        config: Option<&impl codex_rollout::RolloutConfigView>,
        listing_token: usize,
        last_check: &mut Option<usize>,
    ) {
        if self == Self::AllLocal
            && local_backend
            && params.cursor.is_none()
            && *last_check != Some(listing_token)
            && let Some(config) = config
        {
            *last_check = Some(listing_token);
            if let Err(error) =
                codex_alab_tui::LocalThreadStore::repair_local_resume_index(config, params.archived.unwrap_or(false))
                    .await
            {
                tracing::debug!(%error, "could not reconcile all local resume history before listing");
            }
        }
    }

    pub(super) fn apply(
        self,
        mut params: ThreadListParams,
        local_backend: bool,
    ) -> ThreadListParams {
        if self.available() && local_backend {
            params.current_account_only = self == Self::Current;
            params.source_kinds = Some(crate::resume_source_kinds(true));
        }
        params
    }
}

/// Metadata-only views share each source's original request transport. The picker loop
/// still owns the source connections until accepting this selection completes.
pub(super) fn cwd_readers(
    scope: LoginScope,
    backends: &[AppServerPickerBackend],
) -> Vec<AppServerSession> {
    backends
        .iter()
        .filter(|backend| scope.available() && eligible_backend(backend))
        .map(|backend| backend.session.request_view())
        .collect()
}

pub(super) async fn refresh_selected_cwd(
    readers: &mut [AppServerSession],
    state: &PickerState,
    mut selection: SessionSelection,
) -> SessionSelection {
    if let SessionSelection::Resume(target) = &mut selection
        && let Some(reader) = readers
            .iter_mut()
            .find(|reader| reader.backend_id() == target.backend_id)
        && let Some(row) = state.filtered_rows.get(state.selected)
        && row.thread_id == Some(target.thread_id)
        && row.backend_id.unwrap_or(state.backend_id) == target.backend_id
    {
        // The ordinary row remains the fallback if its owner cannot respond promptly.
        // thread_read validates the response UUID before returning any metadata.
        let read = reader.thread_read(target.thread_id, /*include_turns*/ false);
        let observation = tokio::time::timeout(std::time::Duration::from_secs(3), read)
            .await
            .ok()
            .and_then(Result::ok)
            .map(|thread| (thread.updated_at, thread.cwd.to_path_buf()));
        update_cwd(
            &mut target.cwd,
            row.updated_at.map(|seen| seen.timestamp()),
            observation,
        );
    }
    selection
}

fn update_cwd(
    cwd: &mut Option<std::path::PathBuf>,
    listed_at: Option<i64>,
    observation: Option<(i64, std::path::PathBuf)>,
) {
    if let Some((updated_at, observed_cwd)) = observation
        && listed_at.is_none_or(|seen| updated_at >= seen)
    {
        *cwd = Some(observed_cwd);
    }
}

impl PickerState {
    pub(super) fn render_empty_login_hint(&self, frame: &mut Frame, area: Rect) {
        if area.height < 2
            || self.login_scope != LoginScope::Current
            || !self.query.is_empty()
            || self.pagination.is_loading()
        {
            return;
        }

        let Some(key) = self
            .keymap
            .list
            .primary_hint(ListAction::MoveRight)
            .or_else(|| self.keymap.list.primary_hint(ListAction::MoveLeft))
        else {
            return;
        };
        let label = if self.toolbar_focus == ToolbarControl::Logins {
            format!("{} All local", key.display_label())
        } else {
            String::from("Tab to Logins")
        };
        let line = Line::from(label).set_style(secondary_text_style());
        frame.render_widget_ref(
            &line,
            Rect::new(area.x, area.y.saturating_add(1), area.width, 1),
        );
    }

    pub(super) fn toggle_login_scope(&mut self) {
        self.login_scope = match self.login_scope {
            LoginScope::Unavailable => return,
            LoginScope::Current => LoginScope::AllLocal,
            LoginScope::AllLocal => LoginScope::Current,
        };
        self.start_initial_load();
    }

    pub(super) fn append_login_control(
        &self,
        spans: &mut Vec<Span<'static>>,
        compact: bool,
        separator: &'static str,
    ) {
        if self.login_scope.available() {
            spans.extend(self.login_control(compact));
            spans.push(separator.set_style(secondary_text_style()));
        }
    }

    pub(super) fn login_control(&self, compact: bool) -> Vec<Span<'static>> {
        let focused = self.toolbar_focus == ToolbarControl::Logins;
        let mut spans = vec!["Logins:".set_style(secondary_text_style())];
        for (scope, label) in [
            (LoginScope::Current, "Current"),
            (LoginScope::AllLocal, "All local"),
        ] {
            let active = self.login_scope == scope;
            if !compact || active {
                spans.push(toolbar_value(label, active, focused));
            }
        }
        spans
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppServerTarget;
    use crate::RemoteAppServerEndpoint;
    use crate::resume_picker::BackgroundEvent;
    use crate::resume_picker::ProviderFilter;
    use crate::resume_picker::SessionFilterMode;
    use crate::resume_picker::SessionStatus;
    use crate::resume_picker::tests::local_db_first_state;
    use crate::resume_picker::tests::make_row;
    use crate::resume_picker::tests::page;
    use crate::resume_picker::tests::render_picker_list;
    use crate::resume_picker::thread_list_params;
    use codex_app_server_protocol::ThreadSortKey;
    use codex_app_server_protocol::ThreadSourceKind;
    use codex_utils_absolute_path::AbsolutePathBuf;
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;

    #[test]
    fn selected_cwd_keeps_the_listed_value_when_read_is_unavailable_or_older() {
        use std::path::PathBuf;
        let initial = Some(PathBuf::from("/listed"));
        let observed = PathBuf::from("/updated");
        for response in [None, Some((9, observed.clone()))] {
            let mut cwd = initial.clone();
            super::update_cwd(&mut cwd, Some(10), response);
            assert_eq!(cwd, initial);
        }
        // The public protocol reports whole seconds; a later update may share the second.
        for listed in [None, Some(9), Some(10)] {
            let mut cwd = initial.clone();
            super::update_cwd(&mut cwd, listed, Some((10, observed.clone())));
            assert_eq!(cwd.as_ref(), Some(&observed));
        }
    }

    #[test]
    fn startup_defaults_follow_the_local_workspace_owner() {
        let home = tempfile::tempdir().expect("fixture directory");
        let socket = RemoteAppServerEndpoint::UnixSocket {
            socket_path: AbsolutePathBuf::from_absolute_path(home.path().join("server.sock"))
                .expect("absolute fixture socket"),
        };
        let startup = SessionPickerLaunchContext::Startup;
        let existing = SessionPickerLaunchContext::ExistingSession {
            current_thread_id: None,
        };
        for (target, local) in [
            (AppServerTarget::Embedded, true),
            (
                AppServerTarget::LocalDaemon {
                    endpoint: socket.clone(),
                    allow_embedded_fallback: false,
                },
                true,
            ),
            (AppServerTarget::Remote { endpoint: socket }, false),
            (AppServerTarget::Cloud, false),
        ] {
            let mode = target.thread_params_mode();
            assert_eq!(local_history(mode, false), local);
            assert!(!local_history(mode, true));
            for show_all in [false, true] {
                let ordinary = (LoginScope::Unavailable, show_all);
                assert_eq!(
                    LoginScope::initial_filters(false, startup, local, show_all),
                    ordinary
                );
                assert_eq!(
                    LoginScope::initial_filters(true, existing, local, show_all),
                    ordinary
                );
                assert_eq!(
                    LoginScope::initial_filters(true, startup, local, show_all),
                    if local {
                        (LoginScope::AllLocal, true)
                    } else {
                        ordinary
                    }
                );
            }
        }
    }

    #[test]
    fn only_expanded_local_requests_widen_discovery() {
        for scope in [
            LoginScope::Unavailable,
            LoginScope::Current,
            LoginScope::AllLocal,
        ] {
            for local in [true, false] {
                for include_non_interactive in [true, false] {
                    let params = thread_list_params(
                        Some("next page".into()),
                        None,
                        SessionStatus::Active,
                        ProviderFilter::All,
                        ThreadSortKey::UpdatedAt,
                        include_non_interactive,
                        true,
                    );
                    let mut expected = params.clone();
                    if local && scope.available() {
                        expected.current_account_only = scope == LoginScope::Current;
                        expected.source_kinds = Some(vec![
                            ThreadSourceKind::Cli,
                            ThreadSourceKind::VsCode,
                            ThreadSourceKind::Exec,
                            ThreadSourceKind::AppServer,
                        ]);
                    }
                    assert_eq!(scope.apply(params, local), expected);
                }
            }
        }
    }

    #[test]
    fn empty_local_picker_shows_how_to_include_other_logins() {
        let (mut state, _) = local_db_first_state();
        let render = |state: &PickerState| render_picker_list(state, 16, 2);
        state.login_scope = LoginScope::Current;
        assert!(render(&state).contains("Tab to Logins"));

        state.toolbar_focus = ToolbarControl::Logins;
        state.keymap.list.move_right = vec![crate::key_hint::ctrl(KeyCode::Char('u'))];
        assert!(render(&state).contains("⌃u All local"));
        state.keymap.list.move_right.clear();
        state.keymap.list.move_left.clear();
        assert!(!render(&state).contains("All local"));

        state.keymap.list.move_left = vec![crate::key_hint::ctrl(KeyCode::Char('h'))];
        assert!(render(&state).contains("⌃h All local"));
        state.query = String::from("other");
        assert!(!render(&state).contains("All local"));
        state.query.clear();
        for scope in [LoginScope::AllLocal, LoginScope::Unavailable] {
            state.login_scope = scope;
            assert!(!render(&state).contains("All local"));
        }
        state.login_scope = LoginScope::Current;
        state.start_initial_load();
        assert!(!render(&state).contains("All local"));
    }

    #[tokio::test]
    async fn all_local_startup_can_narrow_the_directory_and_login_independently() {
        let (mut state, requests) = local_db_first_state();
        let cwd = std::path::PathBuf::from("project");
        let (login, all_directories) =
            LoginScope::initial_filters(true, SessionPickerLaunchContext::Startup, true, false);
        state.login_scope = login;
        state.filter_cwd = Some(cwd.clone());
        state.filter_mode =
            SessionFilterMode::from_show_all(all_directories, state.filter_cwd.as_deref());
        state.start_initial_load();
        let first = requests.lock().unwrap().last().unwrap().clone();
        assert_eq!(first.cwd_filter, None);
        assert_eq!(first.login_scope, LoginScope::AllLocal);
        let right = KeyEvent::new(KeyCode::Right, KeyModifiers::NONE);
        state
            .handle_key(right)
            .await
            .expect("narrow to the current project");
        let project = requests.lock().unwrap().last().unwrap().clone();
        assert_eq!(project.cwd_filter.as_ref(), Some(&cwd));
        assert_eq!(project.login_scope, LoginScope::AllLocal);
        state
            .handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .await
            .expect("focus logins");
        state
            .handle_key(right)
            .await
            .expect("narrow to the current login");
        let current = requests.lock().unwrap().last().unwrap().clone();
        assert_eq!(current.cwd_filter.as_ref(), Some(&cwd));
        assert_eq!(current.login_scope, LoginScope::Current);
    }

    #[tokio::test]
    async fn toggle_reloads_and_a_late_wider_page_cannot_reappear() {
        let (mut state, requests) = local_db_first_state();
        let tab = KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
        // No new key binding: the user's mapped list movement acts on the focused field.
        state.keymap.list.move_right = vec![crate::key_hint::ctrl(KeyCode::Char('u'))];
        let right = KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL);
        state.handle_key(tab).await.expect("focus status");
        assert_eq!(state.toolbar_focus, ToolbarControl::Status);
        state.toolbar_focus = ToolbarControl::Filter;
        state.login_scope = LoginScope::Current;
        state.start_initial_load();
        state.handle_key(tab).await.expect("focus logins");
        assert_eq!(state.toolbar_focus, ToolbarControl::Logins);
        state
            .handle_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL))
            .await
            .expect("unbound key");
        assert_eq!(state.login_scope, LoginScope::Current);
        state.handle_key(right).await.expect("expand local listing");
        let expanded = requests.lock().unwrap().last().unwrap().clone();
        assert_eq!(expanded.login_scope, LoginScope::AllLocal);
        let row = make_row("/local/account-a", "2026-09-01T00:00:00Z", "Account A work");
        state
            .handle_background_event(BackgroundEvent::PartialPage {
                listing_token: expanded.listing_token,
                page: page(vec![row.clone()], None, 1, false),
            })
            .await
            .expect("expanded result");
        assert_eq!(state.filtered_rows, vec![row.clone()]);
        state
            .handle_key(right)
            .await
            .expect("restore current login");
        assert_eq!(
            requests.lock().unwrap().last().unwrap().login_scope,
            LoginScope::Current
        );
        state
            .handle_background_event(BackgroundEvent::PartialPage {
                listing_token: expanded.listing_token,
                page: page(vec![row], None, 1, false),
            })
            .await
            .expect("late scoped response");
        assert!(state.filtered_rows.is_empty());
    }
}

#[cfg(test)]
#[path = "resume_index_tests.rs"]
mod index_tests;
