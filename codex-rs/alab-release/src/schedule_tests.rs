// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::Updater;
use std::cell::Cell;

fn release(version: &str, source: char) -> Release {
    Release {
        schema: 2,
        repository: PROJECT.into(),
        source: source.to_string().repeat(40),
        version: version.into(),
        client_version: "0.170.0".into(),
        baseline: "0.169.1".into(),
        target: TARGET.into(),
        updater: Updater::Atpkg,
        inputs: vec![],
    }
}

fn pin(version: &str, build: u64) -> Pin {
    Pin {
        version: version.into(),
        build,
    }
}

fn qualified(_: &Path) -> Result<(Ready, Pin, u64)> {
    Ok((
        Ready {
            product: "product".into(),
            ripgrep: "ripgrep".into(),
        },
        pin("0.11.0", PREVIOUS),
        0,
    ))
}

const DAY: u64 = 20261119;
const PREVIOUS: u64 = 2026111701;
const NEXT: u64 = 2026111901;

#[test]
fn same_version_main_moves_retry_original_intent_and_build_and_publish_once() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let old = pin("0.11.0", PREVIOUS);
    let first = release("0.12.0", 'a');
    let newer = release("0.12.0", 'b');
    let calls = Cell::new(0);
    let run = |r: &Reservation, path: PathBuf| -> Result<()> {
        assert_eq!(Release::load(&path)?, first);
        assert_eq!(r.build, NEXT);
        calls.set(calls.get() + 1);
        if calls.get() == 1 {
            return Err("interrupted before signed readback".into());
        }
        Ok(())
    };
    assert!(dispatch(tmp.path(), &first, &old, DAY, qualified, run).is_err());
    dispatch(tmp.path(), &newer, &old, DAY + 1, qualified, run)?;
    let signed = pin(&first.version, NEXT);
    dispatch(tmp.path(), &newer, &signed, DAY + 1, qualified, |_, _| {
        panic!("duplicate publication")
    })?;
    assert_eq!(calls.get(), 2);
    Ok(())
}

#[test]
fn lost_ack_resumes_the_native_publication_journal_on_the_same_public_build() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let first = release("0.12.0", 'a');
    assert!(
        dispatch(
            tmp.path(),
            &first,
            &pin("0.11.0", PREVIOUS),
            DAY,
            qualified,
            |_, _| { Err("index was written and acknowledgement was lost".into()) }
        )
        .is_err()
    );
    dispatch(
        tmp.path(),
        &first,
        &pin("0.12.0", NEXT),
        DAY + 1,
        qualified,
        |r, _| {
            assert_eq!(r.build, NEXT);
            Ok(())
        },
    )?;
    Ok(())
}

