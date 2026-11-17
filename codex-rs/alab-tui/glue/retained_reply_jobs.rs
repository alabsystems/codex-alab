// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Custody of the original human response before its existing reply await.
//! A successful socket write is Unconfirmed, not Source callback acceptance.
//! No retry, synthetic reply, reconnect, or authority follows from saved DATA.
use super::*;
use crate::app::App;
use crate::app_server_session::AppServerSession;
use crate::owned_local_frontend::FrontendTenure;
use serde::Deserialize;
use serde::Serialize;
use std::io;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::atomic::AtomicU8;
use std::sync::atomic::Ordering;

const QUEUED: u8 = 0;
const SENDING: u8 = 1;
const WRITTEN: u8 = 2;
const UNKNOWN: u8 = 3;

#[derive(Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum HumanReplyState {
    Queued {},
    Unconfirmed {},
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct HumanReplyData {
    schema: u32,
    thread_id: String,
    request_id: AppServerRequestId,
    result: serde_json::Value,
    observation: HumanReplyState,
}
impl HumanReplyData {
    pub(crate) fn request_id_data(&self) -> &AppServerRequestId {
        &self.request_id
    }
    pub(crate) fn thread_id_data(&self) -> &str {
        &self.thread_id
    }
    pub(crate) fn validate_data(&self) -> io::Result<()> {
        if self.schema != 1 || codex_protocol::ThreadId::from_string(&self.thread_id).is_err() {
            return Err(invalid(
                "retained human reply has invalid schema/thread DATA",
            ));
        }
        Ok(())
    }
}

/// Original default construction is disabled. Only SAME actual final session
/// tenure installs this owner before UI can answer its retained callbacks.
#[derive(Clone, Default)]
pub(crate) struct Replies {
    owner: Option<Arc<Owner>>,
}
impl std::fmt::Debug for Replies {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Replies([retained original response owners])")
    }
}
struct Owner {
    tenure: FrontendTenure,
    client: AppServerRequestHandle,
    // Kept through original clear/session routing. Only a later authenticated
    // Source outcome +actual UI application may retire these original owners.
    jobs: Mutex<Vec<Arc<Submission>>>,
    inputs: Mutex<Vec<Arc<original_input::Item>>>,
    callbacks: Arc<callbacks::CallbackUi>,
}
pub(in crate::app) struct Submission {
    thread: codex_protocol::ThreadId,
    id: AppServerRequestId,
    result: serde_json::Value,
    // Same claimed atomic and original command channel, never a new handle.
    reply: Option<AppServerReplyHandle>,
    special_elicitation: bool,
    phase: Arc<AtomicU8>,
}
impl Replies {
    pub(crate) fn for_session(session: &AppServerSession) -> io::Result<Self> {
        let mut actual = Self::default();
        actual.bind(session)?;
        Ok(actual)
    }
    pub(crate) fn callback_owner(
        &self,
        session: &AppServerSession,
    ) -> io::Result<Arc<callbacks::CallbackUi>> {
        let owner = self
            .owner
            .as_ref()
            .ok_or_else(|| invalid("actual retained replies are not bound"))?;
        owner.callbacks.validate(session)?;
        Ok(owner.callbacks.clone())
    }
    pub(crate) fn restore_transfer(
        &self,
        session: &AppServerSession,
        data: serde_json::Value,
        limits: callbacks::Limits,
    ) -> io::Result<Arc<callbacks::Transfer>> {
        let owner = self.callback_owner(session)?;
        callbacks::restore_owned(&owner, self, session, data, limits)
    }
    fn bind(&mut self, session: &AppServerSession) -> io::Result<()> {
        let tenure = session
            .retained_frontend()
            .ok_or_else(|| invalid("retained reply owner requires actual final delivery"))?;
        if let Some(owner) = &self.owner {
            if owner.tenure.same_delivery(tenure) {
                return Ok(());
            }
            return Err(invalid(
                "original reply owners cannot move to a different physical delivery",
            ));
        }
        self.owner = Some(Arc::new(Owner {
            tenure: tenure.clone(),
            client: session.request_handle(),
            jobs: Mutex::new(Vec::new()),
            inputs: Mutex::new(Vec::new()),
            callbacks: callbacks::CallbackUi::new(tenure.clone()),
        }));
        Ok(())
    }
    pub(in crate::app) fn retain(
        &mut self,
        thread: codex_protocol::ThreadId,
        original_op: &AppCommand,
        actual: &PendingResolution,
    ) -> io::Result<Option<Arc<Submission>>> {
        let Some(owner) = &self.owner else {
            return Ok(None);
        };
        let phase = Arc::new(AtomicU8::new(QUEUED));
        let submission = Arc::new(Submission {
            thread,
            id: actual.request.request_id.clone(),
            result: actual.result.clone(),
            reply: actual.request.reply.clone(),
            phase,
            special_elicitation: matches!(
                original_op,
                AppCommand::ResolveElicitation { .. } | AppCommand::ResolveUserVerification { .. }
            ),
        });
        // Store BEFORE all fallible correlation checks so a bad/stale original
        // reply cannot disappear after take_resolution_record removed its map entry.
        let mut jobs = owner.jobs.lock().unwrap_or_else(PoisonError::into_inner);
        jobs.push(submission.clone());
        if !submission
            .reply
            .as_ref()
            .is_some_and(|reply| reply.retained_pending_for_client(&owner.client))
            || jobs[..jobs.len() - 1]
                .iter()
                .any(|prior| prior.id == submission.id)
        {
            submission.phase.store(UNKNOWN, Ordering::Release);
            return Err(invalid(
                "retained original reply is missing, claimed, foreign, or duplicate",
            ));
        }
        Ok(Some(submission))
    }
    pub(in crate::app) fn capture(
        &self,
        session: &AppServerSession,
    ) -> io::Result<Vec<HumanReplyData>> {
        let owner = self
            .owner
            .as_ref()
            .ok_or_else(|| invalid("retained reply owner was not installed before human input"))?;
        if !session
            .retained_frontend()
            .is_some_and(|tenure| owner.tenure.same_delivery(tenure))
        {
            return Err(invalid(
                "retained reply capture belongs to another actual delivery",
            ));
        }
        let mut ids = std::collections::HashSet::new();
        let jobs = owner.jobs.lock().unwrap_or_else(PoisonError::into_inner);
        jobs.iter().map(|job| {
            if job.special_elicitation {
                return Err(io::Error::new(io::ErrorKind::WouldBlock,
                    "elicitation/verification response needs its actual separate owner/auth-revision outcome"));
            }
            if !ids.insert(job.id.clone()) { return Err(invalid("duplicate retained human response identity is uncertain")); }
            Ok(HumanReplyData { schema: 1, thread_id: job.thread.to_string(), request_id: job.id.clone(),
                result: job.result.clone(), observation: if job.phase.load(Ordering::Acquire) == QUEUED {
                    HumanReplyState::Queued {}
                } else { HumanReplyState::Unconfirmed {} } })
        }).collect()
    }
}
impl Submission {
    pub(in crate::app) fn begin(&self) -> io::Result<Attempt> {
        begin(&self.phase)
    }
}
fn begin(phase: &Arc<AtomicU8>) -> io::Result<Attempt> {
    phase
        .compare_exchange(QUEUED, SENDING, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| invalid("original human reply cannot be resubmitted"))?;
    Ok(Attempt {
        phase: phase.clone(),
        observed: false,
    })
}
pub(in crate::app) struct Attempt {
    phase: Arc<AtomicU8>,
    observed: bool,
}
impl Attempt {
    pub(in crate::app) fn observed(mut self, actual: &io::Result<()>) {
        self.phase.store(
            if actual.is_ok() { WRITTEN } else { UNKNOWN },
            Ordering::Release,
        );
        self.observed = true;
    }
}
impl Drop for Attempt {
    fn drop(&mut self) {
        if !self.observed {
            self.phase.store(UNKNOWN, Ordering::Release);
        }
    }
}

