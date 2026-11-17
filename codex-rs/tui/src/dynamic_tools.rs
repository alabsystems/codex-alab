//! Task-management tools hosted by a TUI connected to an external app server.

#[path = "../../alab-core/glue/task_create_policy.rs"]
mod alab_task_create_policy; // ALAB(upgrade): the backend and TUI share the exact pure task policy.

#[path = "../../alab-core/glue/task_read_policy.rs"]
mod alab_task_read_policy; // ALAB(upgrade): TUI and retained task reads share exact pure policy.
use alab_task_read_policy::{ListArguments, ReadArguments, WaitArguments, same_thread_id, truncate}; // ALAB(upgrade): identical pure read/wait arguments and predicates.
pub(crate) use alab_task_read_policy::parse_delegated_tool_output; // ALAB(upgrade): keep existing replay callers on the shared parser.
#[cfg(test)]
use alab_task_read_policy::{DEFAULT_OUTPUT_CHARS, turn_summary}; // ALAB(upgrade): preserve existing formatting controls.

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_client::TypedRequestError;
use codex_app_server_protocol::Account;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::DynamicToolCallOutputContentItem;
use codex_app_server_protocol::DynamicToolCallParams;
use codex_app_server_protocol::DynamicToolCallResponse;
use codex_app_server_protocol::DynamicToolFunctionSpec;
use codex_app_server_protocol::DynamicToolNamespaceSpec;
use codex_app_server_protocol::DynamicToolNamespaceTool;
use codex_app_server_protocol::DynamicToolSpec;
use codex_app_server_protocol::GetAccountParams;
use codex_app_server_protocol::GetAccountResponse;
use codex_app_server_protocol::ModelListParams;
use codex_app_server_protocol::ModelListResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::SortDirection;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadArchiveParams;
use codex_app_server_protocol::ThreadArchiveResponse;
use codex_app_server_protocol::ThreadForkParams;
use codex_app_server_protocol::ThreadForkResponse;
use codex_app_server_protocol::ThreadHistoryMode;
use codex_app_server_protocol::ThreadItemEntry;
use codex_app_server_protocol::ThreadItemsListParams;
use codex_app_server_protocol::ThreadItemsListResponse;
use codex_app_server_protocol::ThreadListResponse;
use codex_app_server_protocol::ThreadReadParams;
use codex_app_server_protocol::ThreadReadResponse;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::ThreadSetNameParams;
use codex_app_server_protocol::ThreadSetNameResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStatus;
use codex_app_server_protocol::ThreadStatusChangedNotification;
use codex_app_server_protocol::ThreadTurnsListParams;
use codex_app_server_protocol::ThreadTurnsListResponse;
use codex_app_server_protocol::ThreadUnarchiveParams;
use codex_app_server_protocol::ThreadUnarchiveResponse;
#[cfg(test)]
use codex_app_server_protocol::Turn; // ALAB(upgrade): extracted projection leaves actual typed fixtures here.
use codex_app_server_protocol::TurnItemsView;
#[cfg(test)]
use codex_app_server_protocol::TurnStartParams; // ALAB(upgrade): retain the original typed turn fixtures after policy extraction.
use codex_app_server_protocol::TurnStartResponse;
#[cfg(test)]
use codex_app_server_protocol::UserInput; // ALAB(upgrade): extracted projection leaves actual typed fixtures here.
use codex_features::Feature;
use codex_features::Features;
use codex_protocol::ThreadId;
#[cfg(test)]
use codex_protocol::models::FunctionCallOutputBody; // ALAB(upgrade): preserve existing delegation parser controls.
use codex_protocol::models::PermissionProfile;
use serde::de::DeserializeOwned;
use serde_json::Value;
use serde_json::json;
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::time::Instant;
use uuid::Uuid;

#[path = "dynamic_tools_response.rs"]
mod response;

pub(crate) const NAMESPACE: &str = alab_task_create_policy::NAMESPACE; // ALAB(upgrade): preserve the original task namespace.
pub(crate) const DELEGATION_TOOLS: [&str; 3] =
    ["create_thread", "send_message_to_thread", "fork_thread"];
