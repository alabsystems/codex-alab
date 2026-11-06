// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Local recovery of one existing exec turn. The caller supplies native account
//! admission; neither a stored settings object nor a thread ID authorizes work.

use codex_alab_core::quota::HeadlessExec;
use codex_alab_core::renewal::TurnAdmission;
use codex_core::CodexThread;
use codex_core::ExecRecoveryOptions;
use codex_core::RecoverTurnRequest;
use codex_core::StartIfIdleSubmission;
#[cfg(test)]
use codex_core::ThreadManager;
use codex_core::WithTurnExtensionData;
use codex_extension_api::ExtensionDataInit;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

#[path = "exec_recovery_tree.rs"]
mod tree;
pub use tree::ExecRecoveryTree;

/// One original-turn recovery attempt and its live native admission callback.
/// Clones identify the same attempt. A successful reply may be reconciled by
/// retrying a clone; a refusal or failure requires fresh native admission.
#[derive(Clone)]
pub struct ExecRecoveryRequest {
    options: Arc<ExecRecoveryOptions>,
    admission: Arc<TurnAdmission>,
    attempted: Arc<AtomicBool>,
    launch: Option<codex_core::ExecLaunchPermit>,
    review: Option<ExtensionDataInit>,
}

impl ExecRecoveryRequest {
    /// The callback must revalidate the native receipt, current account,
    /// owner/cancellation state and policy before accepting the original turn.
    /// Constructing this request does not perform any of those checks.
    pub fn new(
        options: ExecRecoveryOptions,
        admit: impl Fn(
            String,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<(), String>> + Send>,
        > + Send
        + Sync
        + 'static,
    ) -> Self {
        Self {
            options: Arc::new(options),
            admission: Arc::new(TurnAdmission::new(admit)),
            attempted: Arc::new(AtomicBool::new(false)),
            launch: None,
            review: None,
        }
    }

    /// Coordinate this admitted original turn with the other recovered tasks.
    /// The permit only delays execution; the callback still supplies live admission.
    pub fn new_for_launch(
        options: ExecRecoveryOptions,
        launch: codex_core::ExecLaunchPermit,
        admit: impl Fn(
            String,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<(), String>> + Send>,
        > + Send
        + Sync
        + 'static,
    ) -> Result<Self, String> {
        if !launch.matches(options.thread(), options.turn()) {
            return Err("exec launch permit does not match recovery options".into());
        }
        let mut request = Self::new(options, admit);
        request.launch = Some(launch);
        Ok(request)
    }

    fn same_attempt(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.attempted, &other.attempted)
    }
}

#[derive(Default)]
pub(super) struct State {
    accepted: Option<(ExecRecoveryRequest, StartIfIdleSubmission)>,
}

async fn unfinished(thread: &CodexThread, options: &ExecRecoveryOptions) -> Result<(), String> {
    // A completed/aborted turn may already be idle while its terminal record
    // remains queued. Persist it before deciding that an older running marker
    // is still recoverable, including the recheck under Core's reservation.
    thread
        .flush_rollout()
        .await
        .map_err(|_| "exec recovery history could not be made durable".to_owned())?;
    let history = thread
        .exec_recovery_history()
        .await
        .map_err(|_| "exec recovery history is unavailable".to_owned())?;
    let matches = if options.is_review() {
        options.matches_open_history(&history)
    } else {
        history.thread_id == options.thread()
            && codex_alab_autopilot::unfinished_recovery_turn(&history.items, options.turn())
    };
    if !matches {
        return Err("exec recovery no longer names the original unfinished turn".to_owned());
    }
    Ok(())
}

impl State {
    #[cfg(test)]
    pub(super) async fn run(
        &mut self,
        manager: &ThreadManager,
        request: ExecRecoveryRequest,
    ) -> Result<StartIfIdleSubmission, String> {
        if let Some((accepted, result)) = &self.accepted {
            return if accepted.same_attempt(&request) {
                Ok(result.clone())
            } else {
                Err("exec runtime already accepted a different recovery attempt".to_owned())
            };
        }
        let thread = manager
            .get_thread(request.options.thread())
            .await
            .map_err(|_| "exec recovery thread is unavailable".to_owned())?;
        self.run_loaded(thread, request).await
    }

    async fn run_loaded(
        &mut self,
        thread: Arc<CodexThread>,
        request: ExecRecoveryRequest,
    ) -> Result<StartIfIdleSubmission, String> {
        if let Some((accepted, result)) = &self.accepted {
            return if accepted.same_attempt(&request) {
                Ok(result.clone())
            } else {
                Err("exec runtime already accepted a different recovery attempt".to_owned())
            };
        }
        if request.attempted.swap(true, Ordering::AcqRel) {
            return Err("exec recovery requires a freshly validated admission".to_owned());
        }
        if thread.exec_thread_id() != request.options.thread() {
            return Err("exec recovery changed the loaded runtime identity".into());
        }
        if thread
            .thread_extension_data()
            .get::<HeadlessExec>()
            .is_none()
        {
            return Err("exec recovery is unavailable in this runtime".to_owned());
        }
        unfinished(&thread, &request.options).await?;

        let mut init = request.review.clone().unwrap_or_default();
        init.insert(request.options.as_ref().clone());
        if let Some(launch) = &request.launch {
            init.insert(launch.clone());
        }
        let original = request.options.clone();
        let current = Arc::downgrade(&thread);
        let admission = Arc::clone(&request.admission);
        init.insert(TurnAdmission::new(move |turn| {
            let original = original.clone();
            let current = current.clone();
            let admission = Arc::clone(&admission);
            Box::pin(async move {
                if turn != original.turn() {
                    return Err("exec recovery changed the original turn".to_owned());
                }
                let thread = current
                    .upgrade()
                    .ok_or("exec recovery runtime is no longer present")?;
                // Core has reserved the idle turn before invoking this callback.
                // Recheck durable history here, then invoke the caller's live
                // native admission before Core records a start or consumes input.
                unfinished(&thread, &original).await?;
                admission.admit(turn).await
            })
        }));
        let result = thread
            .recover_turn_if_idle(WithTurnExtensionData::new(
                RecoverTurnRequest {
                    turn_id: request.options.turn().to_owned(),
                    thread_settings: request.options.thread_settings(),
                    trace: None,
                    cyber_access_program: None,
                },
                init,
            ))
            .await
            .map_err(|_| "exec recovery was not admitted".to_owned())?;
        if matches!(result, StartIfIdleSubmission::Started { .. }) {
            self.accepted = Some((request, result.clone()));
        }
        Ok(result)
    }
}
