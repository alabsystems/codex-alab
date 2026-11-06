// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn sample(time: i64, used: f64) -> WeeklySnapshot {
    WeeklySnapshot {
        used_percent: used,
        observed_at: time,
        resets_at: 604_800,
        window_minutes: 10_080,
    }
}

#[test]
fn forecasts_use_elapsed_time_and_expire_without_a_fresh_reading() {
    let mut history = History::default();
    assert!(history.observe(sample(0, 0.0)));
    assert!(history.observe(sample(600, 5.0)));
    assert!(history.view(600).unwrap().projection.is_none());
    assert!(history.observe(sample(3600, 10.0)));
    let view = history.view(3600).unwrap();
    let estimate = view.projection.as_ref().unwrap();
    assert!((estimate.percent_per_day - 240.0).abs() < 0.00001);
    assert_eq!(estimate.exhausted_at, Some(36_000));
    assert!(view.at(5401).projection.is_none());
    assert!(view.at(604_800).projection.is_none());
    assert!(view.at(3599).projection.is_none());
    let mut flat = History::default();
    for time in [0, 600, 3600] {
        flat.observe(sample(time, 10.0));
    }
    let flat_estimate = flat.view(3600).unwrap().projection.unwrap();
    assert_eq!(flat_estimate.projected_percent_at_reset, 10.0);
    assert_eq!(flat_estimate.exhausted_at, None);
}

#[test]
fn resets_decreases_and_stale_responses_never_mix_consumption_cycles() {
    let mut history = History::default();
    for (time, used) in [(100, 10.0), (1100, 15.0), (2100, 20.0)] {
        assert!(history.observe(sample(time, used)));
    }
    assert!(!history.observe(sample(1100, 99.0)));
    let current = history.view(2100).unwrap();
    assert_eq!(current.sample_count, 3);
    assert!(current.projection.is_some());
    assert_eq!(
        Weekly::for_observation(sample(2100, 20.0), Some(current.clone())),
        Some(current.clone()),
        "the accepted reading keeps its matching observed history"
    );
    for accepted in [
        WeeklySnapshot {
            resets_at: 604_700,
            ..sample(2100, 20.0)
        },
        WeeklySnapshot {
            window_minutes: 10_081,
            ..sample(2100, 20.0)
        },
        sample(2100, 19.0),
        sample(2200, 20.0),
    ] {
        assert!(accepted.valid());
        let authoritative = Weekly::for_observation(accepted, Some(current.clone())).unwrap();
        assert_eq!(authoritative.samples, vec![accepted]);
        assert_eq!(authoritative.sample_count, 1);
        assert!(authoritative.projection.is_none());
    }
    assert!(history.observe(sample(2200, 5.0)));
    assert_eq!(history.view(2200).unwrap().sample_count, 1);
    let next = WeeklySnapshot {
        resets_at: 1_209_600,
        ..sample(604_801, 2.0)
    };
    assert!(history.observe(next));
    let view = history.view(604_801).unwrap();
    assert_eq!(view.used_percent, 2.0);
    assert_eq!(view.sample_count, 1);
    assert!(view.projection.is_none());
    assert!(!history.observe(sample(604_799, 95.0)));
    assert_eq!(history.view(604_801).unwrap(), view);
}

#[test]
fn same_second_reads_persist_the_latest_usage_without_duplicate_forecast_anchors() {
    let home = tempfile::tempdir().unwrap();
    let store = Store::new(home.path());
    store.observe("account", sample(0, 10.0)).unwrap();
    let updated = store.observe("account", sample(0, 20.0)).unwrap();
    assert_eq!(updated.used_percent, 20.0);
    assert_eq!(updated.sample_count, 1);
    store.observe("account", sample(900, 30.0)).unwrap();
    let updated = store.observe("account", sample(900, 35.0)).unwrap();
    assert_eq!(updated.used_percent, 35.0);
    assert_eq!(updated.sample_count, 2);
    assert!(updated.projection.is_none());
    store.observe("account", sample(1800, 40.0)).unwrap();
    let updated = store.observe("account", sample(1800, 45.0)).unwrap();
    assert_eq!(updated.used_percent, 45.0);
    assert_eq!(updated.sample_count, 3);
    assert!(
        (updated.projection.as_ref().unwrap().percent_per_day - 1200.0).abs() < 0.00001,
        "the forecast uses the corrected first and last readings"
    );
    assert_eq!(
        Store::new(home.path())
            .read("account", 1800)
            .unwrap()
            .unwrap(),
        updated,
        "the correction survives another tab or process reading the history"
    );
    assert_eq!(
        store.observe("account", sample(1800, 45.0)).unwrap(),
        updated
    );
    assert_eq!(
        store.observe("account", sample(1799, 90.0)).unwrap(),
        updated,
        "an older reading still cannot replace the latest accepted value"
    );
}

