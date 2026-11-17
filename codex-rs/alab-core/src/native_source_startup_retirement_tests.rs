// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Local seed dispatch/ownership policy only, not a Native Closed/SDK grant.
use super::*;
#[test]
fn dispatch_failure_retains_exact_unconsumed_seed_and_does_not_publish_a_receipt() {
    let actual = Arc::new(17_u32);
    let mut seed = Some(actual.clone());
    let mut retiring: Option<Arc<()>> = None;
    let observed = dispatch_seed(&mut seed, &mut retiring, |_| {
        Err(io::Error::from(io::ErrorKind::WouldBlock))
    });
    assert!(observed.is_err());
    assert!(seed.as_ref().is_some_and(|seed| Arc::ptr_eq(seed, &actual)));
    assert!(retiring.is_none());
}
#[test]
fn dispatch_is_once_and_returns_actual_destruction_custody_outside_the_lock_transaction()
-> Result<(), Box<dyn std::error::Error>> {
    let actual = Arc::new(17_u32);
    let mut seed = Some(actual.clone());
    let mut retiring: Option<Arc<()>> = None;
    let local_token = Arc::new(()); // Local token, explicitly NOT a Native receipt.
    let (first, removed) = dispatch_seed(&mut seed, &mut retiring, |value| {
        assert!(Arc::ptr_eq(value, &actual));
        Ok(local_token.clone())
    })?;
    assert!(
        removed
            .as_ref()
            .is_some_and(|value| Arc::ptr_eq(value, &actual))
    );
    assert!(
        first
            .as_ref()
            .is_some_and(|value| Arc::ptr_eq(value, &local_token))
    );
    assert!(seed.is_none());
    let (again, removed) = dispatch_seed(&mut seed, &mut retiring, |_| -> io::Result<Arc<()>> {
        panic!("same retirement dispatch must not be started again")
    })?;
    assert!(
        again
            .as_ref()
            .is_some_and(|value| Arc::ptr_eq(value, &local_token))
    );
    assert!(removed.is_none());
    Ok(())
}
#[test]
fn consumed_or_absent_seed_never_opens_or_claims_a_reader_cohort() -> io::Result<()> {
    let mut seed: Option<()> = None;
    let mut retiring: Option<Arc<()>> = None;
    let (receipt, removed) = dispatch_seed(&mut seed, &mut retiring, |_| -> io::Result<Arc<()>> {
        panic!("no original unused seed exists")
    })?;
    assert!(receipt.is_none() && removed.is_none());
    Ok(())
}
#[tokio::test]
async fn observing_retirement_without_an_early_attempt_does_not_create_one() -> io::Result<()> {
    let actual = Startup::default();
    assert!(actual.retire_unused().await?.is_none());
    assert!(actual.seed.get().is_none());
    Ok(())
}
