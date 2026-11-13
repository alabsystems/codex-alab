// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Native-admitted idle queue installation shared by roots and descendants.
use super::*;

type IdleOutcome = tokio::sync::watch::Receiver<Option<Result<(), String>>>;

/// Retained queue data bound to one actual destination member. It neither
/// starts a turn nor grants native admission. Dropped waiters retain one owner.
pub struct ExecIdleRestore {
    thread: Arc<CodexThread>,
    input: Mutex<Option<PreparedExecIdleInput>>,
    outcome: OnceLock<IdleOutcome>,
}

struct RootBinding(std::sync::Weak<ExecIdleRestore>);

pub(super) fn pending_root(thread: &Arc<CodexThread>) -> bool {
    thread
        .session
        .services
        .thread_extension_data
        .get::<RootBinding>()
        .and_then(|binding| binding.0.upgrade())
        .is_some_and(|owner| Arc::ptr_eq(&owner.thread, thread))
        && thread
            .session
            .active_turn
            .try_lock()
            .is_ok_and(|active| active.is_none())
}

impl ExecIdleRestore {
    #[cfg(test)]
    pub(super) fn retained_len(&self) -> Option<usize> {
        self.input
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .map(PreparedExecIdleInput::len)
    }

    pub(super) fn new(thread: Arc<CodexThread>, input: PreparedExecIdleInput) -> Arc<Self> {
        Arc::new(Self {
            thread,
            input: Mutex::new(Some(input)),
            outcome: OnceLock::new(),
        })
    }

    /// Publish pending delivery only after every queue in the forest is installed.
    pub async fn activate(&self) -> Result<(), String> {
        let outcome = self.outcome.get().ok_or("idle input was not restored")?;
        let restored = matches!(outcome.borrow().as_ref(), Some(Ok(())));
        if !restored {
            return Err("idle input restoration did not succeed".into());
        }
        exec_input_state::activate_idle(&self.thread.session).await
    }

    /// Retain one installation across cancellation and reconcile retries. The
    /// callback must perform fresh native admission for this exact thread.
    pub async fn restore(
        self: &Arc<Self>,
        admit: impl FnOnce(
            ThreadId,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<(), String>> + Send>,
        > + Send
        + 'static,
    ) -> Result<(), String> {
        let owner = Arc::clone(self);
        let mut outcome = self
            .outcome
            .get_or_init(move || {
                let (send, receive) = tokio::sync::watch::channel(None);
                tokio::spawn(async move {
                    #[expect(
                        clippy::await_holding_invalid_type,
                        reason = "retain exact registration only across local queue installation, after native admission has completed"
                    )]
                    let result = async {
                        if owner
                            .input
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .is_none()
                        {
                            return Err("member has no retained idle input".into());
                        }
                        let _pin = Arc::clone(&owner.thread.residency_gate).read_owned().await;
                        let state = owner
                            .thread
                            .session
                            .services
                            .local_agent_runtime
                            .upgrade()
                            .map_err(|e| e.to_string())?;
                        if !state
                            .threads
                            .read()
                            .await
                            .get(&owner.thread.session.thread_id)
                            .is_some_and(|current| Arc::ptr_eq(current, &owner.thread))
                            || !owner.thread.is_running()
                        {
                            return Err("idle input owner is no longer registered".into());
                        }
                        admit(owner.thread.session.thread_id).await?;
                        // Admission can await a native host. Revalidate the exact
                        // owner afterward and retain registration through install.
                        let registered = state.threads.read().await;
                        if !registered
                            .get(&owner.thread.session.thread_id)
                            .is_some_and(|current| Arc::ptr_eq(current, &owner.thread))
                            || !owner.thread.is_running()
                        {
                            return Err("idle input owner retired during admission".into());
                        }
                        let input = owner
                            .input
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .take()
                            .ok_or("idle input was already taken")?;
                        exec_input_state::install_idle(&owner.thread.session, input).await?;
                        drop(registered);
                        Ok(())
                    }
                    .await;
                    let _ = send.send_replace(Some(result));
                });
                receive
            })
            .clone();
        loop {
            if let Some(result) = outcome.borrow_and_update().clone() {
                return result;
            }
            outcome
                .changed()
                .await
                .map_err(|_| "idle restoration owner exited without a result".to_owned())?;
        }
    }
}

impl CodexThread {
    /// Bind a decoded idle-root frame to its actual fresh destination. Source
    /// metadata supplies data only; restore still requires live native admission.
    pub fn prepare_exec_idle_root_restore(
        self: &Arc<Self>,
        input: PreparedExecIdleInput,
    ) -> Result<Arc<ExecIdleRestore>, String> {
        if self.session_source.is_non_root_agent()
            || input.thread() != self.session.thread_id
            || !input.owns_controller()
        {
            return Err("idle root input requires its matching root controller owner".into());
        }
        let owner = ExecIdleRestore::new(Arc::clone(self), input);
        self.session
            .services
            .thread_extension_data
            .insert(RootBinding(Arc::downgrade(&owner)));
        Ok(owner)
    }
}
