// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual local Tui/image/file custody at the private transaction seam only.
//! No Binding, Entered frontend, Native permission or whole-scene ACK is made.
use super::*;
use image::Rgba;
use image::RgbaImage;
use std::cell::Cell;
use std::path::PathBuf;

struct ResultDropWitness<'a> {
    _actual: Arc<OriginalFile>,
    original: PathBuf,
    saw_original: &'a Cell<bool>,
}
impl Drop for ResultDropWitness<'_> {
    fn drop(&mut self) {
        self.saw_original.set(self.original.exists());
    }
}

fn drawn() -> anyhow::Result<crate::tui::Tui> {
    let image = Arc::new(TranscriptImage {
        protocol: crate::pets::ImageProtocol::KittyLocalFile,
        columns: 1,
        rows: 1,
        pixels: RgbaImage::from_pixel(2, 2, Rgba([100, 20, 3, 255])),
        cell_width: 8,
        cell_height: 16,
    });
    let mut tui = crate::tui::test_support::make_test_tui()?;
    tui.set_owned_screen(true)?;
    tui.draw(24, |frame| {
        frame.image_placements.push(ImagePlacement {
            image,
            area: Rect::new(2, 2, 1, 1),
            first_row: 0,
            file: None,
        })
    })?;
    Ok(tui)
}
fn prepared() -> anyhow::Result<(Prepared, PathBuf)> {
    let mut source = drawn()?;
    let original = source.retained_images[0]
        .file
        .as_ref()
        .ok_or_else(|| io::Error::other("actual source file absent"))?
        .actual
        .clone();
    let path = original.with_native(|file| Ok(file.path().to_path_buf()))?;
    let mut graph = native::Capture::default();
    let captured = Book::capture(&source, &mut graph)?;
    let shared = native::Restored::new(graph.export()?)?;
    let prepared = captured
        .data
        .clone()
        .prepare_data(&shared, |_| Ok(original.clone()))?;
    drop(captured);
    drop(original);
    source.draw(24, |_| {})?;
    source.set_owned_screen(false)?;
    assert!(path.exists()); // Prepared is now the actual file owner.
    Ok((prepared, path))
}
fn lower<T, F>(
    owner: &mut Installation<T>,
    tui: &mut crate::tui::Tui,
    install: &mut Option<F>,
    invalidated: &Cell<bool>,
    handoff: &mut dyn FnMut(&[Arc<OriginalFile>]) -> io::Result<()>,
) -> InstallState
where
    F: FnOnce(&mut crate::tui::Tui, &[ImagePlacement]) -> io::Result<T>,
{
    let mut authorize = |_: &crate::tui::Tui| Ok(None);
    let mut recheck = |_: &crate::tui::Tui| Ok(());
    let mut invalidate = || invalidated.set(true);
    owner.install_bound(
        tui,
        install,
        &mut InstallChecks {
            authorize: &mut authorize,
            handoff,
            recheck: &mut recheck,
            invalidate: &mut invalidate,
        },
    )
}
fn local_file() -> anyhow::Result<(Arc<OriginalFile>, PathBuf)> {
    let file = OriginalFile::retain_native(tempfile::NamedTempFile::new()?);
    let path = file.with_native(|file| Ok(file.path().to_path_buf()))?;
    Ok((file, path))
}

#[tokio::test]
async fn preflight_refusal_keeps_fn_once_capture_and_can_retry_before_call() -> anyhow::Result<()> {
    let (prepared, path) = prepared()?;
    let mut owner = prepared.begin_install();
    let mut occupied = drawn()?;
    let (callback_owner, callback_path) = local_file()?;
    let calls = Cell::new(0);
    let mut install = Some(|tui: &mut crate::tui::Tui, expected: &[ImagePlacement]| {
        calls.set(calls.get() + 1);
        tui.retained_images = retain_placements(expected);
        Ok(callback_owner)
    });
    let invalidated = Cell::new(false);
    let mut handoff = |_: &[Arc<OriginalFile>]| Ok(());
    assert_eq!(
        lower(
            &mut owner,
            &mut occupied,
            &mut install,
            &invalidated,
            &mut handoff
        ),
        InstallState::Refused
    );
    assert_eq!(calls.get(), 0);
    assert!(install.is_some());
    assert!(!invalidated.get());
    assert!(path.exists() && callback_path.exists());
    let mut owner = match owner.into_outcome() {
        InstallOutcome::Refused(refused) => {
            assert!(refused.error().is_some());
            refused.retry()
        }
        _ => return Err(io::Error::other("preflight did not return Refused").into()),
    };
    occupied.draw(24, |_| {})?;
    occupied.set_owned_screen(false)?;
    let mut target = crate::tui::test_support::make_test_tui()?;
    assert_eq!(
        lower(
            &mut owner,
            &mut target,
            &mut install,
            &invalidated,
            &mut handoff
        ),
        InstallState::Installed
    );
    assert_eq!(calls.get(), 1);
    assert!(install.is_none());
    assert!(!invalidated.get());
    let installed = match owner.into_outcome() {
        InstallOutcome::Installed(installed) => installed,
        _ => return Err(io::Error::other("retry did not install").into()),
    };
    assert!(installed.result().is_some());
    target.retained_images.clear();
    drop(target);
    assert!(path.exists() && callback_path.exists());
    drop(installed);
    assert!(!path.exists() && !callback_path.exists());
    Ok(())
}

