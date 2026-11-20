// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::CwdPromptAction;
use super::CwdPromptScreen;
use super::CwdSelection;
use crate::render::Insets;
use crate::render::renderable::FlexRenderable;
use crate::render::renderable::RenderableExt as _;
use ratatui::style::Stylize as _;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Wrap;
use std::fs::Metadata;
use std::io;
use std::io::ErrorKind;
use std::path::Path;

fn definitely_not_a_directory(metadata: io::Result<Metadata>) -> bool {
    match metadata {
        Ok(metadata) => !metadata.is_dir(),
        Err(error) => matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory),
    }
}

pub(super) async fn set_default(
    screen: &mut CwdPromptScreen,
    enhanced: bool,
    local_workspace: bool,
    saved: &Path,
    current: &Path,
) {
    if enhanced
        && local_workspace
        && screen.action == CwdPromptAction::Resume
        && definitely_not_a_directory(tokio::fs::metadata(saved).await)
        // Looking up "." inside the directory also checks search permission on Unix.
        && tokio::fs::metadata(current.join("."))
            .await
            .is_ok_and(|metadata| metadata.is_dir())
    {
        screen.default_selection = CwdSelection::Current;
        screen.highlighted = CwdSelection::Current;
    }
}

pub(super) fn explain_default(screen: &CwdPromptScreen, column: &mut FlexRenderable<'_>) {
    if screen.default_selection == CwdSelection::Current {
        column.push(
            /*flex*/ 0,
            Paragraph::new("The saved session directory is unavailable.")
                .dim()
                .wrap(Wrap { trim: false })
                .inset(Insets::vh(/*v*/ 0, /*h*/ 2)),
        );
    }
}

pub(super) fn escape_hint(screen: &CwdPromptScreen) -> &'static str {
    if screen.default_selection == CwdSelection::Current {
        " use current · "
    } else {
        " use session · "
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_backend::VT100Backend;
    use crate::tui::FrameRequester;
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;
    use pretty_assertions::assert_eq;
    use ratatui::Terminal;
    use ratatui::widgets::FrameExt;
    use std::fs;
    use tempfile::tempdir;

    async fn prompt(
        saved: &Path,
        current: &Path,
        enhanced: bool,
        local: bool,
        action: CwdPromptAction,
    ) -> CwdPromptScreen {
        let mut screen = CwdPromptScreen::new(
            FrameRequester::test_dummy(),
            action,
            current.display().to_string(),
            saved.display().to_string(),
            current.display().to_string(),
            /*allow_remember_current*/ true,
        );
        set_default(&mut screen, enhanced, local, saved, current).await;
        screen
    }

    #[tokio::test]
    async fn moved_local_directory_renders_current_and_enter_resumes_there() {
        let temp = tempdir().expect("temporary project");
        let saved = temp.path().join("original");
        let current = temp.path().join("moved");
        fs::create_dir(&saved).expect("create saved project");
        fs::rename(&saved, &current).expect("move project");
        let mut screen = prompt(&saved, &current, true, true, CwdPromptAction::Resume).await;
        let mut terminal =
            Terminal::new(VT100Backend::new(/*width*/ 100, /*height*/ 17)).expect("terminal");
        terminal
            .draw(|frame| frame.render_widget_ref(&screen, frame.area()))
            .expect("paint prompt");
        let rendered = terminal.backend().to_string();
        assert!(rendered.contains("The saved session directory is unavailable."));
        assert!(rendered.contains("› 2. Use current directory"));
        assert!(rendered.contains("esc use current"));

        screen.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let choice = screen.selection().expect("selected default");
        assert_eq!(choice, CwdSelection::Current);
        assert_eq!(choice.selected_cwd(&current, &saved, &current), current);
        assert_eq!(choice.remembered_mode(), None);
        assert!(!saved.exists());
    }

    #[tokio::test]
    async fn missing_directory_escape_uses_default_and_explicit_session_is_preserved() {
        let temp = tempdir().expect("temporary directory");
        let current = temp.path();
        let saved = current.join("not-a-directory");
        fs::write(&saved, b"replacement file").expect("replace project with file");
        for saved in [&saved, &saved.join("nested")] {
            for (key, expected) in [
                (KeyCode::Esc, CwdSelection::Current),
                (KeyCode::Enter, CwdSelection::Session),
                (KeyCode::Char('1'), CwdSelection::Session),
                (KeyCode::Char('3'), CwdSelection::SessionAndRemember),
            ] {
                let mut screen = prompt(saved, current, true, true, CwdPromptAction::Resume).await;
                assert_eq!(screen.highlighted, CwdSelection::Current);
                screen.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
                screen.handle_key(KeyEvent::new(key, KeyModifiers::NONE));
                assert_eq!(screen.selection(), Some(expected));
            }
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn unsearchable_current_directory_keeps_session_when_traversal_is_denied() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempdir().expect("temporary directory");
        let current = temp.path().join("current");
        fs::create_dir(&current).expect("create current directory");
        fs::set_permissions(&current, fs::Permissions::from_mode(0o600))
            .expect("remove search permission");
        // A privileged process may still traverse it. Test the requirement using this process's permissions.
        let expected = match fs::metadata(current.join(".")) {
            Ok(_) => CwdSelection::Current,
            Err(error) if error.kind() == ErrorKind::PermissionDenied => CwdSelection::Session,
            Err(error) => panic!("unexpected directory probe: {error}"),
        };
        let screen = prompt(
            &temp.path().join("missing"),
            &current,
            true,
            true,
            CwdPromptAction::Resume,
        )
        .await;
        fs::set_permissions(&current, fs::Permissions::from_mode(0o700))
            .expect("restore search permission");
        assert_eq!(screen.highlighted, expected);
    }

    #[tokio::test]
    async fn existing_remote_fork_and_unavailable_current_keep_session_default() {
        let temp = tempdir().expect("temporary directory");
        let existing = temp.path();
        let missing = existing.join("missing");
        for (saved, current, enhanced, local, action) in [
            (existing, existing, true, true, CwdPromptAction::Resume),
            (&missing, existing, false, true, CwdPromptAction::Resume),
            (&missing, existing, true, false, CwdPromptAction::Resume),
            (&missing, existing, true, true, CwdPromptAction::Fork),
            (&missing, &missing, true, true, CwdPromptAction::Resume),
        ] {
            let mut screen = prompt(saved, current, enhanced, local, action).await;
            assert_eq!(escape_hint(&screen), " use session · ");
            screen.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            assert_eq!(screen.selection(), Some(CwdSelection::Session));
        }
        assert!(!definitely_not_a_directory(Err(
            ErrorKind::PermissionDenied.into()
        )));
        assert!(!definitely_not_a_directory(Err(
            ErrorKind::Interrupted.into()
        )));
    }
}
