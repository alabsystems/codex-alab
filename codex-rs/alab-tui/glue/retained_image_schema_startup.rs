// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! The actual CLI reporter is linked to THIS mandatory-field whole-UI decoder.
//! Completing the pre-thread socket inventory advertises implementation support,
//! never receipt/claim/application, package admission or Native permission.
use super::*;
/// # Safety
/// Same first synchronous main exclusion as existing exec custody: no runtime
/// threads/callback may close or reuse inherited raw descriptors yet.
pub unsafe fn pin_before_threads() -> io::Result<()> {
    // Compile-time field/type coupling prevents this actual CLI hook from being
    // built with an older decoder that silently ignores image/Clipboard custody.
    let _required_field: fn(&Frame) -> &codex_alab_tui::upgrade_exec_custody::images::LocatorData =
        |frame| &frame.image_custody;
    let _required_clipboard: fn(&Frame) -> &crate::clipboard_copy::worker::alab_retained_clipboard::ClipboardData =
        |frame| &frame.clipboard;
    // SAFETY: the caller owns the original first-main descriptor exclusion.
    unsafe { codex_alab_tui::upgrade_exec_custody::images::pin_before_threads() }
}
