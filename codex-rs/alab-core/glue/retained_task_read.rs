// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Read-only task operations on the retained host's actual processor. The live
//! issuing parent comes from the host's opaque binding. Explicit target IDs are
//! read arguments, never a Native role, frontend identity or permission grant.
#![cfg(unix)]
use super::ThreadRequestProcessor;
use crate::request_processors::OperationContext;
use codex_alab_core::native_incoming::Incoming;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::SortDirection;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadItemEntry;
use codex_app_server_protocol::ThreadItemsListParams;
use codex_app_server_protocol::ThreadReadParams;
use codex_app_server_protocol::ThreadStatus;
use codex_app_server_protocol::ThreadTurnsListParams;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnItemsView;
use codex_core::CodexThread;
use codex_core::ThreadRegistration;
use codex_protocol::ThreadId;
use futures::future::select_all;
use serde::de::DeserializeOwned;
use serde_json::Value;
use serde_json::json;
use std::future::pending;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tokio::time::Instant;
use tracing::Instrument;

#[path = "task_history_support.rs"]
pub(super) mod history;
#[path = "task_read_policy.rs"]
pub(crate) mod policy;
#[path = "../../tui/src/dynamic_tools_response.rs"]
mod response; // The original standalone pure serializer is reused unchanged.

pub(crate) fn supports(tool: &str) -> bool {
    matches!(
        tool,
        "list_threads" | "list_archived_threads" | "read_thread" | "wait_threads"
    )
}

pub(crate) struct BackendReadExecutor {
    processor: ThreadRequestProcessor,
    incoming: Incoming,
}
impl BackendReadExecutor {
    /// Only the actual retained constructor supplies these same-host objects.
    pub(crate) fn new(processor: ThreadRequestProcessor, incoming: Incoming) -> Self {
        Self {
            processor,
            incoming,
        }
    }

    async fn check_parent(
        &self,
        parent_id: ThreadId,
        parent: &Arc<CodexThread>,
        context: &OperationContext,
    ) -> Result<(), String> {
        if self.incoming.is_stopped()
            || context.cancellation.is_cancelled()
            || parent.has_shutdown_status()
        {
            return Err("retained task read owner withdrew".into());
        }
        if !matches!(
            self.processor
                .thread_manager
                .thread_registration(parent_id, parent)
                .await,
            ThreadRegistration::Current
        ) {
            return Err("issuing task runtime is no longer current".into());
        }
        if self.incoming.is_stopped() || context.cancellation.is_cancelled() {
            return Err("retained task read owner withdrew".into());
        }
        Ok(())
    }

    /// Read-only work is canceled directly with its actual operation/owner.
    /// There is no mutation retry, detached request or invented client context.
    pub(crate) async fn execute(
        &self,
        parent_id: ThreadId,
        parent: Arc<CodexThread>,
        context: OperationContext,
        tool: &str,
        arguments: Value,
    ) -> Result<String, String> {
        let work = async {
            self.check_parent(parent_id, &parent, &context).await?;
            let result = match tool {
                "list_threads" | "list_archived_threads" => {
                    let arguments: policy::ListArguments = parse(arguments)?;
                    let archived = tool == "list_archived_threads";
                    let params = policy::list_params(&arguments, archived)?;
                    let list = self
                        .processor
                        .thread_list_response_inner(params)
                        .await
                        .map_err(|error| server_error("thread/list", error))?;
                    policy::list_result(list, archived)
                }
                "read_thread" => self.read(parse(arguments)?).await?,
                "wait_threads" => {
                    self.wait(parent_id, &parent, &context, parse(arguments)?)
                        .await?
                }
                _ => return Err(format!("Unsupported TUI dynamic tool: {tool}")),
            };
            self.check_parent(parent_id, &parent, &context).await?;
            response::serialize(result)
        };
        tokio::select! {
            biased;
            () = self.incoming.withdrawn() => Err("retained task read owner withdrew".into()),
            () = context.cancellation.cancelled() => Err("task read operation cancelled".into()),
            () = parent.wait_until_terminated() => Err("issuing task runtime terminated".into()),
            result = work.instrument(context.span.clone()) => result,
        }
    }