const MAX_ERROR_CHARS: usize = alab_task_read_policy::MAX_ERROR_CHARS; // ALAB(upgrade): preserve the same tool error budget.
const MAX_INPUT_BYTES: usize = alab_task_create_policy::MAX_INPUT_BYTES; // ALAB(upgrade): one exact prompt bound.
const MAX_DELEGATED_INPUT_BYTES: usize = alab_task_create_policy::MAX_DELEGATED_INPUT_BYTES; // ALAB(upgrade): one exact delegated prompt bound.

type CreateArguments = alab_task_create_policy::CreateArguments; // ALAB(upgrade): preserve exact CREATE argument validation.

#[path = "../../alab-core/glue/task_fork_policy.rs"]
mod alab_task_fork_policy; // ALAB(upgrade): identical pure fork schema/result in both actual hosts.
use alab_task_fork_policy::ForkArguments;

type SendArguments = alab_task_create_policy::SendArguments; // ALAB(upgrade): identical SEND arguments in both real hosts.

pub(crate) fn tool_specs() -> Vec<DynamicToolSpec> {
    let [list_threads, list_archived_threads, read_thread, wait_threads] = alab_task_read_policy::tool_definitions(); // ALAB(upgrade): preserve schemas in both actual hosts.
    let [set_thread_title, set_thread_archived] = alab_task_read_policy::management::tool_definitions(); // ALAB(upgrade): preserve both canonical management schemas.
    let definitions = [
        list_threads,
        list_archived_threads,
        read_thread,
        wait_threads,
        alab_task_create_policy::send_tool_definition(), // ALAB(upgrade): shared canonical SEND schema.
        alab_task_create_policy::create_tool_definition(), // ALAB(upgrade): same CREATE schema/description in both real hosts.
        alab_task_fork_policy::tool_definition(), // ALAB(upgrade): preserve canonical fork schema.
        set_thread_title,
        set_thread_archived,
    ];

    vec![DynamicToolSpec::Namespace(DynamicToolNamespaceSpec {
        name: NAMESPACE.to_string(),
        description: "Manage Codex tasks available through the connected app server.".to_string(),
        tools: definitions
            .into_iter()
            .map(|(name, description, properties, required)| {
                DynamicToolNamespaceTool::Function(DynamicToolFunctionSpec {
                    name: name.to_string(),
                    description: description.to_string(),
                    input_schema: json!({
                        "type": "object",
                        "additionalProperties": false,
                        "properties": properties,
                        "required": required
                    }),
                    defer_loading: true,
                })
            })
            .collect(),
    })]
}

pub(crate) fn non_delegation_tool_specs() -> Vec<DynamicToolSpec> {
    tool_specs()
        .into_iter()
        .filter_map(|spec| match spec {
            DynamicToolSpec::Function(function) => (!DELEGATION_TOOLS
                .contains(&function.name.as_str()))
            .then_some(DynamicToolSpec::Function(function)),
            DynamicToolSpec::Namespace(mut namespace) => {
                namespace.tools.retain(|tool| match tool {
                    DynamicToolNamespaceTool::Function(function) => {
                        !DELEGATION_TOOLS.contains(&function.name.as_str())
                    }
                });
                Some(DynamicToolSpec::Namespace(namespace))
            }
        })
        .collect()
}

pub(crate) fn failure_response(message: impl Into<String>) -> DynamicToolCallResponse {
    DynamicToolCallResponse {
        content_items: vec![DynamicToolCallOutputContentItem::InputText {
            text: truncate(&message.into(), MAX_ERROR_CHARS),
        }],
        success: false,
    }
}

pub(crate) async fn execute(
    request_handle: AppServerRequestHandle,
    params: DynamicToolCallParams,
    thread_start_params: ThreadStartParams,
    features: Features,
    status_updates: broadcast::Receiver<ThreadStatusChangedNotification>,
    app_event_tx: Option<&AppEventSender>,
) -> DynamicToolCallResponse {
    match execute_inner(
        request_handle,
        params,
        thread_start_params,
        features,
        status_updates,
        app_event_tx,
    )
    .await
    .and_then(success_response)
    {
        Ok(response) => response,
        Err(error) => failure_response(error),
    }
}