#[test]
fn accepted_reset_corrections_replace_the_cycle_even_within_the_same_second() {
    let mut history = History::default();
    for (time, used) in [(100, 10.0), (1100, 15.0), (2100, 20.0)] {
        assert!(history.observe(sample(time, used)));
    }
    let corrected = WeeklySnapshot {
        resets_at: 600_000,
        ..sample(2101, 21.0)
    };
    assert!(history.observe(corrected));
    let view = history.view(2101).unwrap();
    assert_eq!(view.resets_at, corrected.resets_at);
    assert_eq!(view.used_percent, 21.0);
    assert_eq!(view.sample_count, 1);
    assert!(view.projection.is_none());
    let redeemed = WeeklySnapshot {
        resets_at: 600_001,
        ..sample(2101, 0.0)
    };
    assert!(history.observe(redeemed));
    let view = history.view(2101).unwrap();
    assert_eq!(view.resets_at, redeemed.resets_at);
    assert_eq!(view.used_percent, 0.0);
    assert_eq!(view.sample_count, 1);
    assert!(view.projection.is_none());
    assert!(!history.observe(redeemed));
    assert!(!history.observe(sample(2100, 99.0)));
    assert_eq!(history.view(2101).unwrap(), view);
}

#[test]
fn graph_positions_are_time_buckets_with_unobserved_gaps() {
    let mut history = History::default();
    for (time, used) in [(0, 0.0), (900, 50.0), (3500, 100.0)] {
        assert!(history.observe(WeeklySnapshot {
            resets_at: 3600,
            window_minutes: 60,
            ..sample(time, used)
        }));
    }
    let view = history.view(3500).unwrap();
    assert_eq!(view.sparkline(4), "▁▅·█");
    assert_eq!(view.sparkline(0), "");
}

