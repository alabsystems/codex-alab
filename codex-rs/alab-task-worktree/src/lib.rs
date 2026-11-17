// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The existing local worktree manager and its bounded tool contracts.
//! Allocation owns no SDK authority and changes no issuing thread cwd/permissions.
use codex_app_server_protocol::DynamicToolSpec;
use codex_app_server_protocol::ThreadAttachmentAddResponse;
use codex_app_server_protocol::ThreadAttachmentListResponse;
use codex_app_server_protocol::ThreadAttachmentUpdatedNotification;
use codex_worktree::CreateWorktree;
use codex_worktree::ManagedWorktree;
use codex_worktree::WorktreeManager;
use codex_worktree::WorktreeSettings;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;

// Reuse the original standalone schema source, without depending on the TUI.
#[path = "../../tui/src/managed_worktree_tool_specs.rs"]
mod original_specs;

pub use codex_worktree::ManagedWorktree as ManagedCheckout;

pub fn validate_host_settings(
    home: &std::path::Path,
    desktop: Option<&HashMap<String, Value>>,
) -> Result<(), String> {
    WorktreeSettings::for_cli(home, desktop)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// The actual original source builder after final interactive choices. This is
/// process-local custody: no default, deserializer, or Config reconstruction.
pub struct SourceConfig {
    builder: codex_core::config::ConfigBuilder,
}
impl SourceConfig {
    pub fn new(builder: codex_core::config::ConfigBuilder) -> Self {
        Self { builder }
    }
    pub async fn check_trusted(&self, cwd: PathBuf) -> Result<(), String> {
        let source = self
            .builder
            .clone()
            .fallback_cwd(Some(cwd))
            .build()
            .await
            .map_err(|error| error.to_string())?;
        if source.active_project.is_untrusted() {
            Err("Source project is explicitly untrusted".into())
        } else {
            Ok(())
        }
    }
}

pub const MAX_OPERATIONS: usize = 64;
pub const COMPLETED_LIFETIME_SECS: u64 = 3600;

pub fn specs() -> Vec<DynamicToolSpec> {
    let mut specs = original_specs::specs();
    // The host, rather than a replaceable frontend, now owns this same registry.
    for spec in &mut specs {
        if let DynamicToolSpec::Function(spec) = spec
            && spec.name == "get_worktree_creation_status"
        {
            spec.description = spec
                .description
                .replace(
                    "while this TUI session remains open",
                    "while this task's host remains open",
                )
                .replace("After restart,", "After host restart,");
        }
    }
    specs
}

pub fn supports(name: &str) -> bool {
    matches!(
        name,
        "create_worktree" | "get_worktree_creation_status" | "list_worktrees"
    )
}
pub fn read_only(name: &str) -> bool {
    matches!(name, "get_worktree_creation_status" | "list_worktrees")
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateArguments {
    #[serde(rename = "ref")]
    pub revision: Option<String>,
    #[serde(rename = "allowAsync", alias = "async")]
    asynchronous: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StatusArguments {
    operation_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListArguments {
    cursor: Option<String>,
}

pub enum Operation {
    Create(CreateArguments),
    Status(String),
    List(Option<String>),
}
pub fn parse(tool: &str, arguments: Value) -> Result<Operation, String> {
    if arguments.to_string().len() > 1024 {
        return Err("Worktree arguments are too large".into());
    }
    match tool {
        "create_worktree" => {
            let args: CreateArguments =
                serde_json::from_value(arguments).map_err(|error| error.to_string())?;
            if !args.asynchronous
                || args
                    .revision
                    .as_ref()
                    .is_some_and(|value| value.len() > 128)
            {
                return Err("Use allowAsync: true and a revision of at most 128 bytes".into());
            }
            Ok(Operation::Create(args))
        }
        "get_worktree_creation_status" => {
            let args: StatusArguments =
                serde_json::from_value(arguments).map_err(|error| error.to_string())?;
            Ok(Operation::Status(args.operation_id))
        }
        "list_worktrees" => {
            let args: ListArguments =
                serde_json::from_value(arguments).map_err(|error| error.to_string())?;
            Ok(Operation::List(args.cursor))
        }
        _ => Err("Unknown managed worktree tool".into()),
    }
}

/// Typed output of the unchanged attachment validation/store body. The original
/// client route still sends its response before publishing this notification.
pub struct AttachmentAdded {
    pub response: ThreadAttachmentAddResponse,
    pub notification: Option<ThreadAttachmentUpdatedNotification>,
}

#[derive(Default)]
pub struct AllocationCapture(Mutex<Option<Arc<ManagedWorktree>>>);
impl AllocationCapture {
    pub fn created(&self) -> Option<Arc<ManagedWorktree>> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

pub struct Allocation {
    manager: WorktreeManager,
    source_cwd: PathBuf,
    revision: Option<String>,
    owner: String,
}
pub struct Allocated {
    pub checkout: Arc<ManagedWorktree>,
    pub registration: Result<(), String>,
}
impl Allocation {
    pub fn prepare(
        codex_home: &std::path::Path,
        desktop: Option<&HashMap<String, Value>>,
        source_cwd: PathBuf,
        arguments: CreateArguments,
        owner: String,
    ) -> Result<Self, String> {
        let manager = WorktreeManager::new(
            WorktreeSettings::for_cli(codex_home, desktop).map_err(|error| error.to_string())?,
        );
        Ok(Self {
            manager,
            source_cwd,
            revision: arguments.revision,
            owner,
        })
    }
    /// Run only inside the actual host's tracked blocking task. Capture the
    /// exact checkout immediately after create returns, before registration.
    pub fn run(self, capture: &AllocationCapture) -> Result<Allocated, String> {
        let base = match self.revision {
            Some(base) => base,
            None => codex_worktree::default_worktree_base(&self.source_cwd)
                .map_err(|error| error.to_string())?,
        };
        let checkout = Arc::new(
            self.manager
                .create(&CreateWorktree {
                    source_cwd: self.source_cwd,
                    base: Some(base),
                })
                .map_err(|error| error.to_string())?,
        );
        *capture.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(checkout.clone());
        let registration = self
            .manager
            .bind_thread(&checkout.root, &self.owner)
            .map_err(|error| error.to_string());
        Ok(Allocated {
            checkout,
            registration,
        })
    }
}

pub fn payload(checkout: &ManagedWorktree) -> Value {
    json!({"root":checkout.root,"workspaceRoot":checkout.cwd,"sourceCwd":checkout.source_cwd})
}
pub fn creation_result(checkout: &ManagedWorktree, registered: bool) -> Value {
    json!({"type":if registered {"created"} else {"registrationFailed"},"worktree":payload(checkout),
        "message":if registered {"Keep the task cwd. Use the returned workspaceRoot explicitly and request filesystem permissions if needed."}
            else {"Checkout retained; registration failed. Do not create a duplicate. Inspect the worktree browser to recover."}})
}
pub fn pending(operation_id: &str, initial: bool) -> Value {
    if initial {
        json!({"type":"pending","operationId":operation_id,"message":"Creation is running. Poll status; do not repeat creation. Work on independent tasks while waiting."})
    } else {
        json!({"type":"pending","operationId":operation_id})
    }
}
pub fn list_result(page: ThreadAttachmentListResponse) -> Value {
    let data = page.data.into_iter().filter(|item| matches!(item.attachment_type.as_str(), "worktree" | "archived_worktree"))
        .map(|item| {
            let worktree = if item.attachment_type == "archived_worktree" { &item.payload["worktree"] } else { &item.payload };
            let entry = json!({"id":item.id,"type":item.attachment_type,"root":worktree["root"],"workspaceRoot":worktree["workspaceRoot"],"sourceCwd":worktree["sourceCwd"]});
            if entry.to_string().len() > 650 {
                json!({"id":item.id,"type":item.attachment_type,"truncated":true,"message":"Inspect this attachment in the worktree browser for its full paths."})
            } else { entry }
        }).collect::<Vec<_>>();
    json!({"data":data,"nextCursor":page.next_cursor})
}
pub struct Rendered {
    pub text: String,
    pub success: bool,
}
pub fn render(result: Result<Value, String>) -> Rendered {
    let success = result.is_ok();
    let text = match result {
        Ok(value) => value.to_string(),
        Err(message) => json!({"error":message}).to_string(),
    };
    if text.len() > 999 {
        Rendered { text: "Worktree result exceeds the display limit. Use the worktree browser to inspect retained checkouts; do not repeat creation.".into(), success: false }
    } else {
        Rendered { text, success }
    }
}

mod startup_binding;
pub use startup_binding::StartupBinding;