#[test]
fn signed_version_and_monotonic_build_reservations_survive_channel_changes() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let old = pin("0.11.0", PREVIOUS);
    let a = release("0.12.0", 'a');
    let b = release("0.13.0", 'b');
    assert_eq!(
        reserve(tmp.path(), &a, &old, DAY, qualified)?
            .unwrap()
            .build,
        NEXT
    );
    assert_eq!(
        reserve(tmp.path(), &b, &old, DAY, qualified)?
            .unwrap()
            .build,
        NEXT + 1
    );
    assert!(reserve(tmp.path(), &a, &pin("0.12.0", NEXT + 1), DAY, qualified)?.is_none());
    assert!(reserve(tmp.path(), &a, &pin("0.13.0", NEXT + 2), DAY, qualified)?.is_none());
    assert!(
        reserve(
            tmp.path(),
            &release("0.14.0", 'c'),
            &pin("0.11.0", 2026111999),
            DAY,
            qualified
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn qualification_crossing_midnight_uses_the_date_at_final_allocation() -> Result<()> {
    let release = release("0.12.0", 'a');
    let old = pin("0.11.0", PREVIOUS);
    let tomorrow = 20261120;
    let first = tomorrow * 100 + 1;
    for (latest_build, manual_floor, expected) in [
        (PREVIOUS, 0, first),
        (first, 0, first + 1),
        (PREVIOUS, first, first + 1),
    ] {
        let dir = tempfile::tempdir()?;
        let today = Cell::new(DAY);
        let reconciled = Cell::new(false);
        let reservation = reserve_with_allocation(
            dir.path(),
            &release,
            &old,
            || {
                assert!(reconciled.get());
                Ok(today.get())
            },
            |intent| {
                today.set(tomorrow);
                Ok((qualified(intent)?.0, pin(&old.version, latest_build), 0))
            },
            |latest| {
                assert_eq!(latest.build, latest_build);
                reconciled.set(true);
                Ok(((), manual_floor))
            },
        )?
        .expect("a number is available after midnight");
        assert_eq!(reservation.build, expected);
    }
    Ok(())
}

#[test]
fn only_an_unoccupied_new_reservation_needs_a_working_clock() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let state = dir.path();
    let release = release("0.12.0", 'a');
    let old = pin("0.11.0", PREVIOUS);
    let signed = pin("0.12.0", NEXT);
    let no_clock = || -> Result<u64> { panic!("read the clock without a new allocation") };
    let no_qualify = |_: &Path| -> Result<(Ready, Pin, u64)> { panic!("unexpected qualification") };
    let no_reconcile = |_: &Pin| -> Result<((), u64)> { panic!("unexpected reconciliation") };

    let covered =
        reserve_with_allocation(state, &release, &signed, no_clock, no_qualify, no_reconcile)?;
    assert!(covered.is_none());
    let covered = reserve_with_allocation(
        state,
        &release,
        &old,
        no_clock,
        |intent| Ok((qualified(intent)?.0, signed.clone(), 0)),
        no_reconcile,
    )?;
    assert!(covered.is_none());

    let failed = reserve_with_allocation(
        state,
        &release,
        &old,
        || Err("clock unavailable".into()),
        qualified,
        |_| Ok(((), 0)),
    );
    assert!(failed.is_err());
    assert!(!state.join("reservation-0.12.0.json").exists());

    let recovered = reserve_with_allocation(state, &release, &old, no_clock, qualified, |_| {
        reserve(state, &release, &old, DAY, qualified)?.expect("fixture reservation");
        Ok(((), NEXT))
    })?;
    assert_eq!(recovered.expect("recovered reservation").build, NEXT);
    let existing =
        reserve_with_allocation(state, &release, &old, no_clock, no_qualify, no_reconcile)?;
    assert_eq!(existing.expect("existing reservation").build, NEXT);
    Ok(())
}

#[test]
fn only_one_scheduler_owns_a_release_state() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let lock = acquire(tmp.path())?;
    // A descriptor inherited before exec retains this open file description.
    let inherited = rustix::io::dup(&lock)?;
    assert!(acquire(tmp.path()).is_err());
    drop(lock);
    let next = acquire(tmp.path())?;
    drop(inherited);
    assert!(acquire(tmp.path()).is_err());
    drop(next);
    acquire(tmp.path())?;
    Ok(())
}

#[test]
fn release_registry_requires_a_policy_for_the_registered_owner() -> Result<()> {
    let (owner, _) = PROJECT.split_once('/').expect("qualified project name");
    let registered = format!("{owner}/publication");
    assert_eq!(
        registry_reference(&format!("{registered}\n"))?,
        format!("{registered}@main:registry/repos.toml")
    );
    for invalid in [
        String::new(),
        "other-owner/publication".to_owned(),
        format!("{owner}/other"),
        format!("{registered}@other:registry/repos.toml"),
        format!("{registered}\nextra"),
    ] {
        assert!(registry_reference(&invalid).is_err(), "{invalid}");
    }
    Ok(())
}

#[test]
fn requires_live_open_delegation_and_current_signed_stable_platform() -> Result<()> {
    let dev = format!("git@github.com:{PROJECT}.git");
    let registry = format!(
        "[repos.codex-alab]\ndev='{dev}'\nrelease='https://github.com/alabsystems/codex-alab.git'\nreleased_by='codex-release'\natpkg={{recipe='package',program='codex'}}\n"
    );
    registry_standing(&registry)?;
    for changed in [
        registry.replace("codex-release", "other"),
        registry.replace("alabsystems/codex-alab", "other/codex-alab"),
        registry.replace(&dev, "git@github.com:other/codex-alab.git"),
        format!("{registry}public_release=false"),
        format!("{registry}[lifecycle.codex-alab]\nstate='on-hold'"),
        registry.replace("package", "none"),
    ] {
        assert!(registry_standing(&changed).is_err());
    }
    let report = "atpkg-publish status: codex-alab | 0.11.0 | UNKNOWN | aarch64-apple-darwin | Public release metadata; payload not checked\natpkg-publish status:   program codex; channel stable; build 2026111701; index 165\n";
    assert_eq!(parse_pin(report)?, pin("0.11.0", PREVIOUS));
    for changed in [
        report.replace(TARGET, "other"),
        report.replace("stable", "beta"),
        report.replace("Public release metadata; payload not checked", "YANKED"),
        format!("atpkg-publish status: LAPSED index\n{report}"),
        format!("{report}{report}"),
    ] {
        assert!(parse_pin(&changed).is_err());
    }
    Ok(())
}