#[test]
fn history_survives_restart_and_stays_with_its_account() {
    let home = tempfile::tempdir().unwrap();
    let store = Store::new(home.path());
    store.observe("account-a", sample(0, 10.0)).unwrap();
    store.observe("account-b", sample(100, 80.0)).unwrap();
    let reopened = Store::new(home.path());
    let a = reopened.observe("account-a", sample(1800, 20.0)).unwrap();
    assert_eq!(a.sample_count, 2);
    assert_eq!(
        reopened
            .read("account-b", 1800)
            .unwrap()
            .unwrap()
            .used_percent,
        80.0
    );
    assert!(reopened.read("unknown-account", 1800).unwrap().is_none());
    assert!(
        !store
            .path("person@example.com")
            .to_string_lossy()
            .contains("person")
    );
    #[cfg(unix)]
    {
        let mode = fs::metadata(store.path("account-a"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    // Even a file copied under a different fingerprint is never read as that account.
    fs::copy(store.path("account-a"), store.path("account-c")).unwrap();
    assert_eq!(
        store.read("account-c", 1800).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
    fs::write(store.path("account-d"), b"interrupted or invalid history").unwrap();
    let repaired = store.observe("account-d", sample(1800, 30.0)).unwrap();
    assert_eq!(repaired.sample_count, 1);
    assert_eq!(repaired.used_percent, 30.0);

    fs::create_dir(store.path("directory-account")).unwrap();
    assert!(store.read("directory-account", 1800).is_err());
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;

        let fifo =
            std::ffi::CString::new(store.path("fifo-account").as_os_str().as_bytes()).unwrap();
        // SAFETY: this owned TempDir path is NUL-terminated and names no existing file.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        assert_eq!(
            store.read("fifo-account", 1800).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            store
                .observe("fifo-account", sample(1800, 30.0))
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }
    let unchanged = store.read("account-a", 1800).unwrap().unwrap();
    assert_eq!(unchanged.sample_count, 2);
    assert_eq!(unchanged.used_percent, 20.0);

    let destination = store.path("account-a");
    let temporary = store.directory.join("occupied-candidate.tmp");
    let sentinel = b"an existing temporary candidate";
    fs::write(&temporary, sentinel).unwrap();
    let before = fs::read(&destination).unwrap();
    let mut replacement = store.load("account-a").unwrap();
    assert!(replacement.observe(sample(3600, 25.0)));
    assert_eq!(
        Store::save_to_temporary(&destination, &temporary, "account-a", &replacement)
            .unwrap_err()
            .kind(),
        io::ErrorKind::AlreadyExists
    );
    assert_eq!(fs::read(&temporary).unwrap(), sentinel);
    assert_eq!(fs::read(&destination).unwrap(), before);
    fs::remove_file(&temporary).unwrap();
    Store::save_to_temporary(&destination, &temporary, "account-a", &replacement).unwrap();
    assert!(!temporary.exists());
    let replaced = store.read("account-a", 3600).unwrap().unwrap();
    assert_eq!(replaced.sample_count, 3);
    assert_eq!(replaced.used_percent, 25.0);
}

#[test]
fn samples_are_bounded_and_fast_updates_keep_the_first_anchor() {
    let mut history = History::default();
    for time in 0..300 {
        history.observe(sample(time, time as f64 / 100.0));
    }
    assert_eq!(history.samples.len(), 2);
    assert_eq!(history.samples[0].observed_at, 0);
    assert_eq!(history.samples[1].observed_at, 299);
    for time in (300..604_800).step_by(300) {
        history.observe(sample(time, 3.0));
    }
    assert!(history.samples.len() <= MAX_SAMPLES);
    assert!(history.valid());
    assert!(!history.observe(sample(604_800, f64::NAN)));
    for time in (604_800..2_678_400).step_by(300) {
        history.observe(WeeklySnapshot {
            resets_at: 2_678_400,
            window_minutes: 31 * 24 * 60,
            ..sample(time, 3.0)
        });
    }
    assert_eq!(history.samples.len(), MAX_SAMPLES);
    assert!(history.valid());
}

#[test]
fn retained_history_flushes_without_replaying_acknowledged_cross_tab_readings() {
    let home = tempfile::tempdir().unwrap();
    let store = Store::new(home.path());
    let mut accepted = History::default();
    store
        .observe_with_history("account", sample(0, 10.0), &mut accepted)
        .unwrap();
    store.observe("account", sample(0, 20.0)).unwrap();
    let merged = store
        .observe_with_history("account", sample(1800, 30.0), &mut accepted)
        .unwrap();
    assert_eq!(merged.samples, vec![sample(0, 20.0), sample(1800, 30.0)]);

    // An independently newer durable observation cannot be replaced by a stale
    // accepted read, nor lend its forecast to that read's in-memory display.
    let newer = store.observe("account", sample(3600, 40.0)).unwrap();
    let durable = store
        .observe_with_history("account", sample(2700, 35.0), &mut accepted)
        .unwrap();
    assert_eq!(durable, newer);
    assert_eq!(store.read("account", 3600).unwrap(), Some(newer));
    assert_eq!(accepted.samples.last(), Some(&sample(2700, 35.0)));
    let flushed = store
        .observe_with_history("account", sample(4500, 50.0), &mut accepted)
        .unwrap();
    assert_eq!(
        flushed.samples,
        vec![
            sample(0, 20.0),
            sample(1800, 30.0),
            sample(2700, 35.0),
            sample(3600, 40.0),
            sample(4500, 50.0)
        ]
    );
    let corrected = store
        .observe_with_history("account", sample(4500, 49.0), &mut accepted)
        .unwrap();
    assert_eq!(corrected.samples, vec![sample(4500, 49.0)]);
}

#[test]
fn a_fresh_memory_same_second_decrease_invalidates_durable_anchors() {
    let home = tempfile::tempdir().unwrap();
    let store = Store::new(home.path());
    for (time, used) in [(0, 10.0), (900, 15.0), (1800, 30.0)] {
        store.observe("account", sample(time, used)).unwrap();
    }
    let corrected = store.observe("account", sample(1800, 20.0)).unwrap();
    assert_eq!(corrected.samples, vec![sample(1800, 20.0)]);
    assert!(corrected.projection.is_none());
    assert_eq!(store.read("account", 1800).unwrap(), Some(corrected));
}

#[test]
fn retained_history_flushes_an_unwritten_same_second_correction() {
    let home = tempfile::tempdir().unwrap();
    let store = Store::new(home.path());
    let mut accepted = History::default();
    store
        .observe_with_history("account", sample(0, 10.0), &mut accepted)
        .unwrap();
    let guard = Lock::acquire(&store.path("account").with_extension("lock")).unwrap();
    assert!(
        store
            .observe_with_history("account", sample(0, 20.0), &mut accepted)
            .is_err()
    );
    drop(guard);
    let recovered = store
        .observe_with_history("account", sample(1800, 30.0), &mut accepted)
        .unwrap();
    assert_eq!(recovered.samples, vec![sample(0, 20.0), sample(1800, 30.0)]);
    assert_eq!(store.read("account", 1800).unwrap(), Some(recovered));
}

#[test]
fn retained_history_does_not_guess_the_order_of_competing_pending_corrections() {
    let home = tempfile::tempdir().unwrap();
    let store = Store::new(home.path());
    let mut accepted = History::default();
    store
        .observe_with_history("account", sample(0, 10.0), &mut accepted)
        .unwrap();
    let guard = Lock::acquire(&store.path("account").with_extension("lock")).unwrap();
    assert!(
        store
            .observe_with_history("account", sample(0, 20.0), &mut accepted)
            .is_err()
    );
    drop(guard);
    store.observe("account", sample(0, 30.0)).unwrap();
    let recovered = store
        .observe_with_history("account", sample(1800, 40.0), &mut accepted)
        .unwrap();
    assert_eq!(recovered.samples, vec![sample(1800, 40.0)]);
    assert!(recovered.projection.is_none());
    assert_eq!(store.read("account", 1800).unwrap(), Some(recovered));
}

#[test]
fn retained_history_cannot_restore_anchors_before_an_unwritten_segment_change() {
    let home = tempfile::tempdir().unwrap();
    let store = Store::new(home.path());
    for change in ["decrease", "reset-away-and-back"] {
        let mut accepted = History::default();
        store
            .observe_with_history(change, sample(0, 10.0), &mut accepted)
            .unwrap();
        let guard = Lock::acquire(&store.path(change).with_extension("lock")).unwrap();
        let changed = match change {
            "decrease" => sample(900, 50.0),
            _ => WeeklySnapshot {
                resets_at: 604_700,
                ..sample(900, 15.0)
            },
        };
        for observation in [changed, sample(1800, 20.0)] {
            assert!(
                store
                    .observe_with_history(change, observation, &mut accepted)
                    .is_err()
            );
        }
        drop(guard);
        let recovered = store
            .observe_with_history(change, sample(3600, 25.0), &mut accepted)
            .unwrap();
        assert_eq!(
            recovered.samples,
            vec![sample(1800, 20.0), sample(3600, 25.0)],
            "{change}: the old durable anchor belongs to an invalidated segment"
        );
        assert!(recovered.projection.is_none());
        assert_eq!(
            store.read(change, 3600).unwrap(),
            Some(recovered),
            "the segment boundary survives a new process reading the cache"
        );
    }
}

#[test]
fn another_tabs_lock_does_not_block_the_caller_or_lose_the_stored_reading() {
    let home = tempfile::tempdir().unwrap();
    let store = Store::new(home.path());
    store.observe("account", sample(0, 10.0)).unwrap();
    let guard = Lock::acquire(&store.path("account").with_extension("lock")).unwrap();
    assert_eq!(
        store
            .observe("account", sample(1800, 20.0))
            .unwrap_err()
            .kind(),
        io::ErrorKind::WouldBlock
    );
    drop(guard);
    let view = store.observe("account", sample(1800, 20.0)).unwrap();
    assert_eq!(view.sample_count, 2);
}

#[test]
fn a_retimed_first_read_hydrates_fresh_memory_without_acknowledging_its_unsaved_tail() {
    let home = tempfile::tempdir().unwrap();
    let store = Store::new(home.path());
    for (time, used) in [(0, 10.0), (900, 20.0), (1800, 30.0)] {
        store.observe("account", sample(time, used)).unwrap();
    }
    let path = store.path("account");
    let written = fs::read(&path).unwrap();
    let reopened = Store::new(home.path());
    let mut accepted = History::default();
    let observation = sample(1830, 30.0);
    let merged = reopened
        .observe_with_history("account", observation, &mut accepted)
        .unwrap();
    assert_eq!(
        merged.samples,
        vec![sample(0, 10.0), sample(900, 20.0), observation],
        "the real store loads the durable anchors and retimes only the latest reading"
    );
    assert!(merged.projection.is_some());
    assert_eq!(fs::read(&path).unwrap(), written, "retiming writes nothing");
    let displayed = Weekly::for_observation(observation, accepted.view(1830)).unwrap();

    let guard = Lock::acquire(&path.with_extension("lock")).unwrap();
    assert_eq!(
        reopened
            .observe_with_history("account", sample(2100, 35.0), &mut accepted)
            .unwrap_err()
            .kind(),
        io::ErrorKind::WouldBlock
    );
    assert_eq!(fs::read(&path).unwrap(), written);
    drop(guard);
    let flushed = reopened
        .observe_with_history("account", sample(2400, 40.0), &mut accepted)
        .unwrap();
    let mut expected = vec![
        sample(0, 10.0),
        sample(900, 20.0),
        observation,
        sample(2100, 35.0),
        sample(2400, 40.0),
    ];
    assert_eq!(
        flushed.samples, expected,
        "the retimed tail stays pending through contention until a later save"
    );
    assert_eq!(
        Store::new(home.path()).read("account", 2400).unwrap(),
        Some(flushed)
    );

    Store::new(home.path())
        .observe("account", sample(2400, 41.0))
        .unwrap();
    expected.pop();
    expected.extend([sample(2400, 41.0), sample(2700, 45.0)]);
    let combined = reopened
        .observe_with_history("account", sample(2700, 45.0), &mut accepted)
        .unwrap();
    assert_eq!(
        combined.samples, expected,
        "acknowledged history cannot overwrite another tab's later correction"
    );
    assert_eq!(
        Store::new(home.path()).read("account", 2700).unwrap(),
        Some(combined)
    );

    assert_eq!(
        displayed, merged,
        "the first footer view after reopening must retain the loaded graph and forecast"
    );
}

/// The usage poll repeats one reading every 5 to 60 s. A read that only moves the latest
/// point's time within its five-minute bucket writes nothing (each write was a full rewrite and
/// an fsync), the tab still shows it, and the next bucket's write saves it. NEGATIVE CONTROL:
/// without `only_retimed` the file changes at 930.
#[test]
fn a_poll_that_only_retimes_the_latest_reading_writes_nothing() {
    let home = tempfile::tempdir().unwrap();
    let store = Store::new(home.path());
    let mut accepted = History::default();
    for time in [0, 900] {
        store
            .observe_with_history("account", sample(time, 10.0), &mut accepted)
            .unwrap();
    }
    let path = store.path("account");
    let written = fs::read(&path).unwrap();
    let shown = store
        .observe_with_history("account", sample(930, 10.0), &mut accepted)
        .unwrap();
    assert_eq!(shown.observed_at, 930);
    assert_eq!(fs::read(&path).unwrap(), written);

    store
        .observe_with_history("account", sample(1200, 10.0), &mut accepted)
        .unwrap();
    assert_eq!(
        store
            .read("account", 1200)
            .unwrap()
            .map(|view| view.samples),
        Some(vec![sample(0, 10.0), sample(930, 10.0), sample(1200, 10.0)])
    );
    store
        .observe_with_history("account", sample(1230, 11.0), &mut accepted)
        .unwrap();
    assert_eq!(
        store
            .read("account", 1230)
            .unwrap()
            .map(|view| view.used_percent),
        Some(11.0),
        "a changed reading is written within its bucket"
    );
}