    async fn thread(&self, id: &str, include_turns: bool) -> Result<Thread, String> {
        self.processor
            .thread_read_response_inner(ThreadReadParams {
                thread_id: id.into(),
                include_turns,
            })
            .await
            .map(|response| response.thread)
            .map_err(|error| server_error("thread/read", error))
    }

    async fn read(&self, arguments: policy::ReadArguments) -> Result<Value, String> {
        let (limit, output_chars) = policy::read_limits(&arguments)?;
        let thread = self.thread(&arguments.thread_id, false).await?;
        let page = self
            .processor
            .thread_turns_list_response_inner(ThreadTurnsListParams {
                thread_id: arguments.thread_id.clone(),
                cursor: arguments.cursor.clone(),
                limit: Some(limit),
                sort_direction: Some(SortDirection::Desc),
                items_view: Some(TurnItemsView::Full),
            })
            .await;
        let (turns, cursor) = match page {
            Ok(page) => (page.data, page.next_cursor),
            Err(error) if history::is_history_pagination_unsupported(&error) => {
                let history = self.thread(&arguments.thread_id, true).await?;
                policy::history_fallback(&history, arguments.cursor.as_deref(), limit)?
            }
            Err(error) => return Err(server_error("thread/turns/list", error)),
        };
        Ok(policy::read_result(
            &thread,
            &turns,
            cursor,
            limit,
            arguments.include_outputs == Some(true),
            output_chars,
        ))
    }

    async fn latest(
        &self,
        thread: &Thread,
        deadline: Instant,
    ) -> (Option<Turn>, Option<Vec<ThreadItemEntry>>) {
        let page = tokio::time::timeout_at(
            deadline,
            self.processor
                .thread_turns_list_response_inner(ThreadTurnsListParams {
                    thread_id: thread.id.clone(),
                    cursor: None,
                    limit: Some(1),
                    sort_direction: Some(SortDirection::Desc),
                    items_view: Some(TurnItemsView::Summary),
                }),
        )
        .await;
        let latest_turn = match page {
            Ok(Ok(page)) => page.data.into_iter().next(),
            Ok(Err(error)) if history::is_history_pagination_unsupported(&error) => {
                tokio::time::timeout_at(deadline, self.thread(&thread.id, true))
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .and_then(|thread| thread.turns.into_iter().next_back())
            }
            _ => None,
        };
        let latest_items = if let Some(turn) = &latest_turn {
            tokio::time::timeout_at(
                deadline,
                self.processor
                    .thread_items_list_response_inner(ThreadItemsListParams {
                        thread_id: thread.id.clone(),
                        turn_id: Some(turn.id.clone()),
                        cursor: None,
                        limit: Some(20),
                        sort_direction: Some(SortDirection::Desc),
                        defer_output: None,
                    }),
            )
            .await
            .ok()
            .and_then(Result::ok)
            .map(|page| page.data)
            .or_else(|| {
                Some(
                    turn.items
                        .iter()
                        .rev()
                        .take(20)
                        .cloned()
                        .map(|item| ThreadItemEntry {
                            turn_id: turn.id.clone(),
                            output_deferred: false,
                            started_at_ms: item.started_at_ms,
                            completed_at_ms: item.completed_at_ms,
                            item,
                        })
                        .collect(),
                )
            })
        } else {
            None
        };
        (latest_turn, latest_items)
    }