#[test]
fn failed_compilation_leaves_the_version_open_for_a_fixed_descendant() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let old = pin("0.11.0", PREVIOUS);
    let broken = release("0.12.0", 'a');
    let fixed = release("0.12.0", 'b');
    assert!(
        dispatch(
            dir.path(),
            &broken,
            &old,
            DAY,
            |_| Err("protected product compile failed".into()),
            |_, _| panic!("unqualified source reached publisher")
        )
        .is_err()
    );
    assert!(!dir.path().join("reservation-0.12.0.json").exists());
    dispatch(
        dir.path(),
        &fixed,
        &old,
        DAY,
        |intent| {
            assert_eq!(Release::load(intent)?, fixed);
            qualified(intent)
        },
        |record, _| {
            assert_eq!(record.release.source, fixed.source);
            Ok(())
        },
    )?;
    dispatch(
        dir.path(),
        &release("0.12.0", 'c'),
        &pin("0.12.0", NEXT),
        DAY + 1,
        |_| panic!("published version requalified"),
        |_, _| panic!("published version replaced"),
    )?;
    Ok(())
}

#[test]
fn peer_advance_during_compilation_uses_new_build_floor_or_preserves_published_version()
-> Result<()> {
    let day = 20261119;
    let dir = tempfile::tempdir()?;
    let first = release("0.12.0", 'a');
    let old = pin("0.10.0", PREVIOUS - 1);
    let reserved = reserve(dir.path(), &first, &old, day, |path| {
        let (ready, _, _) = qualified(path)?;
        Ok((ready, pin("0.11.0", NEXT + 3), 0))
    })?
    .unwrap();
    assert_eq!(reserved.build, NEXT + 4);
    let second = tempfile::tempdir()?;
    assert!(
        reserve(second.path(), &first, &old, day, |path| {
            let (ready, _, _) = qualified(path)?;
            Ok((ready, pin("0.12.0", NEXT), 0))
        })?
        .is_none()
    );
    assert!(!second.path().join("reservation-0.12.0.json").exists());
    Ok(())
}

#[test]
fn native_manual_owner_keeps_its_lease_and_qualified_frozen_source() -> Result<()> {
    let f = crate::workflow::tests::fixture()?;
    let journal = f.state.join("regular-schedule");
    private_directory(&journal)?;
    let previous = pin("0.1.0", PREVIOUS);
    let used = NEXT + 5;
    let campaign =
        crate::workflow::Campaign::open(&f.repo, &f.release, &f.state, used, &f.publisher)?;
    let recover = || {
        existing::recover(
            &f.repo,
            &f.publisher,
            &f.state,
            &journal,
            &f.release,
            &previous,
        )
    };
    assert!(recover().is_err());
    let dir = f.state.join(f.release.id()?);
    let product = dir.join("build-test.json");
    let rg = dir.join("ripgrep-test.json");
    std::fs::write(&product, b"qualified product witness")?;
    std::fs::write(&rg, b"native ripgrep witness")?;
    campaign.retain_stage("product", &product)?;
    campaign.retain_stage("ripgrep", &rg)?;
    drop(campaign);
    assert_eq!(recover()?, used);
    let reserved = reserve(&journal, &f.release, &previous, DAY, |_| {
        panic!("requalified live native campaign")
    })?
    .unwrap();
    assert_eq!(reserved.build, used);
    assert_eq!(reserved.release, f.release);
    assert_eq!(reserved.ready.product, product.canonicalize()?);
    Ok(())
}

fn later_source(f: &crate::workflow::tests::Fixture) -> Result<Release> {
    crate::git(
        &f.repo,
        &[
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "later release source",
        ],
    )?;
    let source = crate::git(&f.repo, &["rev-parse", "HEAD"])?;
    let later = Release::from_commit(&f.repo, source.trim(), &f.release.target)?;
    assert_eq!(later.version, f.release.version);
    assert_ne!(later.id()?, f.release.id()?);
    Ok(later)
}

