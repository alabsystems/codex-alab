// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Original title/archive task arguments and pure policy. This is a child of
//! the same task policy module in the actual TUI and backend host crates.
use super::same_thread_id;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TitleArguments {
    thread_id: Option<String>,
    title: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ArchiveArguments {
    thread_id: Option<String>,
    archived: bool,
}

pub(crate) enum ManagementOperation {
    SetTitle { thread_id: String, title: String },
    SetArchived { thread_id: String, archived: bool },
}
impl ManagementOperation {
    pub(crate) fn result(&self) -> Value {
        match self {
            Self::SetTitle { thread_id, title } => json!({"threadId":thread_id,"title":title}),
            Self::SetArchived {
                thread_id,
                archived,
            } => json!({"threadId":thread_id,"archived":archived}),
        }
    }
}

pub(crate) fn prepare(
    issuing_parent: &str,
    tool: &str,
    arguments: Value,
) -> Result<ManagementOperation, String> {
    match tool {
        "set_thread_title" => {
            let arguments: TitleArguments = serde_json::from_value(arguments)
                .map_err(|error| format!("Invalid tool arguments: {error}"))?;
            if arguments.title.trim().is_empty() {
                return Err("title must not be empty".into());
            }
            Ok(ManagementOperation::SetTitle {
                thread_id: arguments.thread_id.unwrap_or_else(|| issuing_parent.into()),
                title: arguments.title,
            })
        }
        "set_thread_archived" => {
            let arguments: ArchiveArguments = serde_json::from_value(arguments)
                .map_err(|error| format!("Invalid tool arguments: {error}"))?;
            let thread_id = arguments.thread_id.unwrap_or_else(|| issuing_parent.into());
            if arguments.archived && same_thread_id(&thread_id, issuing_parent) {
                return Err("cannot archive the calling task".into());
            }
            Ok(ManagementOperation::SetArchived {
                thread_id,
                archived: arguments.archived,
            })
        }
        _ => Err(format!("Unsupported TUI dynamic tool: {tool}")),
    }
}

pub(crate) fn tool_definitions() -> [(&'static str, &'static str, Value, Vec<&'static str>); 2] {
    let thread_id = json!({"type":"string","minLength":1});
    [
        (
            "set_thread_title",
            "Rename a Codex task. Omit threadId to rename the calling task.",
            json!({"threadId":thread_id,"title":{"type":"string","minLength":1}}),
            vec!["title"],
        ),
        (
            "set_thread_archived",
            "Archive a Codex task and its descendants, or restore only the selected task. Omit threadId to update the calling task.",
            json!({"threadId":thread_id,"archived":{"type":"boolean"}}),
            vec!["archived"],
        ),
    ]
}