fn success_response(value: Value) -> Result<DynamicToolCallResponse, String> {
    let text = response::serialize(value)?;
    Ok(DynamicToolCallResponse {
        content_items: vec![DynamicToolCallOutputContentItem::InputText { text }],
        success: true,
    })
}

async fn execute_inner(
    handle: AppServerRequestHandle,
    params: DynamicToolCallParams,
    mut thread_start_params: ThreadStartParams,
    features: Features,
    mut status_updates: broadcast::Receiver<ThreadStatusChangedNotification>,
    app_event_tx: Option<&AppEventSender>,
) -> Result<Value, String> {
    let mcp_config = thread_start_params.config.as_ref().and_then(|overrides| {
        let key = format!("mcp_servers.{NAMESPACE}");
        overrides
            .get(&key)
            .map(|server| HashMap::from([(key, server.clone())]))
    });
    match params.tool.as_str() {
        "list_threads" | "list_archived_threads" => {
            let arguments: ListArguments = parse_arguments(params.arguments)?;
            let archived = params.tool == "list_archived_threads";
            let list_params = alab_task_read_policy::list_params(&arguments, archived)?; // ALAB(upgrade): one current-account/filter/limit policy.
            let response: ThreadListResponse = request(&handle, |request_id| ClientRequest::ThreadList { request_id, params: list_params }).await?;
            Ok(alab_task_read_policy::list_result(response, archived))
        }
        "read_thread" => {
            let arguments: ReadArguments = parse_arguments(params.arguments)?;
            let (turn_limit, output_chars) = alab_task_read_policy::read_limits(&arguments)?; // ALAB(upgrade): one exact read/output bound.
            let thread = read_thread(&handle, &arguments.thread_id).await?;
            let page: Result<ThreadTurnsListResponse, TypedRequestError> = handle
                .request_typed(ClientRequest::ThreadTurnsList {
                    request_id: RequestId::String(format!("tui-dynamic-{}", Uuid::new_v4())),
                    params: ThreadTurnsListParams {
                        thread_id: arguments.thread_id.clone(),
                        cursor: arguments.cursor.clone(),
                        limit: Some(turn_limit),
                        sort_direction: Some(SortDirection::Desc),
                        items_view: Some(TurnItemsView::Full),
                    },
                })
                .await;
            let (turns, next_cursor) = match page {
                Ok(page) => (page.data, page.next_cursor),
                Err(TypedRequestError::Server { source, .. })
                    if crate::app_server_session::is_history_pagination_unsupported(&source) =>
                {
                    let response: ThreadReadResponse =
                        request(&handle, |request_id| ClientRequest::ThreadRead {
                            request_id,
                            params: ThreadReadParams {
                                thread_id: arguments.thread_id.clone(),
                                include_turns: true,
                            },
                        })
                        .await?;
                    alab_task_read_policy::history_fallback(&response.thread, arguments.cursor.as_deref(), turn_limit)?
                }
                Err(error) => return Err(error.to_string()),
            };
            Ok(alab_task_read_policy::read_result(&thread, &turns, next_cursor, turn_limit, arguments.include_outputs == Some(true), output_chars)) // ALAB(upgrade): shared full result formatting.
        }
        "create_thread" => {
            let arguments: CreateArguments = parse_arguments(params.arguments)?;
            validate_prompt(&arguments.prompt, MAX_INPUT_BYTES)?;
            let prompt = delegated_prompt(&params.thread_id, &arguments.prompt);
            validate_prompt(&prompt, MAX_DELEGATED_INPUT_BYTES)?;
            if arguments
                .title
                .as_deref()
                .is_some_and(|title| title.trim().is_empty())
            {
                return Err("title must not be empty".to_string());
            }
            let source_thread = read_thread(&handle, &params.thread_id).await?;
            if source_thread.ephemeral {
                return Err(
                    "ephemeral tasks cannot create inspectable background tasks".to_string()
                );
            }
            thread_start_params.model_provider = Some(source_thread.model_provider.clone());
            thread_start_params.cwd = Some(source_thread.cwd.to_string_lossy().into_owned());
            thread_start_params.project_id = source_thread.project_id.clone();
            thread_start_params.ephemeral = Some(source_thread.ephemeral);
            if features.enabled(Feature::CliDaybreak)
                && thread_start_params.daybreak_enabled.is_none()
            {
                let defaults = crate::config_update::read_effective_config_if_supported(
                    handle.clone(),
                    source_thread.cwd.as_path(),
                )
                .await
                .map_err(|error| error.to_string())?;
                thread_start_params.daybreak_enabled = defaults.and_then(|defaults| {
                    defaults
                        .config
                        .additional
                        .get("daybreak")
                        .and_then(Value::as_bool)
                });
            }
            thread_start_params.history_mode = (source_thread.history_mode
                == ThreadHistoryMode::Paginated)
                .then_some(ThreadHistoryMode::Paginated);
            let exclude_turns = source_thread.history_mode == ThreadHistoryMode::Paginated;
            let source: ThreadResumeResponse = request_with_history_fallback(
                &handle,
                exclude_turns,
                |request_id, exclude_turns| ClientRequest::ThreadResume {
                    request_id,
                    params: ThreadResumeParams {
                        thread_id: params.thread_id.clone(),
                        exclude_turns,
                        ..ThreadResumeParams::default()
                    },
                },
            )
            .await?;
            thread_start_params.model = Some(source.model);
            thread_start_params.service_tier = Some(source.service_tier);
            thread_start_params.runtime_workspace_roots = Some(source.runtime_workspace_roots);
            thread_start_params.approval_policy = Some(source.approval_policy);
            thread_start_params.approvals_reviewer = Some(source.approvals_reviewer);
            let source_permissions =
                PermissionProfile::from_legacy_sandbox_policy_template(&source.sandbox.to_core());
            let inherited = alab_task_create_policy::inherit_permissions( // ALAB(upgrade): share the existing named-profile/legacy-sandbox rule.
                source.active_permission_profile.as_ref().map(|profile| profile.id.as_str()),
                source_permissions,
            )?;
            thread_start_params.permissions = inherited.profile;
            thread_start_params.sandbox = inherited.sandbox;
            let permission_profile_override = inherited.turn_override;
            if let Some(model) = arguments.model {
                thread_start_params.model = Some(model);
            }
            let (started, _, task_tools_available) =
                crate::app_server_session::request_thread_start_with_history_fallback(
                    &handle,
                    RequestId::String(format!("tui-dynamic-{}", Uuid::new_v4())),
                    thread_start_params,
                )
                .await
                .map_err(|error| error.to_string())?;
            let thread_id = started.thread.id.clone();
            let daybreak_enabled = started.thread.daybreak_enabled.unwrap_or(false);
            register_background_thread(app_event_tx, started.thread, task_tools_available).await?;
            let cyber_access_program = background_turn_program(
                &features,
                &handle,
                &thread_id,
                &started.model,
                &started.model_provider,
                daybreak_enabled,
            )
            .await?;
            if let Some(title) = arguments.title
                && let Err(error) = request::<ThreadSetNameResponse>(&handle, |request_id| {
                    ClientRequest::ThreadSetName {
                        request_id,
                        params: ThreadSetNameParams {
                            thread_id: thread_id.clone(),
                            name: title.trim().to_string(),
                        },
                    }
                })
                .await
            {
                tracing::warn!(thread_id, %error, "failed to name background task");
            }
            start_turn(
                &handle,
                &thread_id,
                "create_thread",
                prompt,
                /*model*/ None,
                permission_profile_override,
                cyber_access_program,
            )
            .await?;
            Ok(json!({"threadId": thread_id}))
        }
        "fork_thread" => {
            let arguments: ForkArguments = parse_arguments(params.arguments)?;
            let thread_id = arguments
                .thread_id
                .unwrap_or_else(|| params.thread_id.clone());
            let thread = read_thread(&handle, &thread_id).await?;
            // Keep source requests routable while the fork is being created.
            register_background_thread(
                app_event_tx,
                thread.clone(),
                /*task_tools_available*/ false,
            )
            .await?;
            let before_turn_id = if same_thread_id(&thread_id, &params.thread_id) {
                Some(params.turn_id)
            } else if matches!(thread.status, ThreadStatus::Active { .. }) {
                let page: Result<ThreadTurnsListResponse, TypedRequestError> = handle
                    .request_typed(ClientRequest::ThreadTurnsList {
                        request_id: RequestId::String(format!("tui-dynamic-{}", Uuid::new_v4())),
                        params: ThreadTurnsListParams {
                            thread_id: thread_id.clone(),
                            cursor: None,
                            limit: Some(1),
                            sort_direction: Some(SortDirection::Desc),
                            items_view: Some(TurnItemsView::NotLoaded),
                        },
                    })
                    .await;
                let turns = match page {
                    Ok(page) => page.data,
                    Err(TypedRequestError::Server { source, .. })
                        if crate::app_server_session::is_history_pagination_unsupported(
                            &source,
                        ) =>
                    {
                        let response: ThreadReadResponse =
                            request(&handle, |request_id| ClientRequest::ThreadRead {
                                request_id,
                                params: ThreadReadParams {
                                    thread_id: thread_id.clone(),
                                    include_turns: true,
                                },
                            })
                            .await?;
                        response.thread.turns
                    }
                    Err(error) => return Err(error.to_string()),
                };
                turns
                    .into_iter()
                    .find(|turn| turn.status == codex_app_server_protocol::TurnStatus::InProgress)
                    .map(|turn| turn.id)
            } else {
                None
            };
            let exclude_turns = thread.history_mode == ThreadHistoryMode::Paginated;
            let response: ThreadForkResponse = request_with_history_fallback(
                &handle,
                exclude_turns,
                |request_id, exclude_turns| ClientRequest::ThreadFork {
                    request_id,
                    params: ThreadForkParams {
                        thread_id: thread_id.clone(),
                        before_turn_id: before_turn_id.clone(),
                        ephemeral: thread.ephemeral,
                        exclude_turns,
                        config: mcp_config.clone(),
                        ..ThreadForkParams::default()
                    },
                },
            )
            .await?;
            let forked_thread_id = response.thread.id.clone();
            register_background_thread(
                app_event_tx,
                response.thread,
                /*task_tools_available*/ false,
            )
            .await?;
            Ok(alab_task_fork_policy::result(&thread_id, &forked_thread_id)) // ALAB(upgrade): original exact result shared with the actual backend.
        }
        "send_message_to_thread" => {
            let arguments: SendArguments = parse_arguments(params.arguments)?;
            let prompt = alab_task_create_policy::send_prompt(&params.thread_id, &arguments)?; // ALAB(upgrade): same SEND validation/delegation.

            let thread = read_thread(&handle, &arguments.thread_id).await?;
            let exclude_turns = thread.history_mode == ThreadHistoryMode::Paginated;
            let resumed: ThreadResumeResponse = request_with_history_fallback(
                &handle,
                exclude_turns,
                |request_id, exclude_turns| ClientRequest::ThreadResume {
                    request_id,
                    params: ThreadResumeParams {
                        thread_id: arguments.thread_id.clone(),
                        exclude_turns,
                        config: mcp_config.clone(),
                        ..ThreadResumeParams::default()
                    },
                },
            )
            .await?;
            let daybreak_enabled = thread.daybreak_enabled.unwrap_or(false);
            register_background_thread(
                app_event_tx,
                resumed.thread,
                /*task_tools_available*/ false,
            )
            .await?;
            let cyber_access_program = background_turn_program(
                &features,
                &handle,
                &arguments.thread_id,
                arguments.model.as_deref().unwrap_or(&resumed.model),
                &resumed.model_provider,
                daybreak_enabled,
            )
            .await?;
            start_turn(
                &handle,
                &arguments.thread_id,
                "send_message_to_thread",
                prompt,
                arguments.model,
                /*permission_profile_override*/ None,
                cyber_access_program,
            )
            .await?;
            Ok(json!({"threadId": arguments.thread_id}))
        }
        "set_thread_title" | "set_thread_archived" => {
            let operation = alab_task_read_policy::management::prepare(&params.thread_id, &params.tool, params.arguments)?; // ALAB(upgrade): identical argument/default/self-archive policy in both actual hosts.
            let result = operation.result();
            match operation {
                alab_task_read_policy::management::ManagementOperation::SetTitle { thread_id, title } => {
                    let _: ThreadSetNameResponse = request(&handle, |request_id| ClientRequest::ThreadSetName {
                        request_id,
                        params: ThreadSetNameParams { thread_id: thread_id.clone(), name: title.clone() },
                    }).await?;
                }
                alab_task_read_policy::management::ManagementOperation::SetArchived { thread_id, archived } => {
                    if archived {
                        let _: ThreadArchiveResponse = request(&handle, |request_id| ClientRequest::ThreadArchive {
                            request_id, params: ThreadArchiveParams { thread_id: thread_id.clone() },
                        }).await?;
                    } else {
                        let _: ThreadUnarchiveResponse = request(&handle, |request_id| ClientRequest::ThreadUnarchive {
                            request_id, params: ThreadUnarchiveParams { thread_id: thread_id.clone() },
                        }).await?;
                    }
                }
            }
            Ok(result)
        }
        "wait_threads" => {
            let arguments: WaitArguments = parse_arguments(params.arguments)?;
            let (timeout_ms, unique_targets) = alab_task_read_policy::wait_settings(&arguments, &params.thread_id)?; // ALAB(upgrade): preserve canonical/self/duplicate wait validation.
            let deadline = Instant::now() + Duration::from_millis(timeout_ms);
            let snapshot_deadline = if timeout_ms == 0 {
                Instant::now() + Duration::from_secs(/*secs*/ 5)
            } else {
                deadline
            };
            let result = alab_task_read_policy::wait_result; // ALAB(upgrade): same partial poll/timeout envelope.
            loop {
                let mut polls = Vec::with_capacity(arguments.targets.len());
                let mut errors = Vec::new();
                let mut wake = None;
                for (index, target) in arguments.targets.iter().enumerate() {
                    let now = Instant::now();
                    let target_deadline = now
                        + snapshot_deadline.saturating_duration_since(now)
                            / (arguments.targets.len() - index) as u32;
                    match tokio::time::timeout_at(
                        target_deadline,
                        read_thread(&handle, &target.thread_id),
                    )
                    .await
                    {
                        Ok(Ok(thread)) => {
                            let turns_page = tokio::time::timeout_at(
                                target_deadline,
                                handle.request_typed::<ThreadTurnsListResponse>(
                                    ClientRequest::ThreadTurnsList {
                                        request_id: RequestId::String(format!(
                                            "tui-dynamic-{}",
                                            Uuid::new_v4()
                                        )),
                                        params: ThreadTurnsListParams {
                                            thread_id: thread.id.clone(),
                                            cursor: None,
                                            limit: Some(1),
                                            sort_direction: Some(SortDirection::Desc),
                                            items_view: Some(TurnItemsView::Summary),
                                        },
                                    },
                                ),
                            )
                            .await;
                            let latest_turn = match turns_page {
                                Ok(Ok(page)) => page.data.into_iter().next(),
                                Ok(Err(TypedRequestError::Server { source, .. }))
                                    if crate::app_server_session::is_history_pagination_unsupported(
                                        &source,
                                    ) =>
                                {
                                    tokio::time::timeout_at(
                                        target_deadline,
                                        request::<ThreadReadResponse>(&handle, |request_id| {
                                            ClientRequest::ThreadRead {
                                                request_id,
                                                params: ThreadReadParams {
                                                    thread_id: thread.id.clone(),
                                                    include_turns: true,
                                                },
                                            }
                                        }),
                                    )
                                    .await
                                    .ok()
                                    .and_then(Result::ok)
                                    .and_then(|response| response.thread.turns.into_iter().next_back())
                                }
                                _ => None,
                            };
                            let latest_items = if let Some(turn) = &latest_turn {
                                tokio::time::timeout_at(
                                    target_deadline,
                                    request::<ThreadItemsListResponse>(&handle, |request_id| {
                                        ClientRequest::ThreadItemsList {
                                            request_id,
                                            params: ThreadItemsListParams {
                                                thread_id: thread.id.clone(),
                                                turn_id: Some(turn.id.clone()),
                                                cursor: None,
                                                limit: Some(20),
                                                sort_direction: Some(SortDirection::Desc),
                                                defer_output: None,
                                            },
                                        }
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
                            let (poll, target_wake) = alab_task_read_policy::wait_poll(&thread, &latest_turn, &latest_items, target)?; // ALAB(upgrade): same cursor, actionable predicate and bounded projection.
                            if wake.is_none() { wake = target_wake; }
                            polls.push(poll);
                            if wake.is_some() {
                                break;
                            }
                        }
                        Ok(Err(message)) => {
                            errors.push(json!({"threadId": target.thread_id, "message": message}))
                        }
                        Err(_) => errors.push(json!({
                            "threadId": target.thread_id,
                            "message": "Timed out while reading task status"
                        })),
                    }
                }
                if wake.is_some() || polls.is_empty() || Instant::now() >= deadline {
                    return Ok(result(
                        wake.is_none()
                            && (!polls.is_empty()
                                || (timeout_ms > 0 && Instant::now() >= deadline)),
                        wake,
                        polls,
                        errors,
                    ));
                }
                loop {
                    let refresh_at = deadline.min(Instant::now() + Duration::from_secs(/*secs*/ 1));
                    match tokio::time::timeout_at(refresh_at, status_updates.recv()).await {
                        Ok(Ok(update)) if unique_targets.contains(update.thread_id.as_str()) => {
                            break;
                        }
                        Ok(Ok(_)) => continue,
                        Ok(Err(broadcast::error::RecvError::Lagged(_))) => break,
                        Ok(Err(broadcast::error::RecvError::Closed)) => break,
                        Err(_) if Instant::now() >= deadline => {
                            return Ok(result(/*timed_out*/ true, wake, polls, errors));
                        }
                        Err(_) => break,
                    }
                }
            }
        }
        tool => Err(format!("Unsupported TUI dynamic tool: {tool}")),
    }
}

fn parse_arguments<T: DeserializeOwned>(arguments: Value) -> Result<T, String> {
    serde_json::from_value(arguments).map_err(|error| format!("Invalid tool arguments: {error}"))
}

fn validate_prompt(prompt: &str, max_bytes: usize) -> Result<(), String> {
    alab_task_create_policy::validate_prompt(prompt, max_bytes) // ALAB(upgrade): preserve task prompt validation.
}

fn delegated_prompt(source_thread_id: &str, prompt: &str) -> String {
    alab_task_create_policy::delegated_prompt(source_thread_id, prompt) // ALAB(upgrade): preserve canonical tool output wrapping.
}

async fn register_background_thread(
    app_event_tx: Option<&AppEventSender>,
    mut thread: Thread,
    task_tools_available: bool,
) -> Result<(), String> {
    if let Some(app_event_tx) = app_event_tx {
        thread.turns.clear();
        let (registered, registration) = tokio::sync::oneshot::channel();
        app_event_tx.send(AppEvent::DynamicToolThreadStarted {
            thread,
            task_tools_available,
            registered,
        });
        registration
            .await
            .map_err(|error| format!("Failed to register background task: {error}"))?;
    }
    Ok(())
}

async fn request<T: DeserializeOwned>(
    handle: &AppServerRequestHandle,
    build: impl FnOnce(RequestId) -> ClientRequest,
) -> Result<T, String> {
    handle
        .request_typed(build(RequestId::String(format!(
            "tui-dynamic-{}",
            Uuid::new_v4()
        ))))
        .await
        .map_err(|error| error.to_string())
}

async fn request_with_history_fallback<T: DeserializeOwned>(
    handle: &AppServerRequestHandle,
    exclude_turns: bool,
    mut build: impl FnMut(RequestId, bool) -> ClientRequest,
) -> Result<T, String> {
    let request_id = || RequestId::String(format!("tui-dynamic-{}", Uuid::new_v4()));
    match handle
        .request_typed(build(request_id(), exclude_turns))
        .await
    {
        Err(TypedRequestError::Server { source, .. })
            if exclude_turns
                && crate::app_server_session::is_history_pagination_unsupported(&source) =>
        {
            handle
                .request_typed(build(request_id(), /*exclude_turns*/ false))
                .await
                .map_err(|error| error.to_string())
        }
        result => result.map_err(|error| error.to_string()),
    }
}

async fn read_thread(handle: &AppServerRequestHandle, thread_id: &str) -> Result<Thread, String> {
    let response: ThreadReadResponse = request(handle, |request_id| ClientRequest::ThreadRead {
        request_id,
        params: ThreadReadParams {
            thread_id: thread_id.to_string(),
            include_turns: false,
        },
    })
    .await?;
    Ok(response.thread)
}

async fn start_turn(
    handle: &AppServerRequestHandle,
    thread_id: &str,
    tool: &str,
    prompt: String,
    model: Option<String>,
    permission_profile_override: Option<PermissionProfile>,
    cyber_access_program: Option<codex_app_server_protocol::CyberAccessProgram>,
) -> Result<TurnStartResponse, String> {
    let params = alab_task_create_policy::initial_tool_turn( // ALAB(upgrade): identical tool-output input in TUI and backend.
        thread_id.to_string(), tool, prompt, model, permission_profile_override, cyber_access_program,
    )?;
    request(handle, |request_id| ClientRequest::TurnStart { request_id, params }).await
}

async fn background_turn_program(
    features: &Features,
    handle: &AppServerRequestHandle,
    thread_id: &str,
    model: &str,
    provider: &str,
    enabled: bool,
) -> Result<Option<codex_app_server_protocol::CyberAccessProgram>, String> {
    if !features.enabled(Feature::CliDaybreak) {
        return Ok(None);
    }
    let (account, models) = tokio::join!(
        request::<GetAccountResponse>(handle, |request_id| ClientRequest::GetAccount {
            request_id,
            params: GetAccountParams {
                refresh_token: false
            },
        }),
        request::<ModelListResponse>(handle, |request_id| ClientRequest::ModelList {
            request_id,
            params: ModelListParams {
                cursor: None,
                limit: None,
                include_hidden: Some(true),
                model_provider: None,
            },
        }),
    );
    let account = match account {
        Ok(response) => response.account,
        Err(error) if enabled && provider == "openai" => {
            return Err(format!(
                "Could not determine Daybreak account eligibility: {error}"
            ));
        }
        Err(_) => None,
    };
    let eligible = provider == "openai"
        && match account {
            Some(Account::Chatgpt { .. }) => true,
            Some(Account::ApiKey { .. }) => {
                // Only the target thread can establish its forwarding policy, including
                // project opt-outs. Older servers that cannot report it keep omission.
                let (tx, rx) = tokio::sync::oneshot::channel();
                crate::experimental_features::fetch(
                    handle.clone(),
                    Some(ThreadId::from_string(thread_id).map_err(|error| error.to_string())?),
                    format!("tui-dynamic-{}", Uuid::new_v4()),
                    tx,
                );
                if !rx.await.ok().and_then(Result::ok).is_some_and(|features| {
                    features.into_iter().any(|feature| {
                        feature.name == Feature::ApiKeyCyberAccessPrograms.key() && feature.enabled
                    })
                }) {
                    return Ok(None);
                }
                true
            }
            Some(Account::AmazonBedrock { .. } | Account::Gcp {}) | None => false,
        };
    // Retain saved metadata while suppressing it for accounts that cannot use Daybreak.
    let enabled = enabled && eligible;
    let models = models
        .map(|response| {
            response
                .data
                .into_iter()
                .map(crate::app_server_session::model_preset_from_api_model)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    crate::daybreak::program_for_turn(&models, model, eligible, enabled)
        .map(|program| program.map(Into::into))
}

#[cfg(test)]
#[path = "dynamic_tools_tests.rs"]
mod tests;
