// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Local caller custody only. Targets/choice IDs here are DATA without a
//! controller/delivery; no Native/SDK admission or UI success is fabricated.
use super::*;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

pub(super) fn fixture(target: &OwnedLocalTarget) -> Result<Arc<ConstructionRecord>> {
    fixture_with_presentation(target, ClientPresentation::Primary)
}
pub(super) fn fixture_with_presentation(
    target: &OwnedLocalTarget,
    presentation: ClientPresentation,
) -> Result<Arc<ConstructionRecord>> {
    fixture_for_original(target, presentation, None)
}
fn fixture_for_original(
    target: &OwnedLocalTarget,
    presentation: ClientPresentation,
    saved: Option<(uuid::Uuid, ThreadId)>,
) -> Result<Arc<ConstructionRecord>> {
    let selection = InitialSessionSelection {
        claim: "1".repeat(32),
        prospective_create: None,
    };
    let owner = InitialConstructionOperation::new(selection.clone(), Action::Start)?;
    target
        .0
        .initial_choice
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .current = Some(Chosen {
        action: Action::Start,
        selection,
        operation: owner.clone(),
    });
    let record = Arc::new(ConstructionRecord {
        target: Arc::downgrade(&target.0),
        delivery: std::sync::Weak::new(),
        // Pure lower ownership DATA. Side uses the real Source operation
        // constructor with explicit presentation; no Native grant/acceptance.
        owner: if presentation == ClientPresentation::Primary {
            ConstructionOwner::Initial(owner)
        } else {
            let selection = ClientConstructionSelection {
                prospective_create: ThreadStartParams::default(),
                presentation,
                prepared: None,
            };
            let operation = match saved {
                Some((opening, child)) => ClientConstructionOperation::for_saved_resume(
                    selection,
                    &opening.simple().to_string(),
                    &child.to_string(),
                )?,
                None => ClientConstructionOperation::new(selection)?,
            };
            ConstructionOwner::Later(operation)
        },
        presentation,
        output: Mutex::new(Output::Running),
        side_creation: Mutex::new(None),
        existing_original: Mutex::new(None),
        rejected_primary: Mutex::new(None),
    });
    ConstructionAdmission::reserve(&target.0)?.publish(record.clone())?;
    Ok(record)
}

#[test]
fn unpolled_original_future_records_unknown_without_polling_or_losing_owner() -> Result<()> {
    let target = OwnedLocalTarget::new("a".repeat(32), None, None);
    let record = fixture(&target)?;
    let polls = AtomicUsize::new(0);
    let observation = ConstructionObservation {
        actual: record.clone(),
        complete: false,
    };
    let future = crate::app_server_session::alab_construction::ConstructionScope::Retained(
        observation,
    )
    .run(async {
        polls.fetch_add(1, Ordering::SeqCst);
        Err(eyre!("unpolled original must remain unpolled"))
    });
    drop(future);
    assert_eq!(polls.load(Ordering::SeqCst), 0);
    assert!(matches!(
        *record.output.lock().unwrap_or_else(PoisonError::into_inner),
        Output::Unknown
    ));
    assert_eq!(
        target
            .0
            .initial_choice
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .constructions
            .len(),
        1
    );
    assert!(record.prepared_offer().is_none());
    Ok(())
}

#[tokio::test]
async fn repeated_original_preflight_errors_preserve_typed_cause_and_free_local_capacity()
-> Result<()> {
    let target = OwnedLocalTarget::new("a".repeat(32), None, None);
    for _ in 0..MAX_CONSTRUCTIONS + 3 {
        let record = fixture(&target)?;
        let observation = ConstructionObservation {
            actual: record,
            complete: false,
        };
        let error = observation
            .run(async {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "original selected-profile refusal",
                )
                .into())
            })
            .await
            .err()
            .ok_or_else(|| eyre!("original error disappeared"))?;
        assert_eq!(
            error.downcast_ref::<io::Error>().map(io::Error::kind),
            Some(io::ErrorKind::PermissionDenied)
        );
        let state = target
            .0
            .initial_choice
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        assert!(state.constructions.is_empty());
        assert!(state.current.is_none());
        assert!(state.admissions.is_empty());
    }
    Ok(())
}

#[test]
fn synchronous_admission_reservation_bounds_pending_owners_and_returns_cancelled_slots()
-> Result<()> {
    let target = OwnedLocalTarget::new("a".repeat(32), None, None);
    let mut admissions = Vec::new();
    for _ in 0..MAX_CONSTRUCTIONS {
        admissions.push(ConstructionAdmission::reserve(&target.0)?);
    }
    assert!(ConstructionAdmission::reserve(&target.0).is_err());
    drop(admissions.pop());
    let next = ConstructionAdmission::reserve(&target.0)?;
    drop(next);
    drop(admissions);
    assert!(
        target
            .0
            .initial_choice
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .admissions
            .is_empty()
    );
    Ok(())
}

#[test]
fn pending_local_record_does_not_keep_its_target_alive_in_a_cycle() -> Result<()> {
    let target = OwnedLocalTarget::new("a".repeat(32), None, None);
    let record = fixture(&target)?;
    let weak = Arc::downgrade(&target.0);
    drop(target);
    assert!(weak.upgrade().is_none());
    assert!(record.target.upgrade().is_none());
    Ok(())
}

#[test]
fn saved_resume_history_releases_live_capacity_and_retires_only_the_original_record() -> Result<()>
{
    let target = OwnedLocalTarget::new("a".repeat(32), None, None);
    let child = ThreadId::new();
    let saved = || {
        fixture_for_original(
            &target,
            ClientPresentation::SideConversation,
            Some((uuid::Uuid::new_v4(), child)),
        )
    };
    let old = saved()?;
    for _ in 1..MAX_CONSTRUCTIONS {
        fixture(&target)?;
    }
    assert!(ConstructionAdmission::reserve(&target.0).is_err());
    *old.output.lock().unwrap_or_else(PoisonError::into_inner) = Output::Failed;
    old.retain_saved_resume_history();
    // C1 history remains available to a handoff while an explicit same-child
    // C2 construction uses the newly available live slot.
    let newer = saved()?;
    let current = || {
        target
            .0
            .initial_choice
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .constructions
            .clone()
    };
    assert!(current().iter().any(|r| Arc::ptr_eq(r, &old)));
    assert!(ConstructionAdmission::reserve(&target.0).is_err());
    old.finish_saved_resume_history();
    old.finish_saved_resume_history();
    let remaining = current();
    assert_eq!(remaining.len(), MAX_CONSTRUCTIONS);
    assert!(!remaining.iter().any(|r| Arc::ptr_eq(r, &old)));
    assert!(remaining.iter().any(|r| Arc::ptr_eq(r, &newer)));
    Ok(())
}