#[test]
fn a_manual_campaign_opened_during_qualification_keeps_its_number_and_source() -> Result<()> {
    use crate::workflow::Campaign;

    let f = crate::workflow::tests::fixture()?;
    let scheduled = later_source(&f)?;
    let state = f.state.canonicalize()?;
    let journal = state.join("regular-schedule");
    private_directory(&journal)?;
    let old = pin("0.5.0", PREVIOUS);
    let before = existing::recover(&f.repo, &f.publisher, &state, &journal, &scheduled, &old)?;
    assert_eq!(before, PREVIOUS);
    let reconcile = |latest: &Pin| {
        existing::reconcile(&f.repo, &f.publisher, &state, &journal, &scheduled, latest)
    };
    let ready = Ready {
        product: observed_stage(&state, &scheduled, "build")?,
        ripgrep: observed_stage(&state, &scheduled, "ripgrep")?,
    };
    let manual = Ready {
        product: observed_stage(&state, &f.release, "build")?,
        ripgrep: observed_stage(&state, &f.release, "ripgrep")?,
    };
    let mut owner = None;
    let first = dispatch_with_allocation(
        &journal,
        &scheduled,
        &old,
        || Ok(DAY),
        |intent| {
            assert_eq!(Release::load(intent)?, scheduled);
            let campaign = Campaign::open(&f.repo, &f.release, &state, NEXT, &f.publisher)?;
            campaign.retain_stage("product", &manual.product)?;
            campaign.retain_stage("ripgrep", &manual.ripgrep)?;
            owner = Some(campaign);
            Ok((ready.clone(), old.clone(), before))
        },
        reconcile,
        |_, _| panic!("scheduled source reached the publisher while the manual owner was live"),
    );
    assert!(owner.is_some());
    assert!(first.is_err());
    assert!(!journal.join("reservation-0.6.0.json").exists());
    drop(owner);

    let calls = Cell::new(0);
    dispatch_with_allocation(
        &journal,
        &scheduled,
        &old,
        || Ok(DAY),
        |_| Ok((ready, old.clone(), before)),
        reconcile,
        |reservation, intent| {
            assert_eq!(reservation.build, NEXT);
            assert_eq!(Release::load(&intent)?, f.release);
            assert_eq!(reservation.ready.product, manual.product);
            assert_eq!(reservation.ready.ripgrep, manual.ripgrep);
            calls.set(calls.get() + 1);
            Ok(())
        },
    )?;
    assert_eq!(calls.get(), 1);
    Ok(())
}

#[test]
fn a_scheduled_reservation_excludes_a_different_manual_source_after_restart() -> Result<()> {
    use crate::workflow::Campaign;

    let f = crate::workflow::tests::fixture()?;
    let competing = later_source(&f)?;
    let state = f.state.canonicalize()?;
    let journal = state.join("regular-schedule");
    private_directory(&journal)?;
    let old = pin("0.5.0", PREVIOUS);
    let first = dispatch_with_allocation(
        &journal,
        &f.release,
        &old,
        || Ok(DAY),
        |_| {
            let ready = Ready {
                product: observed_stage(&state, &f.release, "build")?,
                ripgrep: observed_stage(&state, &f.release, "ripgrep")?,
            };
            Ok((ready, old.clone(), old.build))
        },
        |latest| existing::reconcile(&f.repo, &f.publisher, &state, &journal, &f.release, latest),
        |_, _| Err("fixture publisher stopped before signing".into()),
    );
    assert!(first.is_err());
    let record: Reservation = serde_json::from_slice(&crate::record_bytes(
        &journal.join("reservation-0.6.0.json"),
    )?)?;
    assert_eq!(record.release, f.release);
    assert_eq!(record.build, NEXT);
    let attempt = state
        .join(competing.id()?)
        .join(format!("publication-{NEXT}"));
    assert!(Campaign::open(&f.repo, &competing, &state, NEXT, &f.publisher).is_err());
    assert!(!attempt.exists());
    let original = || Campaign::open(&f.repo, &f.release, &state, NEXT, &f.publisher);
    let owner = original()?;
    assert!(original().is_err());
    drop(owner);
    original()?.check()?;
    assert!(Campaign::open(&f.repo, &competing, &state, NEXT, &f.publisher).is_err());
    assert!(!attempt.exists());
    Ok(())
}

