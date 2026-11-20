// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use crate::clipboard_copy::CopyFormat;
use crate::clipboard_copy::CopyStatus;
use crate::history_cell::AgentMarkdownCell;
use crate::history_cell::HistoryCell;
use crate::history_cell::SessionInfoCell;
use crate::history_cell::new_user_prompt;
use crate::transcript_view::TranscriptView;
use crate::transcript_view::ViewAction;
use color_eyre::eyre::OptionExt;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use std::path::Path;
use std::sync::Arc;

pub(super) fn check(card: &Arc<dyn HistoryCell>) -> color_eyre::Result<()> {
    const RELEASE: &str = "0.164 on Codex unreleased";
    const USER: &str = "café 界 e\u{301} \u{10eeee}\u{305}\u{305}";
    let cells: Vec<Arc<dyn HistoryCell>> = vec![
        card.clone(),
        Arc::new(new_user_prompt(USER.into(), vec![], vec![], vec![])),
        Arc::new(AgentMarkdownCell::new(
            "**afterward**".into(),
            Path::new("/"),
        )),
    ];
    let mut view = TranscriptView::default();
    view.primary_selection = true;
    let area = Rect::new(0, 0, 80, 24);
    let mut buffer = Buffer::empty(area);
    view.render(area, &mut buffer, &cells);
    let find = |word: &str| {
        area.positions().find(|p| {
            (p.x..area.right())
                .map(|x| buffer[(x, p.y)].symbol())
                .collect::<String>()
                .starts_with(word)
        })
    };
    let release = find(RELEASE).ok_or_eyre("release in owned transcript")?;
    let bold = find("afterward").ok_or_eyre("Markdown in owned transcript")?;
    let y = release.y;
    let tiles: Vec<_> = (0..area.width)
        .filter(|&x| buffer[(x, y)].symbol().starts_with('\u{10eeee}'))
        .collect();
    assert_eq!(
        tiles.len(),
        32,
        "the painted release row retains both native images"
    );
    let plain_row = (0..area.width)
        .map(|x| {
            let s = buffer[(x, y)].symbol();
            if s.starts_with('\u{10eeee}') { " " } else { s }
        })
        .collect::<String>();
    let model = card
        .as_any()
        .downcast_ref::<SessionInfoCell>()
        .and_then(SessionInfoCell::live_banner_model)
        .ok_or_eyre("live launch card")?;
    let update = |label: &str| {
        model
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .update(label.into())
    };
    let mut drag = |start: Position, end: Position, change_model: bool| -> color_eyre::Result<_> {
        let event = |kind, p: Position| MouseEvent {
            kind,
            column: p.x,
            row: p.y,
            modifiers: KeyModifiers::NONE,
        };
        view.handle_mouse(
            event(MouseEventKind::Down(MouseButton::Left), start),
            &cells,
        );
        view.handle_mouse(event(MouseEventKind::Drag(MouseButton::Left), end), &cells);
        let Some(ViewAction::PrimarySelection(primary)) =
            view.handle_mouse(event(MouseEventKind::Up(MouseButton::Left), end), &cells)
        else {
            panic!("publish selected text");
        };
        if change_model {
            // The new model shifts the card's layout. Command-C must still copy the held frame.
            update("different model since selection");
            view.render(area, &mut buffer, &cells);
        }
        let Some(ViewAction::Copy(plain)) = view.handle_key(
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::SUPER),
            &cells,
        ) else {
            panic!("Command-C must copy the selection");
        };
        assert_eq!(plain, primary);
        let mut result = None;
        view.copy_selected_text_with(&cells, &plain, true, |text, format| {
            result = Some((text.to_owned(), format));
            Ok(CopyStatus::Confirmed)
        })
        .map_err(color_eyre::eyre::Report::msg)?;
        result.ok_or_eyre("copied text")
    };
    let at = |x| Position::new(x, y);
    let (text, _) = drag(release, at(release.x + RELEASE.len() as u16), false)?;
    assert_eq!(text, RELEASE);
    for x in [tiles[0], tiles[10]] {
        let (text, _) = drag(at(x + 2), at(x + 5), false)?;
        assert_eq!(text, "   ");
    }
    let (text, format) = drag(at(0), at(79), false)?;
    assert_eq!(text, plain_row.trim_end());
    assert!(matches!(format, CopyFormat::PlainText));
    let (text, format) = drag(at(0), Position::new(bold.x + 9, bold.y), true)?;
    let CopyFormat::MarkdownSelection(markdown) = format else {
        panic!("mixed selection must retain Markdown");
    };
    for copied in [&text[..], &markdown[..]] {
        assert!(copied.contains(USER), "{copied:?}");
        assert_eq!(copied.matches('\u{10eeee}').count(), 1);
    }
    assert!(markdown.contains("**afterward**"));
    update("gpt-test");
    Ok(())
}
