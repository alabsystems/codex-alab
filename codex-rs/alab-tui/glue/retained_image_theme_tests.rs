// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Installed native theme observation only; no renderer/Entered is fabricated.
use super::*;
#[test]
fn same_actual_theme_rebinds_only_its_own_old_revision_without_overwriting_new_state()
-> io::Result<()> {
    let original = current_syntax_theme(); // actual original test installation.
    set_syntax_theme(original.clone());
    let captured = ThemeStamp::capture()?;
    set_syntax_theme(original.clone());
    let newer = syntax_theme_revision();
    assert_ne!(captured.revision_data(), newer);
    assert_eq!(captured.match_installed(captured.revision_data())?, newer);
    assert!(
        captured
            .match_installed(captured.revision_data().wrapping_add(1))
            .is_err()
    );
    let mut changed = original.clone();
    changed.name = Some("different genuine installed test theme".into());
    set_syntax_theme(changed);
    let refused = captured.match_installed(captured.revision_data()).is_err();
    set_syntax_theme(original);
    assert!(refused);
    Ok(())
}
#[test]
fn theme_observation_never_constructs_an_uninstalled_native_theme() -> io::Result<()> {
    // Tests use a per-thread native theme cell, so this dedicated fresh thread
    // has no actual installed theme and must observe None without initialising one.
    let worker = std::thread::spawn(|| {
        let captured = ThemeStamp::capture();
        let observed_absence = captured.as_ref().is_ok_and(|stamp| !stamp.has_theme());
        let absent = TEST_THEME.with(|actual| actual.get().is_none());
        (observed_absence, absent)
    });
    let (observed_absence, absent) = worker
        .join()
        .map_err(|_| io::Error::other("native theme observation worker panicked"))?;
    assert!(observed_absence && absent);
    Ok(())
}

#[test]
fn same_actual_writer_exclusion_preserves_a_newer_theme_and_its_generation() -> io::Result<()> {
    use std::sync::Arc;
    use std::sync::atomic::AtomicU64;
    use std::sync::atomic::Ordering;
    let original = current_syntax_theme();
    let actual = Arc::new(RwLock::new(original.clone()));
    let revision = Arc::new(AtomicU64::new(17));
    let mut newer = original.clone();
    newer.name = Some("newer actual local preview".into());
    let mut late = original;
    late.name = Some("older captured preview".into());
    let observed = revision.load(Ordering::Acquire);
    let mut held = actual
        .write()
        .map_err(|_| invalid("test native write interrupted"))?;
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let worker_actual = actual.clone();
    let worker_revision = revision.clone();
    let worker = std::thread::spawn(move || {
        let _sent = started_tx.send(());
        conditional_swap(
            &worker_actual,
            observed,
            late,
            || worker_revision.load(Ordering::Acquire),
            || {
                worker_revision
                    .fetch_add(1, Ordering::AcqRel)
                    .wrapping_add(1)
            },
        )
    });
    let started = started_rx
        .recv_timeout(std::time::Duration::from_secs(60))
        .map_err(invalid);
    *held = newer.clone();
    revision.fetch_add(1, Ordering::AcqRel);
    drop(held);
    let result = worker
        .join()
        .map_err(|_| invalid("native conditional theme worker panicked"))?;
    started?;
    assert!(result.is_err());
    assert_eq!(
        *actual
            .read()
            .map_err(|_| invalid("test native read interrupted"))?,
        newer
    );
    assert_eq!(revision.load(Ordering::Acquire), 18);
    Ok(())
}

#[test]
fn captured_runtime_theme_initializes_only_actual_absence_without_default_lookup() -> io::Result<()>
{
    let original = current_syntax_theme();
    let worker = std::thread::spawn(move || -> io::Result<()> {
        let absent = ThemeStamp::capture()?;
        assert!(!absent.has_theme());
        let revision = conditional_install(&absent, original.clone())?;
        let restored = ThemeStamp::capture()?;
        assert!(restored.has_theme());
        assert_eq!(restored.revision_data(), revision);
        assert_eq!(
            *TEST_THEME
                .with(|actual| actual.get().cloned())
                .ok_or_else(|| invalid("actual theme not installed"))?
                .read()
                .map_err(invalid)?,
            original
        );
        Ok(())
    });
    worker
        .join()
        .map_err(|_| invalid("native theme initial installation worker panicked"))??;
    Ok(())
}
#[test]
fn original_concurrent_initializer_wins_without_being_replaced_by_captured_data() -> io::Result<()>
{
    let captured = current_syntax_theme();
    let worker = std::thread::spawn(move || -> io::Result<()> {
        let absent = ThemeStamp::capture()?;
        assert!(!absent.has_theme());
        let mut newer = captured.clone();
        newer.name = Some("genuine new local initialization".into());
        set_syntax_theme(newer.clone());
        let revision = syntax_theme_revision();
        assert!(conditional_install(&absent, captured).is_err());
        assert_eq!(current_syntax_theme(), newer);
        assert_eq!(syntax_theme_revision(), revision);
        Ok(())
    });
    worker
        .join()
        .map_err(|_| invalid("native theme concurrent initialization worker panicked"))??;
    Ok(())
}