#[test]
fn recovery_counts_another_manual_number_without_replacing_the_reserved_version() -> Result<()> {
    use crate::workflow::Campaign;

    let f = crate::workflow::tests::fixture()?;
    let manual_source = later_source(&f)?;
    let state = f.state.canonicalize()?;
    let journal = state.join("regular-schedule");
    private_directory(&journal)?;
    let old = pin("0.5.0", PREVIOUS);
    let original = Ready {
        product: observed_stage(&state, &f.release, "build")?,
        ripgrep: observed_stage(&state, &f.release, "ripgrep")?,
    };
    let reserved = reserve_with_allocation(
        &journal,
        &f.release,
        &old,
        || Ok(DAY),
        |_| Ok((original.clone(), old.clone(), 0)),
        |latest| existing::reconcile(&f.repo, &f.publisher, &state, &journal, &f.release, latest),
    )?
    .expect("qualified source should reserve a number");
    assert_eq!(reserved.build, NEXT);
    let record = journal.join(format!("reservation-{}.json", f.release.version));
    let saved = crate::record_bytes(&record)?;

    let product = observed_stage(&state, &manual_source, "build")?;
    let ripgrep = observed_stage(&state, &manual_source, "ripgrep")?;
    let complete_manual = |number| -> Result<()> {
        let owner = Campaign::open(&f.repo, &manual_source, &state, number, &f.publisher)?;
        owner.retain_stage("product", &product)?;
        owner.retain_stage("ripgrep", &ripgrep)
    };
    complete_manual(NEXT + 1)?;
    let recover = || {
        existing::recover(
            &f.repo,
            &f.publisher,
            &state,
            &journal,
            &manual_source,
            &old,
        )
    };
    assert_eq!(recover()?, NEXT + 1);
    assert_eq!(crate::record_bytes(&record)?, saved);
    let resumed = reserve(&journal, &manual_source, &old, DAY, |_| {
        panic!("requalified the reserved version")
    })?
    .expect("original reservation must survive recovery");
    assert_eq!(resumed.release, f.release);
    assert_eq!(resumed.build, NEXT);
    assert_eq!(resumed.ready.product, original.product);
    assert_eq!(resumed.ready.ripgrep, original.ripgrep);
    assert!(complete_manual(NEXT).is_err());

    // Older controllers could write both origins independently. Recreate those
    // exact two records using the real manual producer and restore the reservation.
    let backup = journal.join("fixture-saved-reservation.json");
    std::fs::rename(&record, &backup)?;
    complete_manual(NEXT)?;
    std::fs::rename(&backup, &record)?;
    assert!(recover().is_err());
    assert_eq!(crate::record_bytes(&record)?, saved);
    Ok(())
}

#[test]
fn two_completed_manual_candidates_leave_an_unreserved_version_open() -> Result<()> {
    use crate::workflow::Campaign;

    for reverse in [false, true] {
        let f = crate::workflow::tests::fixture()?;
        let later = later_source(&f)?;
        let state = f.state.canonicalize()?;
        let journal = state.join("regular-schedule");
        private_directory(&journal)?;
        let mut candidates = [(&f.release, NEXT), (&later, NEXT + 1)];
        if reverse {
            candidates.reverse();
        }
        let mut original = Vec::new();
        for (source, number) in candidates {
            let product = observed_stage(&state, source, "build")?;
            let ripgrep = observed_stage(&state, source, "ripgrep")?;
            let owner = Campaign::open(&f.repo, source, &state, number, &f.publisher)?;
            owner.retain_stage("product", &product)?;
            owner.retain_stage("ripgrep", &ripgrep)?;
            original.push((source, number, product, ripgrep));
        }
        let old = pin("0.5.0", PREVIOUS);
        let reservation = journal.join(format!("reservation-{}.json", later.version));
        assert!(!reservation.exists());
        assert!(existing::recover(&f.repo, &f.publisher, &state, &journal, &later, &old).is_err());
        assert!(!reservation.exists());
        for (source, number, product, ripgrep) in original {
            let owner = Campaign::open(&f.repo, source, &state, number, &f.publisher)?;
            assert_eq!(owner.stage("product")?, Some(product));
            assert_eq!(owner.stage("ripgrep")?, Some(ripgrep));
        }
    }
    Ok(())
}

#[test]
fn an_unnumbered_schedule_respects_a_legacy_publication_directory_without_an_intent() -> Result<()>
{
    let f = crate::workflow::tests::fixture()?;
    let later = later_source(&f)?;
    let state = f.state.canonicalize()?;
    let checkout = crate::source::Checkout::prepare(&f.repo, &f.release, &state)?;
    let prior = checkout.campaign.join(format!("publication-{NEXT}"));
    DirBuilder::new().mode(0o700).create(&prior)?;
    drop(checkout);

    let journal = state.join("regular-schedule");
    assert!(!journal.exists());
    assert!(crate::workflow::Campaign::open(&f.repo, &later, &state, NEXT, &f.publisher).is_err());
    assert!(!prior.join("intent.json").exists());
    assert!(
        !state
            .join(later.id()?)
            .join(format!("publication-{NEXT}"))
            .exists()
    );

    private_directory(&journal)?;
    let old = pin("0.5.0", PREVIOUS);
    let dispatched = Cell::new(false);
    dispatch_with_allocation(
        &journal,
        &later,
        &old,
        || Ok(DAY),
        |intent| Ok((qualified(intent)?.0, old.clone(), old.build)),
        |latest| existing::reconcile(&f.repo, &f.publisher, &state, &journal, &later, latest),
        |reservation, intent| {
            assert_eq!(reservation.build, NEXT + 1);
            assert_eq!(Release::load(&intent)?, later);
            dispatched.set(true);
            Ok(())
        },
    )?;
    assert!(dispatched.get());
    assert!(!prior.join("intent.json").exists());
    Ok(())
}

