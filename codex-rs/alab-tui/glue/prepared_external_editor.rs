// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Shared original editor preparation. Retained SDK receives the same actual
//! Command and TempPath; it does not rebuild argv or acquire a document by path.
//! Compiled as a child of the original external_editor policy module.
use super::*;

pub(crate) struct PreparedEditorCommand {
    command: Command,
    document: tempfile::TempPath,
}

pub(crate) fn prepare(
    seed: &str,
    editor_cmd: &[String],
    codex_home: &Path,
    file_system_policy: &FileSystemSandboxPolicy,
    cwd: &Path,
) -> Result<PreparedEditorCommand> {
    if editor_cmd.is_empty() {
        return Err(Report::msg("editor command is empty"));
    }
    let default_codex_home = dirs::home_dir().map(|home| home.join(".codex"));
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    let project_codex_home = cwd.join(".codex");
    let mut candidate_homes = vec![codex_home];
    if let Some(default_codex_home) = default_codex_home.as_deref() {
        candidate_homes.push(default_codex_home);
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    candidate_homes.push(&project_codex_home);
    // SAME original fallible permission/symlink policy, not a copied fallback.
    let editor_directory = editor_directory(&candidate_homes, file_system_policy, cwd)?;
    let document = Builder::new()
        .suffix(".md")
        .tempfile_in(editor_directory)?
        .into_temp_path();
    fs::write(&document, seed)?;
    let mut command = {
        #[cfg(windows)]
        {
            Command::new(resolve_windows_program(&editor_cmd[0]))
        }
        #[cfg(not(windows))]
        {
            Command::new(&editor_cmd[0])
        }
    };
    if editor_cmd.len() > 1 {
        command.args(&editor_cmd[1..]);
    }
    command
        .arg(&document)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    // Preserve original inherited cwd/env; cwd above is the original policy
    // input, not a newly imposed Command::current_dir override.
    Ok(PreparedEditorCommand { command, document })
}

impl PreparedEditorCommand {
    /// Actual resources move once into the SDK accepted producer, before await.
    /// A pathname is never returned as a replacement document/reap capability.
    pub(crate) fn into_original_parts(self) -> (Command, tempfile::TempPath) {
        (self.command, self.document)
    }
    pub(crate) async fn run_original(mut self) -> Result<String> {
        let status = self.command.status().await?;
        if !status.success() {
            return Err(Report::msg(format!("editor exited with status {status}")));
        }
        Ok(fs::read_to_string(&self.document)?)
    }
}

#[cfg(test)]
#[path = "prepared_external_editor_tests.rs"]
mod tests;