impl App {
    /// Move the SAME pre-App configured owner before any event drain. No
    /// callback/view owner may be reconstructed on the App side of handoff.
    pub(crate) fn alab_adopt_retained_reply_owner(
        &mut self,
        session: &AppServerSession,
        actual: Replies,
    ) -> io::Result<()> {
        if self.app_server_backend_id != Some(session.backend_id()) {
            return Err(invalid("reply handoff changed actual backend"));
        }
        let owner = actual
            .owner
            .as_ref()
            .ok_or_else(|| invalid("reply handoff is unbound"))?;
        owner.callbacks.validate(session)?;
        if self
            .pending_app_server_requests
            .alab_retained_replies
            .owner
            .as_ref()
            .is_some_and(|prior| !Arc::ptr_eq(prior, owner))
        {
            return Err(invalid("reply handoff would replace another actual owner"));
        }
        self.pending_app_server_requests.alab_retained_replies = actual;
        Ok(())
    }
    /// Required actual constructor hook before displaying/answering retained
    /// callbacks. No target label or saved reply DATA enables this owner.
    pub(crate) fn alab_bind_retained_reply_owner(
        &mut self,
        session: &AppServerSession,
    ) -> io::Result<()> {
        if self.app_server_backend_id != Some(session.backend_id()) {
            return Err(invalid(
                "retained reply owner has a different actual backend",
            ));
        }
        self.pending_app_server_requests
            .alab_retained_replies
            .bind(session)
    }
    pub(crate) fn alab_capture_retained_human_replies(
        &self,
        session: &AppServerSession,
    ) -> io::Result<Vec<HumanReplyData>> {
        if self.app_server_backend_id != Some(session.backend_id()) {
            return Err(invalid(
                "retained reply capture has a different actual backend",
            ));
        }
        self.pending_app_server_requests
            .alab_retained_replies
            .capture(session)
    }
}
fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

#[cfg(test)]
#[path = "retained_reply_jobs_tests.rs"]
mod tests;

#[path = "retained_callback_ui.rs"]
pub(in crate::app) mod callbacks;

#[path = "retained_original_input_driver.rs"]
pub(crate) mod original_input;