#[test]
fn the_allocator_excludes_manual_creation_between_its_scan_and_durable_reservation() -> Result<()> {
    let f = crate::workflow::tests::fixture()?;
    let state = f.state.canonicalize()?;
    let journal = state.join("regular-schedule");
    private_directory(&journal)?;
    let old = pin("0.5.0", PREVIOUS);
    let (guard, highest) =
        existing::reconcile(&f.repo, &f.publisher, &state, &journal, &f.release, &old)?;
    assert_eq!(highest, old.build);
    let open = || crate::workflow::Campaign::open(&f.repo, &f.release, &state, NEXT, &f.publisher);
    assert!(open().is_err());
    let attempt = state
        .join(f.release.id()?)
        .join(format!("publication-{NEXT}"));
    assert!(!attempt.exists());
    drop(guard);
    open()?.check()?;
    assert!(attempt.join("intent.json").is_file());
    Ok(())
}

#[test]
fn changed_prequalified_observation_is_refused_before_it_enters_the_publisher() -> Result<()> {
    use sha2::Digest;
    let tmp = tempfile::tempdir()?;
    let bytes = b"private exact observation";
    let digest = format!("{:x}", sha2::Sha256::digest(bytes));
    let ready = Ready {
        product: tmp.path().join(format!("build-{digest}.json")),
        ripgrep: tmp.path().join(format!("ripgrep-{digest}.json")),
    };
    std::fs::write(&ready.product, bytes)?;
    std::fs::write(&ready.ripgrep, bytes)?;
    ready.check()?;
    std::fs::write(&ready.product, b"different observation")?;
    assert!(ready.check().is_err());
    Ok(())
}

fn checkpoint_fixture() -> Result<(tempfile::TempDir, PathBuf, PathBuf)> {
    let tmp = tempfile::tempdir()?;
    let state = tmp.path().canonicalize()?;
    private_directory(&state.join("regular-schedule"))?;
    let publisher = state.join("publisher");
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o700)
        .open(&publisher)?;
    std::fs::write(&publisher, b"#!/bin/sh\nexit 0\n")?;
    Ok((tmp, state, publisher))
}

fn observed_stage(state: &Path, release: &Release, stage: &str) -> Result<PathBuf> {
    observed_bytes(
        state,
        release,
        stage,
        format!("{stage} observation for {}", release.source).as_bytes(),
    )
}

fn observed_bytes(state: &Path, release: &Release, stage: &str, bytes: &[u8]) -> Result<PathBuf> {
    let campaign = state.join(release.id()?);
    private_directory(&campaign)?;
    let id = format!("{:x}", Sha256::digest(bytes));
    let record = campaign.join(format!("{stage}-{id}.json"));
    crate::save_record(&record, bytes)?;
    Ok(record)
}

#[test]
fn ripgrep_stop_retries_only_ripgrep_before_reserving_and_publishing_once() -> Result<()> {
    let (_tmp, state, publisher) = checkpoint_fixture()?;
    let journal = state.join("regular-schedule");
    let wanted = release("0.12.0", 'a');
    let old = pin("0.11.0", PREVIOUS);
    let (builds, ripgreps, published) = (Cell::new(0), Cell::new(0), Cell::new(0));
    let qualify = |intent: &Path| {
        let actual = Release::load(intent)?;
        let ready = qualify_stages(
            &state,
            &actual,
            &publisher,
            || {
                builds.set(builds.get() + 1);
                observed_stage(&state, &actual, "build")
            },
            |product| {
                assert_eq!(product.parent(), Some(state.join(actual.id()?).as_path()));
                ripgreps.set(ripgreps.get() + 1);
                if ripgreps.get() == 1 {
                    return Err("ripgrep disk refusal".into());
                }
                observed_stage(&state, &actual, "ripgrep")
            },
            |ready| ready.check(),
            || Ok(()),
        )?;
        Ok((ready, old.clone(), 0))
    };
    let publish = |record: &Reservation, _: PathBuf| {
        assert_eq!(record.release, wanted);
        assert_eq!(record.build, NEXT);
        record.ready.check()?;
        published.set(published.get() + 1);
        Ok(())
    };
    assert!(dispatch(&journal, &wanted, &old, DAY, qualify, publish).is_err());
    assert!(!journal.join("reservation-0.12.0.json").exists());
    dispatch(&journal, &wanted, &old, DAY, qualify, publish)?;
    dispatch(
        &journal,
        &wanted,
        &pin("0.12.0", NEXT),
        DAY,
        |_| panic!("published source requalified"),
        publish,
    )?;
    assert_eq!((builds.get(), ripgreps.get(), published.get()), (1, 2, 1));
    Ok(())
}

