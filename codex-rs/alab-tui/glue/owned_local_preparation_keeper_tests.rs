// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Lower original caller ownership only. Actual canonical mapper + existing
//! operation-admission fixture; no Native origin/frontier/acceptance fabricated.
use super::super::super::tests as data;
use super::*;

async fn started(record: &Arc<ConstructionRecord>) -> Result<AppServerStartedThread> {
    let response = data::response(
        codex_protocol::ThreadId::new(),
        codex_protocol::ThreadId::new(),
    )?;
    let canonical = data::canonical(&response).await?;
    let mut started = data::original(&canonical)?;
    started.alab_construction = Some(record.clone());
    Ok(started)
}
#[derive(Debug)]
struct ExactCause(Arc<()>);
impl std::fmt::Display for ExactCause {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(out, "original preflight cause")
    }
}
impl std::error::Error for ExactCause {}
fn captured_parts(captured: &Arc<CapturedOriginal>) -> Result<PayloadLease<'_, OriginalParts>> {
    captured.original.take()
}

#[tokio::test]
async fn failed_primary_preparation_retains_exact_primary_and_cause_without_record_cycle()
-> Result<()> {
    let (target, record) = ConstructionRecord::preparation_fixture()?;
    let original = started(&record).await?;
    let expected = original.session.clone();
    let preparation = record
        .capture_existing_primary(original)
        .map_err(|failed| failed.error)?;
    let weak = Arc::downgrade(&preparation.captured);
    let cause = Arc::new(());
    let returned = preparation.fail(ExactCause(cause.clone()).into());
    assert!(
        returned
            .downcast_ref::<ExactCause>()
            .is_some_and(|actual| Arc::ptr_eq(&actual.0, &cause))
    );
    let captured = weak
        .upgrade()
        .ok_or_else(|| eyre!("failed original was dropped"))?;
    {
        let mut lease = captured_parts(&captured)?;
        let parts = lease.get_mut()?;
        assert_eq!(parts.started.session, expected);
        assert!(parts.started.alab_construction.is_none());
        assert!(parts.side.is_none());
    }
    let weak_record = Arc::downgrade(&record);
    drop(record);
    drop(target);
    assert!(
        weak_record.upgrade().is_none(),
        "held failed payload must not keep its Record in a cycle"
    );
    assert!(captured.original.take().is_ok());
    Ok(())
}

#[tokio::test]
async fn dropped_side_preparation_keeps_all_original_outer_fields() -> Result<()> {
    let (target, record) = ConstructionRecord::side_preparation_fixture()?;
    let thread = started(&record).await?;
    let expected = thread.session.clone();
    let original = SidebarFork {
        thread,
        boundary: Ok(()),
        inherited_profile: true,
        storage: crate::app_server_session::SideChatStorage::Temporary,
        legacy: true,
        alab_retained_replay: None,
    };
    let preparation = record
        .capture_existing_side(original)
        .map_err(|failed| failed.error)?;
    let weak = Arc::downgrade(&preparation.captured);
    drop(preparation); // Same path as an unpolled caller/failed early App check.
    let captured = weak
        .upgrade()
        .ok_or_else(|| eyre!("side original was dropped"))?;
    {
        let mut lease = captured_parts(&captured)?;
        let parts = lease.get_mut()?;
        assert_eq!(parts.started.session, expected);
        assert!(parts.started.alab_construction.is_none());
        let side = parts
            .side
            .as_ref()
            .ok_or_else(|| eyre!("side outer fields lost"))?;
        assert!(side.boundary.is_ok());
        assert!(side.inherited_profile);
        assert!(side.legacy);
        assert_eq!(
            side.storage,
            crate::app_server_session::SideChatStorage::Temporary
        );
    }
    drop(record);
    drop(target);
    assert!(captured.record.upgrade().is_none());
    Ok(())
}

#[tokio::test]
async fn repeated_primary_admission_preserves_live_payload_and_keeps_refused_dto_without_backlink()
-> Result<()> {
    let (target, record) = ConstructionRecord::preparation_fixture()?;
    let original = started(&record).await?;
    record.converted_cache_for_test(original.clone()); // Lower Source DTO-conversion premise, no Native receipt.
    let mut repeated = original.clone();
    repeated.session.thread_name = Some("refused exact incoming DTO".into());
    let expected = original.session.clone();
    let preparation = record
        .capture_existing_primary(original)
        .map_err(|failed| failed.error)?;
    let captured = preparation.captured.clone();
    let failed = match record.capture_existing_primary(repeated) {
        Ok(_) => return Err(eyre!("same operation admitted a second preparation")),
        Err(failed) => failed,
    };
    let returned = record.retain_rejected_primary(failed.original, failed.error);
    assert_eq!(
        returned.to_string(),
        "original operation already owns its preparation"
    );
    let refused = record
        .rejected_primary_for_test()
        .ok_or_else(|| eyre!("refused original was not retained"))?;
    assert_eq!(
        refused.session.thread_name.as_deref(),
        Some("refused exact incoming DTO")
    );
    assert!(refused.alab_construction.is_none());
    drop(refused);
    let accepted = record
        .original_started()
        .ok_or_else(|| eyre!("refused copy erased accepted conversion fact"))?;
    assert_eq!(
        accepted.session, expected,
        "same output prerequisite used by actual record.applied must remain Converted"
    );
    assert!(accepted.alab_construction.is_none());
    drop(accepted);
    {
        let mut lease = captured_parts(&captured)?;
        assert_eq!(
            lease.get_mut()?.started.session,
            expected,
            "live original cannot be overwritten"
        );
    }
    // The actual failure transition may set Unknown, while retaining refused
    // custody independently. A duplicate did NOT perform that transition.
    drop(preparation);
    let refused_after_failure = record
        .rejected_primary_for_test()
        .ok_or_else(|| eyre!("Unknown erased refused custody"))?;
    assert_eq!(
        refused_after_failure.session.thread_name.as_deref(),
        Some("refused exact incoming DTO")
    );
    assert!(refused_after_failure.alab_construction.is_none());
    drop(refused_after_failure);
    let weak_record = Arc::downgrade(&record);
    drop(record);
    drop(target);
    assert!(
        weak_record.upgrade().is_none(),
        "refused cached DTO must have no Record backlink"
    );
    assert!(captured.original.take().is_ok());
    Ok(())
}

