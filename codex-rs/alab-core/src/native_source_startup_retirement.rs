// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Retire only an actual unused early constructor, with its original Native
//! owner. Absence/consumption is DATA, never proof a credential cohort is gone.
use super::*;
use crate::native_source_retirement::Receipt;

/// Only inspect the existing startup attempt. This never calls enter/allocate
/// or opens another constructor. An unfinished original attempt stays owned
/// through observation cancellation; all callers share its real retirement.
/// The Go caller must separately close/drain the actual reader census first.
pub async fn retire_unused_startup() -> io::Result<Option<Receipt>> {
    STARTUP.retire_unused().await
}
impl Startup {
    async fn retire_unused(&self) -> io::Result<Option<Receipt>> {
        let Some(pending) = self.seed.get() else {
            return Ok(None);
        };
        loop {
            let changed = pending.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if let Some(result) = pending.result.get() {
                let seed = result.as_ref().map_err(|_| unavailable())?;
                let (receipt, allocation) = {
                    let mut seed = seed.lock().map_err(|_| unavailable())?;
                    let mut retirement = pending.retirement.lock().map_err(|_| unavailable())?;
                    dispatch_seed(&mut seed, &mut retirement, |actual| {
                        if actual.parent_use.is_some()
                            || actual.derived_parent.is_some()
                            || actual.target.is_some()
                        {
                            return Err(unavailable());
                        }
                        // start() owns this exact member BEFORE its allocation
                        // leaves the seed. It retries the same release request;
                        // only its actual Native response can publish Closed.
                        crate::native_source_retirement::start(
                            Arc::clone(&actual.member),
                            None,
                            None,
                        )
                    })?
                };
                drop(allocation); // Never destroy a resource under seed/ledger locks.
                return Ok(receipt);
            }
            changed.await;
        }
    }
}

/// Production-shared local ownership transaction. A closure is only dispatch,
/// not retirement evidence; tests below exercise bookkeeping without a Native
/// member or receipt constructor.
fn dispatch_seed<A, R: Clone>(
    seed: &mut Option<A>,
    retiring: &mut Option<R>,
    dispatch: impl FnOnce(&A) -> io::Result<R>,
) -> io::Result<(Option<R>, Option<A>)> {
    if let Some(receipt) = retiring {
        return Ok((Some(receipt.clone()), None));
    }
    let Some(actual) = seed.as_ref() else {
        return Ok((None, None));
    };
    let receipt = dispatch(actual)?;
    *retiring = Some(receipt.clone());
    Ok((Some(receipt), seed.take()))
}

#[cfg(test)]
#[path = "native_source_startup_retirement_tests.rs"]
mod tests;