#[test]
fn a_new_source_qualifies_its_own_product_after_the_previous_ripgrep_stops() -> Result<()> {
    let (_tmp, state, publisher) = checkpoint_fixture()?;
    let journal = state.join("regular-schedule");
    let old = pin("0.11.0", PREVIOUS);
    let previous = release("0.12.0", 'a');
    let current = release("0.12.0", 'b');
    assert!(
        reserve(&journal, &previous, &old, DAY, |_| {
            checkpoint_product(&state, &previous, &publisher, || {
                observed_stage(&state, &previous, "build")
            })?;
            Err("ripgrep disk refusal".into())
        })
        .is_err()
    );
    let builds = Cell::new(0);
    let reservation = reserve(&journal, &current, &old, DAY, |_| {
        let product = checkpoint_product(&state, &current, &publisher, || {
            builds.set(builds.get() + 1);
            observed_stage(&state, &current, "build")
        })?;
        let ripgrep = observed_stage(&state, &current, "ripgrep")?;
        Ok((Ready { product, ripgrep }, old.clone(), 0))
    })?
    .unwrap();
    assert_eq!(builds.get(), 1);
    assert_eq!(reservation.release, current);
    assert_eq!(
        reservation.ready.product.parent(),
        Some(state.join(current.id()?).as_path())
    );
    Ok(())
}

#[test]
fn damaged_product_checkpoints_refuse_before_the_compiler_runs() -> Result<()> {
    for damage in [
        "observation",
        "publisher",
        "linked pointer",
        "linked observation",
        "invalid JSON",
    ] {
        let (_tmp, state, publisher) = checkpoint_fixture()?;
        let wanted = release("0.12.0", 'a');
        let record = checkpoint_product(&state, &wanted, &publisher, || {
            observed_stage(&state, &wanted, "build")
        })?;
        let pointer = state
            .join("regular-schedule")
            .join(format!("product-{}.json", wanted.id()?));
        match damage {
            "observation" => std::fs::write(record, b"changed build")?,
            "publisher" => std::fs::write(&publisher, b"replacement publisher")?,
            "invalid JSON" => std::fs::write(pointer, b"{")?,
            _ => {
                let path = if damage == "linked pointer" {
                    pointer
                } else {
                    record
                };
                let real = state.join("original");
                std::fs::rename(&path, &real)?;
                std::os::unix::fs::symlink(real, path)?;
            }
        }
        assert!(
            checkpoint_product(&state, &wanted, &publisher, || {
                panic!("compiler ran with {damage}")
            })
            .is_err(),
            "{damage}"
        );
    }
    Ok(())
}

#[test]
fn failed_product_cannot_be_reused_even_if_it_left_an_observation() -> Result<()> {
    let (_tmp, state, publisher) = checkpoint_fixture()?;
    let wanted = release("0.12.0", 'a');
    assert!(
        checkpoint_product(&state, &wanted, &publisher, || {
            observed_stage(&state, &wanted, "build")?;
            Err("product stopped before completion".into())
        })
        .is_err()
    );
    let builds = Cell::new(0);
    checkpoint_product(&state, &wanted, &publisher, || {
        builds.set(builds.get() + 1);
        observed_stage(&state, &wanted, "build")
    })?;
    assert_eq!(builds.get(), 1);
    Ok(())
}

fn check_retained_binary(ready: &Ready, binary: &Path) -> Result<()> {
    ready.check()?;
    if crate::product::file_hash(binary)? != std::fs::read_to_string(&ready.ripgrep)? {
        return Err("retained executable changed".into());
    }
    Ok(())
}

