// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual registry-owned editor resources through C0 spawn observation/staging.
//! This is a local ownership cut, not Native acceptance, NoFrontendIO or Go.
//! Only the original actor lends its real Tui; no App or sender is retained here.
use super::*;
use codex_alab_tui::frontend_control::retained_service::original_editor::C0Capture;
use codex_alab_tui::frontend_control::retained_service::original_editor::C0SpawnCompleted;
use codex_alab_tui::input::C0EditorResources;
use codex_alab_tui::input::C0EditorStage;
use std::os::fd::BorrowedFd;

pub(crate) struct Transfer {
    operation: Arc<Operation>,
    capture: Arc<C0Capture>,
    progress: std::sync::Mutex<Progress>,
}
struct Progress {
    spawn: Option<Arc<C0SpawnCompleted>>,
    resources: Option<Resources>,
    errors: Vec<Arc<io::Error>>,
    unknown: bool,
    returned: bool,
}
enum Resources {
    Staged(C0EditorStage),
    Refused(C0EditorResources),
}
impl Transfer {
    pub(crate) fn is_for(&self, operation: &Operation) -> bool {
        self.operation.same(operation) && self.capture.is_for(operation)
    }
    pub(crate) fn capture(&self) -> &Arc<C0Capture> {
        &self.capture
    }
    pub(crate) fn spawn(&self) -> io::Result<Arc<C0SpawnCompleted>> {
        self.progress
            .lock()
            .map_err(|_| unavailable("C0 original editor stage poisoned"))?
            .spawn
            .clone()
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "actual editor spawn has not joined",
                )
            })
    }
    fn retain(&self, error: io::Error) {
        self.progress
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .errors
            .push(Arc::new(error));
    }
    /// Restrict before the FIRST actual Native IO-custody request, including
    /// its control/callback await. A failed or unanswered request cannot restore
    /// the parent locally even if no descriptor send has been observed yet.
    pub(crate) fn retain_offer_started(&self) -> io::Result<()> {
        let progress = self
            .progress
            .try_lock()
            .map_err(|_| unavailable("C0 original editor resources already borrowed"))?;
        if progress.unknown || progress.returned {
            return Err(unavailable(
                "C0 original editor stage is unknown or returned",
            ));
        }
        match progress.resources.as_ref() {
            Some(Resources::Staged(actual)) => actual.mark_offer_started(),
            _ => Err(unavailable(
                "C0 original editor resource stage is incomplete",
            )),
        }
    }
    /// Mark every original domain before the exact synchronous SCM sender sees
    /// one File. A partial send/error leaves the same globally held originals.
    /// This takes no acceptance label and cannot retire any descriptor.
    pub(crate) fn with_offered_descriptions<R>(
        &self,
        action: impl FnOnce([BorrowedFd<'_>; 3]) -> io::Result<R>,
    ) -> io::Result<R> {
        let progress = self
            .progress
            .try_lock()
            .map_err(|_| unavailable("C0 original editor resources already borrowed"))?;
        if progress.unknown || progress.returned {
            return Err(unavailable(
                "C0 original editor stage is unknown or returned",
            ));
        }
        match progress.resources.as_ref() {
            Some(Resources::Staged(actual)) => actual.with_offered_descriptions(action),
            _ => Err(unavailable(
                "C0 original editor resource stage is incomplete",
            )),
        }
    }
}
impl Job {
    pub(super) fn c0_resources_held(&self) -> io::Result<bool> {
        Ok(self
            .c0
            .lock()
            .map_err(|_| unavailable("C0 editor keeper poisoned"))?
            .is_some())
    }
    pub(super) fn require_local_c0_released(&self) -> io::Result<()> {
        if self.c0_resources_held()? {
            return Err(unavailable("original editor C0 resources remain held"));
        }
        Ok(())
    }
    /// The actual global Job stores the capture BEFORE its first await. That
    /// same owner then stores the real spawn join before taking any original
    /// File; cancelling an observer loses none of these resources.
    pub(crate) async fn stage_original_c0(
        &self,
        tui: &crate::tui::Tui,
    ) -> io::Result<Arc<Transfer>> {
        let _serial = self.cut_gate.acquire().await.map_err(io::Error::other)?;
        self.handoff
            .lock()
            .map_err(|_| unavailable("original editor handoff poisoned"))?
            .is_for(tui)?;
        if self
            .cut
            .lock()
            .map_err(|_| unavailable("original editor cut poisoned"))?
            .is_some()
            || self
                .native_started
                .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(unavailable("same-PID editor cut already exists"));
        }
        let actual = {
            let mut slot = self
                .c0
                .lock()
                .map_err(|_| unavailable("C0 editor keeper poisoned"))?;
            if let Some(actual) = slot.as_ref() {
                if !actual.is_for(&self.operation) {
                    return Err(unavailable("C0 editor owner changed operation"));
                }
                actual.clone()
            } else {
                let actual = Arc::new(Transfer {
                    operation: self.operation.clone(),
                    capture: Arc::new(self.operation.capture_c0()?),
                    progress: std::sync::Mutex::new(Progress {
                        spawn: None,
                        resources: None,
                        errors: Vec::new(),
                        unknown: false,
                        returned: false,
                    }),
                });
                *slot = Some(actual.clone());
                actual
            }
        };
        let spawn = {
            actual
                .progress
                .lock()
                .map_err(|_| unavailable("C0 editor stage poisoned"))?
                .spawn
                .clone()
        };
        if spawn.is_none() {
            let joined = match actual.capture.spawn_completed().await {
                Ok(joined) => Arc::new(joined),
                Err(error) => {
                    let kind = error.kind();
                    actual.retain(error);
                    return Err(io::Error::new(
                        kind,
                        "actual original editor spawn join failed",
                    ));
                }
            };
            actual
                .progress
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .spawn = Some(joined);
        }
        let mut progress = actual
            .progress
            .lock()
            .map_err(|_| unavailable("C0 editor stage poisoned"))?;
        if progress.unknown || progress.returned {
            return Err(unavailable("C0 editor stage is unknown or returned"));
        }
        if progress.resources.is_some() {
            if matches!(progress.resources, Some(Resources::Staged(_))) {
                drop(progress);
                return Ok(actual);
            }
            return Err(unavailable(
                "C0 editor stage retains a refused partial owner",
            ));
        }
        let spawn = progress
            .spawn
            .as_ref()
            .cloned()
            .ok_or_else(|| unavailable("actual joined editor spawn absent"))?;
        let mut prechild = self
            .prechild
            .lock()
            .map_err(|_| unavailable("actual editor pre-child bundle poisoned"))?;
        let resources = prechild
            .take()
            .ok_or_else(|| unavailable("actual editor pre-child bundle absent"))?;
        // Every ordinary Err publishes its exact returned native owners before
        // returning. No await follows the first original File move.
        let mut attempt = StageAttempt {
            progress: &mut progress,
            complete: false,
        };
        let failure = match resources.stage_c0(&actual.capture, &spawn) {
            Ok(resources) => {
                attempt.progress.resources = Some(Resources::Staged(resources));
                None
            }
            Err(refused) => {
                attempt.progress.resources = Some(Resources::Refused(refused.actual));
                let kind = refused.error.kind();
                attempt.progress.errors.push(Arc::new(refused.error));
                Some(kind)
            }
        };
        attempt.complete = true;
        drop(attempt);
        drop(prechild);
        drop(progress);
        if let Some(kind) = failure {
            return Err(io::Error::new(
                kind,
                "actual C0 editor stage refused; same partial owners retained",
            ));
        }
        Ok(actual)
    }
    /// Local rollback only before ANY offer started. Actual native partial or
    /// uncertain sends refuse in the lower stage and keep the same Job owner.
    /// This restores slots only; original mode/Tui/parser handback remains the
    /// ordinary actor's genuine Finished path after this cut has returned.
    pub(crate) async fn return_local_c0_before_offer(
        &self,
        actual: &Arc<Transfer>,
    ) -> io::Result<()> {
        let _serial = self.cut_gate.acquire().await.map_err(io::Error::other)?;
        let mut keeper = self
            .c0
            .lock()
            .map_err(|_| unavailable("C0 editor keeper poisoned"))?;
        if !keeper
            .as_ref()
            .is_some_and(|stored| Arc::ptr_eq(stored, actual))
            || !actual.is_for(&self.operation)
        {
            return Err(unavailable("C0 return changed actual global editor Job"));
        }
        let mut progress = actual
            .progress
            .lock()
            .map_err(|_| unavailable("C0 editor stage poisoned"))?;
        if progress.unknown || progress.returned {
            return Err(unavailable("C0 return is unknown or already complete"));
        }
        let mut prechild = self
            .prechild
            .lock()
            .map_err(|_| unavailable("actual editor pre-child bundle poisoned"))?;
        if let Some(resources) = progress.resources.take() {
            if prechild.is_some() {
                progress.resources = Some(resources);
                return Err(unavailable("actual original bundle already present"));
            }
            let mut attempt = StageAttempt {
                progress: &mut progress,
                complete: false,
            };
            let restored = match resources {
                Resources::Staged(actual) => actual.rollback_before_offer(),
                Resources::Refused(actual) => actual.rollback_before_offer(),
            };
            match restored {
                Ok(original) => {
                    *prechild = Some(original);
                    attempt.complete = true;
                }
                Err(refused) => {
                    attempt.progress.resources = Some(Resources::Refused(refused.actual));
                    let kind = refused.error.kind();
                    attempt.progress.errors.push(Arc::new(refused.error));
                    attempt.complete = true;
                    return Err(io::Error::new(
                        kind,
                        "C0 return refused; same owners retained",
                    ));
                }
            }
        } else if prechild.is_none() {
            return Err(unavailable("C0 return has no actual original owners"));
        }
        progress.returned = true;
        let retired = keeper.take();
        drop(prechild);
        drop(progress);
        drop(keeper);
        drop(retired);
        Ok(())
    }
}
struct StageAttempt<'a> {
    progress: &'a mut Progress,
    complete: bool,
}
impl Drop for StageAttempt<'_> {
    fn drop(&mut self) {
        if !self.complete {
            self.progress.unknown = true;
        }
    }
}
fn unavailable(message: &'static str) -> io::Error {
    io::Error::other(message)
}