#[tokio::test]
async fn installer_error_keeps_prepared_and_changed_current_files_without_retry()
-> anyhow::Result<()> {
    let (prepared, path) = prepared()?;
    let (changed, changed_path) = local_file()?;
    let calls = Cell::new(0);
    let mut install = Some(
        |tui: &mut crate::tui::Tui, expected: &[ImagePlacement]| -> io::Result<()> {
            calls.set(calls.get() + 1);
            tui.retained_images = retain_placements(expected);
            tui.retained_images[0].file = Some(ImageFile::from_actual(changed)?);
            Err(io::Error::from_raw_os_error(5))
        },
    );
    let mut owner = prepared.begin_install();
    let mut target = crate::tui::test_support::make_test_tui()?;
    let invalidated = Cell::new(false);
    let mut handoff = |_: &[Arc<OriginalFile>]| Ok(());
    assert_eq!(
        lower(
            &mut owner,
            &mut target,
            &mut install,
            &invalidated,
            &mut handoff
        ),
        InstallState::Unknown
    );
    assert_eq!(
        lower(
            &mut owner,
            &mut target,
            &mut install,
            &invalidated,
            &mut handoff
        ),
        InstallState::Unknown
    );
    assert_eq!(calls.get(), 1);
    assert!(invalidated.get());
    let unknown = match owner.into_outcome() {
        InstallOutcome::Unknown(unknown) => unknown,
        _ => return Err(io::Error::other("attempt did not remain Unknown").into()),
    };
    assert_eq!(unknown.error().and_then(io::Error::raw_os_error), Some(5));
    assert!(unknown.result().is_none());
    target.retained_images.clear();
    drop(target);
    assert!(path.exists() && changed_path.exists());
    drop(unknown);
    assert!(!path.exists() && !changed_path.exists());
    Ok(())
}

#[tokio::test]
async fn alias_postcondition_keeps_successful_result_and_original_prepared_owner()
-> anyhow::Result<()> {
    let (prepared, path) = prepared()?;
    let (result, result_path) = local_file()?;
    let result_drop_saw_original = Cell::new(false);
    let result = ResultDropWitness {
        _actual: result,
        original: path.clone(),
        saw_original: &result_drop_saw_original,
    };
    let mut install = Some(|tui: &mut crate::tui::Tui, expected: &[ImagePlacement]| {
        tui.retained_images = retain_placements(expected);
        tui.retained_images[0].file = None; // PartialEq ignores this real alias loss.
        Ok(result)
    });
    let mut owner = prepared.begin_install();
    let mut target = crate::tui::test_support::make_test_tui()?;
    let invalidated = Cell::new(false);
    let handoff_calls = Cell::new(0);
    let mut handoff = |_: &[Arc<OriginalFile>]| {
        handoff_calls.set(1);
        Ok(())
    };
    assert_eq!(
        lower(
            &mut owner,
            &mut target,
            &mut install,
            &invalidated,
            &mut handoff
        ),
        InstallState::Unknown
    );
    assert!(invalidated.get());
    assert_eq!(handoff_calls.get(), 0);
    let unknown = match owner.into_outcome() {
        InstallOutcome::Unknown(unknown) => unknown,
        _ => return Err(io::Error::other("alias loss did not remain Unknown").into()),
    };
    assert!(unknown.error().is_some());
    assert!(unknown.result().is_some());
    target.retained_images.clear();
    drop(target);
    assert!(path.exists() && result_path.exists());
    drop(unknown);
    assert!(result_drop_saw_original.get());
    assert!(!path.exists() && !result_path.exists());
    Ok(())
}