#[test]
fn a_failed_pin_read_after_qualification_reuses_both_stages_and_publishes_once() -> Result<()> {
    let (_tmp, state, publisher) = checkpoint_fixture()?;
    let wanted = release("0.12.0", 'a');
    let journal = state.join("regular-schedule");
    let reservation = journal.join("reservation-0.12.0.json");
    let old = pin("0.11.0", PREVIOUS);
    let binary = state.join("rg");
    let (products, ripgreps, checked, pin_reads, published) = (
        Cell::new(0),
        Cell::new(0),
        Cell::new(0),
        Cell::new(0),
        Cell::new(0),
    );
    let qualify = |intent: &Path| {
        let release = Release::load(intent)?;
        let ready = qualify_stages(
            &state,
            &release,
            &publisher,
            || {
                products.set(products.get() + 1);
                observed_stage(&state, &release, "build")
            },
            |_| {
                ripgreps.set(ripgreps.get() + 1);
                std::fs::write(&binary, b"qualified rg")?;
                observed_bytes(
                    &state,
                    &release,
                    "ripgrep",
                    crate::product::file_hash(&binary)?.as_bytes(),
                )
            },
            |pair| {
                assert!(!reservation.exists());
                check_retained_binary(pair, &binary)?;
                checked.set(checked.get() + 1);
                Ok(())
            },
            || Ok(()),
        )?;
        pin_reads.set(pin_reads.get() + 1);
        if pin_reads.get() == 1 {
            return Err("signed channel temporarily unreadable".into());
        }
        Ok((ready, old.clone(), 0))
    };
    let publish = |record: &Reservation, _: PathBuf| {
        assert_eq!(record.build, NEXT);
        assert_eq!(record.release, wanted);
        published.set(published.get() + 1);
        Ok(())
    };
    assert!(dispatch(&journal, &wanted, &old, DAY, qualify, publish).is_err());
    assert!(!reservation.exists());
    dispatch(&journal, &wanted, &old, DAY, qualify, publish)?;
    dispatch(
        &journal,
        &wanted,
        &pin("0.12.0", NEXT),
        DAY,
        |_| panic!("already published"),
        publish,
    )?;
    assert_eq!(
        (
            products.get(),
            ripgreps.get(),
            pin_reads.get(),
            published.get()
        ),
        (1, 1, 2, 1)
    );
    assert_eq!(checked.get(), 1);
    Ok(())
}

#[test]
fn damaged_completed_qualification_refuses_before_reservation_or_compilation() -> Result<()> {
    for damage in [
        "pointer",
        "missing product",
        "different product",
        "observation",
        "binary",
        "publisher",
    ] {
        let (_tmp, state, publisher) = checkpoint_fixture()?;
        let wanted = release("0.12.0", 'a');
        let journal = state.join("regular-schedule");
        let binary = state.join("rg");
        std::fs::write(&binary, b"certified fixture")?;
        let pair = qualify_stages(
            &state,
            &wanted,
            &publisher,
            || observed_stage(&state, &wanted, "build"),
            |_| {
                observed_bytes(
                    &state,
                    &wanted,
                    "ripgrep",
                    crate::product::file_hash(&binary)?.as_bytes(),
                )
            },
            |ready| check_retained_binary(ready, &binary),
            || Ok(()),
        )?;
        let marker = journal.join(format!("ready-{}.json", wanted.id()?));
        match damage {
            "pointer" => std::fs::write(&marker, b"{")?,
            "missing product" => {
                std::fs::remove_file(journal.join(format!("product-{}.json", wanted.id()?)))?
            }
            "different product" => {
                let mut saved: ReadyCheckpoint = serde_json::from_slice(&std::fs::read(&marker)?)?;
                saved.product_sha256 = "0".repeat(64);
                std::fs::write(&marker, serde_json::to_vec(&saved)?)?;
            }
            "observation" => std::fs::write(&pair.ripgrep, b"changed metadata")?,
            "publisher" => std::fs::write(&publisher, b"replacement publisher")?,
            _ => std::fs::write(&binary, b"replaced retained executable")?,
        }
        let checked = Cell::new(false);
        let old = pin("0.11.0", PREVIOUS);
        let result = dispatch(
            &journal,
            &wanted,
            &old,
            DAY,
            |_| {
                let ready = qualify_stages(
                    &state,
                    &wanted,
                    &publisher,
                    || panic!("product recompiled after {damage} changed"),
                    |_| panic!("ripgrep recompiled after {damage} changed"),
                    |ready| {
                        checked.set(true);
                        check_retained_binary(ready, &binary)
                    },
                    || Ok(()),
                )?;
                Ok((ready, old.clone(), 0))
            },
            |_, _| panic!("damaged {damage} reached publication"),
        );
        assert!(result.is_err(), "{damage}");
        assert!(!journal.join("reservation-0.12.0.json").exists());
        if damage == "binary" {
            assert!(checked.get());
        }
    }
    Ok(())
}
