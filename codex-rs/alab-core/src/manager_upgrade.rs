// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Final source-catalog closure, not native destination execution authority.
use super::*;

struct Staged(Option<Seal>);
impl Drop for Staged {
    fn drop(&mut self) {
        if let Some(seal) = self.0.take() {
            // Failure remains closed. Never reopen after retirement effects.
            let _ = seal.cancel_before_effects();
        }
    }
}

type Outcome = tokio::sync::watch::Receiver<Option<Result<(), Refused>>>;

/// The old constructor cohort remains closed even when its caller disappears.
/// Successful drain includes the catalog's actual native reader ACKs. Neither
/// this token nor its completion authorizes a destination SDK constructor.
pub struct CommittedUpgrade {
    seal: Seal,
    _census: Vec<Arc<Domain>>,
    outcome: OnceLock<Outcome>,
}

impl Registry {
    /// Call only at the final synchronous host decision, after revocable runtime,
    /// goal and queue preparation. Pending constructors or work refuse before
    /// `decide` runs. A rejected decision reopens the catalog without retirement.
    /// The callback must not reenter this catalog; it runs under its admission lock.
    pub fn commit_upgrade_if(
        self: &Arc<Self>,
        decide: impl FnOnce() -> bool,
    ) -> Result<Option<Arc<CommittedUpgrade>>, Refused> {
        let mut staged = Staged(Some(self.seal()?));
        let seal = staged.0.as_ref().ok_or(Refused::Unavailable)?;
        let census = seal.preflight_census()?;
        if !seal.begin_retirement_if(decide)? {
            return Ok(None);
        }
        Ok(Some(Arc::new(CommittedUpgrade {
            seal: staged.0.take().ok_or(Refused::Unavailable)?,
            _census: census,
            outcome: OnceLock::new(),
        })))
    }
}

impl CommittedUpgrade {
    /// Drain through one owned operation. Cancelling a transport/host waiter
    /// cannot interrupt reader retirement or let a retry start it a second time.
    pub async fn drain(self: &Arc<Self>) -> Result<(), Refused> {
        let owner = self.clone();
        let mut outcome = self
            .outcome
            .get_or_init(move || {
                let (send, receive) = tokio::sync::watch::channel(None);
                tokio::spawn(async move {
                    let result = owner.seal.drain_resources().await;
                    drop(owner);
                    send.send_replace(Some(result));
                });
                receive
            })
            .clone();
        loop {
            if let Some(result) = *outcome.borrow_and_update() {
                return result;
            }
            outcome.changed().await.map_err(|_| Refused::Unavailable)?;
        }
    }
}

#[cfg(test)]
#[path = "manager_upgrade_tests.rs"]
mod tests;