#[tokio::test]
async fn failed_side_boundary_is_retained_before_policy_checks() -> Result<()> {
    let (target, record) = ConstructionRecord::side_preparation_fixture()?;
    let thread = started(&record).await?;
    let original = SidebarFork {
        thread,
        boundary: Err("original boundary failure".into()),
        inherited_profile: true,
        storage: crate::app_server_session::SideChatStorage::Attached,
        legacy: false,
        alab_retained_replay: None,
    };
    let preparation = record
        .capture_existing_side(original)
        .map_err(|failed| failed.error)?;
    let weak = Arc::downgrade(&preparation.captured);
    let cause = Arc::new(());
    let returned = preparation.fail(ExactCause(cause.clone()).into());
    assert!(
        returned
            .downcast_ref::<ExactCause>()
            .is_some_and(|actual| Arc::ptr_eq(&actual.0, &cause))
    );
    let captured = weak
        .upgrade()
        .ok_or_else(|| eyre!("failed side boundary object was dropped"))?;
    {
        let mut lease = captured_parts(&captured)?;
        let side = lease
            .get_mut()?
            .side
            .as_ref()
            .ok_or_else(|| eyre!("failed side outer fields lost"))?;
        assert_eq!(
            side.boundary.as_ref().err().map(String::as_str),
            Some("original boundary failure")
        );
        assert!(side.inherited_profile);
        assert!(!side.legacy);
        assert_eq!(
            side.storage,
            crate::app_server_session::SideChatStorage::Attached
        );
    }
    drop(record);
    drop(target);
    assert!(captured.record.upgrade().is_none());
    Ok(())
}

#[tokio::test]
async fn side_attachment_without_fork_outer_fields_keeps_explicit_side_presentation() -> Result<()>
{
    let (target, record) = ConstructionRecord::side_preparation_fixture()?;
    let original = started(&record).await?;
    let expected = original.session.clone();
    assert_eq!(
        record.presentation_data(),
        ClientPresentation::SideConversation
    );
    let preparation = record
        .capture_existing_side_attachment(original)
        .map_err(|failed| failed.error)?;
    let weak = Arc::downgrade(&preparation.captured);
    drop(preparation);
    let captured = weak
        .upgrade()
        .ok_or_else(|| eyre!("side attachment payload lost"))?;
    {
        let mut lease = captured_parts(&captured)?;
        let parts = lease.get_mut()?;
        assert_eq!(parts.presentation, ClientPresentation::SideConversation);
        assert!(
            parts.side.is_none(),
            "attachment must not fabricate fork outer fields"
        );
        assert_eq!(parts.started.session, expected);
        assert!(parts.started.alab_construction.is_none());
    }
    drop(record);
    drop(target);
    assert!(captured.record.upgrade().is_none());
    Ok(())
}

#[tokio::test]
async fn side_attachment_cannot_reclassify_primary_or_replace_original_payload() -> Result<()> {
    let (target, record) = ConstructionRecord::preparation_fixture()?;
    let original = started(&record).await?;
    let expected = original.session.clone();
    let refused = match record.capture_existing_side_attachment(original) {
        Ok(_) => return Err(eyre!("primary declaration became a side attachment")),
        Err(failed) => failed,
    };
    assert_eq!(refused.original.session, expected);
    assert!(
        refused
            .original
            .alab_construction
            .as_ref()
            .is_some_and(|actual| Arc::ptr_eq(actual, &record))
    );
    let primary = record
        .capture_existing_primary(refused.original)
        .map_err(|failed| failed.error)?;
    let captured = primary.captured.clone();
    drop(primary);
    {
        let mut lease = captured_parts(&captured)?;
        let parts = lease.get_mut()?;
        assert_eq!(parts.presentation, ClientPresentation::Primary);
        assert!(parts.side.is_none());
        assert_eq!(parts.started.session, expected);
    }
    drop(record);
    drop(target);
    assert!(captured.record.upgrade().is_none());
    Ok(())
}
