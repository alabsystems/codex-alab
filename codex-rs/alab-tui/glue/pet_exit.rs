// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Clear pet images at exit and when replacing their widget. Normal shutdown
//! runs this before terminal restoration, and `Drop` retries cleanup on an early
//! exit. Erasing text does not delete Kitty graphics, including the picker's image.

use crate::pets::PetImageRenderError;
use crate::tui::Tui;

impl Tui {
    pub(crate) fn alab_clear_pet_images_for_thread_switch(&mut self) -> std::io::Result<()> {
        if codex_alab_tui::interface::available() {
            self.alab_clear_pet_images_on_exit()
                .map_err(|error| match error {
                    PetImageRenderError::Terminal(error) => error,
                    PetImageRenderError::Asset(error) => std::io::Error::other(error),
                })?;
        }
        Ok(())
    }

    pub(crate) fn alab_clear_pet_images_on_exit(&mut self) -> Result<(), PetImageRenderError> {
        let preview = if codex_alab_tui::interface::available() {
            // A thread switch can already be inside a synchronized redraw.
            // Delete directly instead of ending that redraw with a nested update.
            match super::ensure_virtual_terminal_processing() {
                Ok(()) => crate::pets::render_pet_picker_preview_image(
                    self.terminal.backend_mut(),
                    &mut self.pet_picker_preview_image_state,
                    None,
                ),
                Err(err) => Err(PetImageRenderError::Terminal(err)),
            }
        } else {
            Ok(())
        };
        // Attempt both cleanups even when one terminal write fails.
        let ambient = self.clear_ambient_pet_image();
        preview.and(ambient)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pets::AmbientPetDraw;
    use crate::pets::ImageProtocol;
    use crate::pets::render_ambient_pet_image;
    use crate::pets::render_pet_picker_preview_image;
    use codex_alab_tui::interface;
    use std::path::Path;

    fn install_images(tui: &mut Tui, home: &Path, protocol: ImageProtocol) {
        let frame = home.join("frame.png");
        std::fs::write(&frame, b"png").unwrap();
        std::fs::write(home.join("frame_h75_v2.six"), b"sixel").unwrap();
        let request = || AmbientPetDraw {
            frame: frame.clone(),
            protocol,
            x: 2,
            y: 3,
            clear_top_y: 3,
            columns: 4,
            rows: 5,
            height_px: 75,
            sixel_dir: home.to_owned(),
        };
        let mut output = Vec::new();
        render_ambient_pet_image(
            &mut output,
            &mut tui.ambient_pet_image_state,
            Some(request()),
        )
        .unwrap();
        render_pet_picker_preview_image(
            &mut output,
            &mut tui.pet_picker_preview_image_state,
            Some(request()),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn exit_clears_both_pet_images_before_terminal_restoration() {
        let state = Box::leak(Box::new(interface::State::new()));
        let _scope = interface::scoped(state);
        let home = tempfile::tempdir().unwrap();
        let mut tui = crate::tui::test_support::make_test_tui().unwrap();

        for enabled in [true, false] {
            interface::set_enabled(enabled);
            for owned in [false, true] {
                tui.set_owned_screen(owned).unwrap();
                for protocol in [
                    ImageProtocol::Kitty,
                    ImageProtocol::KittyLocalFile,
                    ImageProtocol::Sixel,
                ] {
                    install_images(&mut tui, home.path(), protocol);
                    tui.alab_clear_pet_images_on_exit().unwrap();

                    // A second native clear emits nothing only if the shutdown
                    // path already deleted both placements and their state.
                    let mut remaining = Vec::new();
                    render_ambient_pet_image(
                        &mut remaining,
                        &mut tui.ambient_pet_image_state,
                        None,
                    )
                    .unwrap();
                    render_pet_picker_preview_image(
                        &mut remaining,
                        &mut tui.pet_picker_preview_image_state,
                        None,
                    )
                    .unwrap();
                    assert!(
                        remaining.is_empty(),
                        "shutdown left an image: enabled={enabled}, owned={owned}, protocol={protocol:?}"
                    );
                }
            }
        }
        tui.set_owned_screen(false).unwrap();
    }

    #[tokio::test]
    async fn inactive_exit_retains_native_preview_cleanup_behavior() {
        assert!(!interface::available());
        let home = tempfile::tempdir().unwrap();
        let mut tui = crate::tui::test_support::make_test_tui().unwrap();
        install_images(&mut tui, home.path(), ImageProtocol::Kitty);
        tui.alab_clear_pet_images_on_exit().unwrap();

        let mut ambient = Vec::new();
        render_ambient_pet_image(&mut ambient, &mut tui.ambient_pet_image_state, None).unwrap();
        assert!(ambient.is_empty());
        let mut preview = Vec::new();
        render_pet_picker_preview_image(
            &mut preview,
            &mut tui.pet_picker_preview_image_state,
            None,
        )
        .unwrap();
        assert!(
            String::from_utf8(preview)
                .unwrap()
                .contains("a=d,d=I,i=49375,")
        );
    }

    #[tokio::test]
    async fn thread_switch_clears_the_previous_widgets_pet_images() {
        let state = Box::leak(Box::new(interface::State::new()));
        let _scope = interface::scoped(state);
        let home = tempfile::tempdir().unwrap();
        let mut tui = crate::tui::test_support::make_test_tui().unwrap();

        for enabled in [true, false] {
            interface::set_enabled(enabled);
            for owned in [true, false] {
                tui.set_owned_screen(owned).unwrap();
                install_images(&mut tui, home.path(), ImageProtocol::Kitty);
                tui.clear_for_thread_switch().unwrap();

                // The replacement widget cannot clear the old preview: it has
                // no record that the previous widget displayed one.
                let mut remaining = Vec::new();
                render_ambient_pet_image(&mut remaining, &mut tui.ambient_pet_image_state, None)
                    .unwrap();
                render_pet_picker_preview_image(
                    &mut remaining,
                    &mut tui.pet_picker_preview_image_state,
                    None,
                )
                .unwrap();
                assert!(
                    remaining.is_empty(),
                    "thread switch left an image: enabled={enabled}, owned={owned}"
                );
            }
        }
        tui.set_owned_screen(false).unwrap();
    }
}
