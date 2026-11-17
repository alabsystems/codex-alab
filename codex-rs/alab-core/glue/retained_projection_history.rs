// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual captured-Core history capability, NOT a target label or permission.
//! Protected listener callers retain their exact Core/lifecycle ownership; this
//! helper performs only the existing typed read policy with explicit UI intent.
use super::*;

pub(crate) struct Captured {
    pub(crate) durable_pages: bool,
    pub(crate) include_turns: bool,
    pub(crate) history_items: Vec<RolloutItem>,
    pub(crate) memory_mode: Option<ThreadHistoryMode>,
}
pub(crate) async fn capture(
    id: ThreadId,
    actual: &CodexThread,
    config: &ThreadConfigSnapshot,
    paginated: bool,
    original_include_turns: bool,
    has_initial_page: bool,
    current_display: bool,
) -> Result<Captured, JSONRPCErrorError> {
    let asks_history = original_include_turns || has_initial_page || current_display;
    // Same actual writer generation + local cutoff + original inherited display
    // view. Metadata-only Original keeps its old local getter; failures never
    // substitute an empty prefix, file lookup, persistence or another runtime.
    let (deferred, inherited_items) = if asks_history {
        actual
            .retained_deferred_display_history()
            .await
            .map_err(thread_store_resume_read_error)?
            .map(|display| (display.local, display.inherited_items))
            .unzip()
    } else {
        (
            actual
                .retained_deferred_history()
                .await
                .map_err(thread_store_resume_read_error)?,
            None,
        )
    };
    if let Some(deferred) = &deferred
        && (deferred.thread_id != id
            || deferred.session_meta.id != id
            || deferred.session_meta.history_mode != config.history_mode)
    {
        return Err(invalid_request(
            "deferred history belongs to another actual runtime",
        ));
    }
    let durable_pages = paginated
        && config.history_mode == ThreadHistoryMode::Paginated
        && !config.ephemeral
        && deferred.is_none();
    // Original live metadata fence stays before any cursor/history projection.
    // Non-durable paths deliberately do not activate the durable checkpoint.
    alab_running_resume::prepare_projection(actual, durable_pages).await?;
    // Actual capability, not a caller assertion of emptiness/durability. An
    // explicit UI request includes real memory history when there is no genuine
    // durable cursor route. Original supplied options otherwise stay unchanged.
    let include_turns = original_include_turns || (current_display && !durable_pages);
    let needs_history = !durable_pages && (include_turns || has_initial_page);
    let history_items = if needs_history {
        if let Some(deferred) = deferred {
            let mut items = inherited_items.ok_or_else(|| {
                internal_error("requested deferred display prefix was not captured")
            })?;
            items.extend(deferred.items);
            items
        } else if config.ephemeral {
            actual
                .exec_recovery_history()
                .await
                .map_err(thread_store_resume_read_error)?
                .items
        } else {
            actual
                .load_history(false)
                .await
                .map_err(thread_store_resume_read_error)?
                .items
        }
    } else {
        Vec::new()
    };
    Ok(Captured {
        durable_pages,
        include_turns,
        history_items,
        memory_mode: (!durable_pages).then_some(config.history_mode),
    })
}