#[tokio::test]
async fn late_handoff_failure_keeps_current_retired_and_successful_result() -> anyhow::Result<()> {
    let (prepared, path) = prepared()?;
    let (result, result_path) = local_file()?;
    let retired_path = std::cell::RefCell::new(None::<PathBuf>);
    let mut install = Some(|tui: &mut crate::tui::Tui, expected: &[ImagePlacement]| {
        // Publish an actual local transport file during the original callback;
        // then install expected SAME aliases. The transaction must retain the
        // callback's actual publication when replacing its book census.
        tui.set_owned_screen(true)?;
        let image = expected[0].image.clone();
        tui.draw(24, |frame| {
            frame.image_placements.push(ImagePlacement {
                image,
                area: Rect::new(2, 2, 1, 1),
                first_row: 0,
                file: None,
            })
        })?;
        *retired_path.borrow_mut() = Some(
            tui.retained_images[0]
                .file
                .as_ref()
                .ok_or_else(|| io::Error::other("actual callback file missing"))?
                .path()
                .to_path_buf(),
        );
        tui.retained_images = retain_placements(expected);
        Ok(result)
    });
    let mut owner = prepared.begin_install();
    let mut target = crate::tui::test_support::make_test_tui()?;
    let invalidated = Cell::new(false);
    let handoff_calls = Cell::new(0);
    let mut handoff = |_: &[Arc<OriginalFile>]| {
        handoff_calls.set(handoff_calls.get() + 1);
        Err(io::Error::from_raw_os_error(5))
    };
    assert_eq!(
        lower(
            &mut owner,
            &mut target,
            &mut install,
            &invalidated,
            &mut handoff
        ),
        InstallState::Unknown
    );
    assert_eq!(handoff_calls.get(), 1);
    assert!(invalidated.get());
    let unknown = match owner.into_outcome() {
        InstallOutcome::Unknown(unknown) => unknown,
        _ => return Err(io::Error::other("handoff failure did not remain Unknown").into()),
    };
    assert_eq!(unknown.0.retired_records.len(), 1);
    assert!(unknown.result().is_some());
    drop(install); // Release the caller closure borrow before moving its path witness.
    let retired_path = retired_path
        .into_inner()
        .ok_or_else(|| io::Error::other("actual retired path not observed"))?;
    target.set_owned_screen(false)?;
    drop(target);
    assert!(path.exists() && retired_path.exists() && result_path.exists());
    drop(unknown);
    assert!(!path.exists() && !retired_path.exists() && !result_path.exists());
    Ok(())
}

#[tokio::test]
async fn callback_unwind_leaves_stored_owner_unknown_and_invalidated() -> anyhow::Result<()> {
    let (prepared, path) = prepared()?;
    let mut owner = prepared.begin_install::<()>();
    let mut target = crate::tui::test_support::make_test_tui()?;
    let invalidated = Cell::new(false);
    let mut install = Some(
        |tui: &mut crate::tui::Tui, expected: &[ImagePlacement]| -> io::Result<()> {
            tui.retained_images = retain_placements(expected);
            panic!("controlled original callback unwind");
        },
    );
    let mut handoff = |_: &[Arc<OriginalFile>]| Ok(());
    let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        lower(
            &mut owner,
            &mut target,
            &mut install,
            &invalidated,
            &mut handoff,
        )
    }));
    assert!(unwind.is_err());
    assert_eq!(owner.state(), InstallState::Unknown);
    assert!(invalidated.get());
    assert!(install.is_none());
    let unknown = match owner.into_outcome() {
        InstallOutcome::Unknown(unknown) => unknown,
        _ => return Err(io::Error::other("unwound callback lost Unknown owner").into()),
    };
    assert!(unknown.error().is_none()); // No invented normal IO outcome.
    target.retained_images.clear();
    drop(target);
    assert!(path.exists());
    drop(unknown);
    assert!(!path.exists());
    Ok(())
}

#[derive(Debug)]
struct ErrorDropWitness {
    original: PathBuf,
    saw_original: Arc<std::sync::atomic::AtomicBool>,
}
impl std::fmt::Display for ErrorDropWitness {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("controlled original installer error")
    }
}
impl std::error::Error for ErrorDropWitness {}
impl Drop for ErrorDropWitness {
    fn drop(&mut self) {
        self.saw_original
            .store(self.original.exists(), std::sync::atomic::Ordering::Release);
    }
}

#[tokio::test]
async fn returned_error_drops_before_its_original_prepared_file_owner() -> anyhow::Result<()> {
    let (prepared, path) = prepared()?;
    let saw_original = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let error = ErrorDropWitness {
        original: path.clone(),
        saw_original: saw_original.clone(),
    };
    // No target mutation or extra Arc pins the original file: Prepared alone
    // must outlive destruction of the actual io::Error-owned failure context.
    let mut install = Some(
        |_: &mut crate::tui::Tui, _: &[ImagePlacement]| -> io::Result<()> {
            Err(io::Error::other(error))
        },
    );
    let mut owner = prepared.begin_install();
    let mut target = crate::tui::test_support::make_test_tui()?;
    let invalidated = Cell::new(false);
    let mut handoff = |_: &[Arc<OriginalFile>]| Ok(());
    assert_eq!(
        lower(
            &mut owner,
            &mut target,
            &mut install,
            &invalidated,
            &mut handoff
        ),
        InstallState::Unknown
    );
    assert!(invalidated.get());
    assert!(install.is_none());
    let unknown = match owner.into_outcome() {
        InstallOutcome::Unknown(unknown) => unknown,
        _ => return Err(io::Error::other("original installer error lost Unknown custody").into()),
    };
    assert!(unknown.error().is_some());
    assert!(unknown.result().is_none());
    drop(target);
    assert!(path.exists());
    drop(unknown);
    assert!(saw_original.load(std::sync::atomic::Ordering::Acquire));
    assert!(!path.exists());
    Ok(())
}