    async fn wait(
        &self,
        parent_id: ThreadId,
        parent: &Arc<CodexThread>,
        context: &OperationContext,
        arguments: policy::WaitArguments,
    ) -> Result<Value, String> {
        let (timeout_ms, _) = policy::wait_settings(&arguments, &parent_id.to_string())?;
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        let snapshot_deadline = if timeout_ms == 0 {
            Instant::now() + Duration::from_secs(5)
        } else {
            deadline
        };
        loop {
            let mut polls = Vec::with_capacity(arguments.targets.len());
            let mut errors = Vec::new();
            let mut wake = None;
            let mut watches: Vec<watch::Receiver<ThreadStatus>> =
                Vec::with_capacity(arguments.targets.len());
            for (index, target) in arguments.targets.iter().enumerate() {
                let now = Instant::now();
                let target_deadline = now
                    + snapshot_deadline.saturating_duration_since(now)
                        / (arguments.targets.len() - index) as u32;
                let snapshot = async {
                    self.check_parent(parent_id, parent, context).await?;
                    // Invalid target IDs still become the original per-target
                    // read error, not a new whole-operation rejection. Watching
                    // never upserts a runtime or provides a result/authority.
                    if let Ok(id) = ThreadId::from_string(&target.thread_id)
                        && let Some(mut receiver) = self
                            .processor
                            .thread_watch_manager
                            .alab_subscribe_tracked_task(id)
                            .await
                    {
                        drop(receiver.borrow_and_update());
                        watches.push(receiver);
                    }
                    self.thread(&target.thread_id, false).await
                };
                match tokio::time::timeout_at(target_deadline, snapshot).await {
                    Ok(Ok(thread)) => {
                        // Keep a successful thread snapshot even when optional
                        // turns/items exhaust their remaining deadline. The
                        // original tool still reports that partial poll.
                        let (latest_turn, latest_items) = self.latest(&thread, target_deadline).await;
                        let (poll, target_wake) = policy::wait_poll(&thread, &latest_turn, &latest_items, target)?;
                        if wake.is_none() { wake = target_wake; }
                        polls.push(poll);
                        if wake.is_some() { break; }
                    }
                    Ok(Err(message)) => errors.push(json!({"threadId":target.thread_id,"message":message})),
                    Err(_) => errors.push(json!({"threadId":target.thread_id,"message":"Timed out while reading task status"})),
                }
            }
            if wake.is_some() || polls.is_empty() || Instant::now() >= deadline {
                return Ok(policy::wait_result(
                    wake.is_none()
                        && (!polls.is_empty() || (timeout_ms > 0 && Instant::now() >= deadline)),
                    wake,
                    polls,
                    errors,
                ));
            }
            let refresh_at = deadline.min(Instant::now() + Duration::from_secs(1));
            let changed = async {
                if watches.is_empty() {
                    pending::<()>().await;
                }
                let changes = watches
                    .iter_mut()
                    .map(|receiver| {
                        Box::pin(async move {
                            let _ = receiver.changed().await;
                        })
                    })
                    .collect::<Vec<_>>();
                let _ = select_all(changes).await;
            };
            match tokio::time::timeout_at(refresh_at, changed).await {
                // Closed receivers refresh too; next pass re-subscribes so a
                // removed/recreated target cannot leave a permanently-ready loop.
                Ok(()) => {}
                Err(_) if Instant::now() >= deadline => {
                    return Ok(policy::wait_result(true, wake, polls, errors));
                }
                Err(_) => {}
            }
        }
    }
}

fn parse<T: DeserializeOwned>(arguments: Value) -> Result<T, String> {
    serde_json::from_value(arguments).map_err(|error| format!("Invalid tool arguments: {error}"))
}

// Preserve the existing tool's typed server-error display without introducing
// an app-server-client dependency or an actual/fabricated client request.
pub(crate) fn server_error(method: &str, error: JSONRPCErrorError) -> String {
    let mut message = format!("{method} failed: {} (code {})", error.message, error.code);
    if let Some(data) = error.data {
        message.push_str(&format!(", data: {data}"));
    }
    message
}

#[path = "retained_task_management.rs"]
pub(crate) mod management;
