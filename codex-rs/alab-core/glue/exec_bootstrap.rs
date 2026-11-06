// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Startup metadata sealed inside the native original-turn checkpoint.
//! Paths alone grant no access: recovery first loads the native grant, then
//! checks the existing checkout and its original thread owner.

#[cfg(unix)]
use codex_core::ExecRecoveryOptions;
#[cfg(unix)]
use codex_worktree::DEFAULT_WORKTREE_KEEP_COUNT;
use codex_worktree::WorktreeManager;
#[cfg(unix)]
use codex_worktree::WorktreeSettings;
use serde::Deserialize;
use serde::Serialize;
use std::path::Path;
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorktreeSnapshot {
    pool: PathBuf,
    checkout: PathBuf,
    source_cwd: PathBuf,
    cwd: PathBuf,
}

pub struct RecoveredWorktree {
    pub manager: WorktreeManager,
    pub checkout: PathBuf,
    pub source_cwd: PathBuf,
    pub cwd: PathBuf,
}

impl WorktreeSnapshot {
    pub fn capture(
        manager: &WorktreeManager,
        checkout: &Path,
        source_cwd: &Path,
        cwd: &Path,
    ) -> Self {
        Self {
            pool: manager.settings().root.clone(),
            checkout: checkout.to_owned(),
            source_cwd: source_cwd.to_owned(),
            cwd: cwd.to_owned(),
        }
    }

    #[cfg(unix)]
    fn reopen(&self, thread: &str) -> anyhow::Result<RecoveredWorktree> {
        anyhow::ensure!(
            [&self.pool, &self.checkout, &self.source_cwd, &self.cwd]
                .into_iter()
                .all(|path| path.is_absolute()),
            "native worktree checkpoint contains a relative path"
        );
        // Recovery reopens an existing checkout. It neither allocates nor
        // cleans one, and never imports mutable Desktop cleanup preferences.
        let manager = WorktreeManager::new(WorktreeSettings {
            root: self.pool.clone(),
            auto_cleanup_enabled: false,
            keep_count: DEFAULT_WORKTREE_KEEP_COUNT,
        });
        let existing = manager
            .list(&self.source_cwd)?
            .into_iter()
            .find(|candidate| candidate.root == self.checkout)
            .ok_or_else(|| anyhow::anyhow!("original managed worktree is unavailable"))?;
        anyhow::ensure!(
            existing.cwd == self.cwd && existing.source_cwd == self.source_cwd,
            "original managed worktree location changed"
        );
        anyhow::ensure!(
            manager.owner(&self.checkout)?.as_deref() == Some(thread),
            "original managed worktree belongs to another thread"
        );
        Ok(RecoveredWorktree {
            manager,
            checkout: existing.root,
            source_cwd: existing.source_cwd,
            cwd: existing.cwd,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg(unix)]
pub(super) struct Checkpoint {
    version: u32,
    thread: String,
    turn: String,
    pub options: ExecRecoveryOptions,
    worktree: Option<WorktreeSnapshot>,
}

#[cfg(unix)]
impl Checkpoint {
    pub fn new(options: ExecRecoveryOptions, worktree: Option<WorktreeSnapshot>) -> Self {
        Self {
            version: 1,
            thread: options.thread().to_string(),
            turn: options.turn().to_owned(),
            options,
            worktree,
        }
    }

    pub fn decode(value: serde_json::Value, thread: &str, turn: &str) -> anyhow::Result<Self> {
        let checkpoint: Self = serde_json::from_value(value)?;
        anyhow::ensure!(
            checkpoint.version == 1
                && checkpoint.thread == thread
                && checkpoint.turn == turn
                && checkpoint.options.thread().to_string() == thread
                && checkpoint.options.turn() == turn,
            "native exec checkpoint changed the original turn"
        );
        Ok(checkpoint)
    }

    pub fn worktree(&self, requested: bool) -> anyhow::Result<Option<RecoveredWorktree>> {
        anyhow::ensure!(
            requested == self.worktree.is_some(),
            "native exec checkpoint changed managed-worktree mode"
        );
        self.worktree
            .as_ref()
            .map(|worktree| worktree.reopen(&self.thread))
            .transpose()
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn git(cwd: &Path, args: &[&str]) -> anyhow::Result<()> {
        let result = std::process::Command::new("git")
            .current_dir(cwd)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .args([
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
            ])
            .args(args)
            .output()?;
        anyhow::ensure!(result.status.success(), "fixture git failed: {args:?}");
        Ok(())
    }

    #[test]
    fn reopens_only_the_original_owned_checkout_without_losing_edits() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let root = std::fs::canonicalize(temp.path())?;
        let source = root.join("source");
        std::fs::create_dir_all(source.join("nested"))?;
        std::fs::write(source.join("tracked"), "initial")?;
        std::fs::write(source.join("nested/file"), "nested")?;
        git(&source, &["init", "--quiet"])?;
        git(&source, &["add", "."])?;
        git(
            &source,
            &["commit", "--quiet", "--no-gpg-sign", "-m", "fixture"],
        )?;
        let manager = WorktreeManager::new(WorktreeSettings {
            root: root.join("managed"),
            auto_cleanup_enabled: false,
            keep_count: DEFAULT_WORKTREE_KEEP_COUNT,
        });
        let checkout = manager.create(&codex_worktree::CreateWorktree {
            source_cwd: source.join("nested"),
            base: None,
        })?;
        manager.bind_thread(&checkout.root, "original")?;
        let snapshot = WorktreeSnapshot::capture(
            &manager,
            &checkout.root,
            &checkout.source_cwd,
            &checkout.cwd,
        );
        std::fs::write(checkout.root.join("tracked"), "unsaved task edits")?;
        std::fs::write(checkout.cwd.join("untracked"), "unfinished work")?;
        assert!(snapshot.reopen("another-thread").is_err());
        let recovered = snapshot.reopen("original")?;
        assert_eq!(recovered.checkout, checkout.root);
        assert_eq!(recovered.cwd, checkout.cwd);
        assert_eq!(
            std::fs::read_to_string(recovered.checkout.join("tracked"))?,
            "unsaved task edits"
        );
        assert_eq!(
            std::fs::read_to_string(recovered.cwd.join("untracked"))?,
            "unfinished work"
        );
        assert_eq!(manager.list(&source)?.len(), 1);
        let mut wrong = snapshot;
        wrong.cwd = checkout.root;
        assert!(wrong.reopen("original").is_err());
        Ok(())
    }

    #[test]
    fn checkpoint_cannot_change_the_original_turn_or_execution_mode() -> anyhow::Result<()> {
        let thread = codex_protocol::ThreadId::new();
        let options = serde_json::from_value(serde_json::json!({
            "thread": thread, "turn": "original-turn", "schema": null, "review": false,
            "service_tier": null, "cyber_access_program": null,
            "step": {
                "collaboration_mode": {"mode": "default", "settings": {
                    "model": "fixture", "reasoning_effort": null, "developer_instructions": null
                }},
                "summary": "auto", "approval_policy": "never",
                "approvals_reviewer": "user", "disabled_plugin_ids": []
            }
        }))?;
        let value = serde_json::to_value(Checkpoint::new(options, None))?;
        let checkpoint = Checkpoint::decode(value.clone(), &thread.to_string(), "original-turn")?;
        assert!(checkpoint.worktree(false)?.is_none());
        assert!(checkpoint.worktree(true).is_err());
        assert!(Checkpoint::decode(value.clone(), &thread.to_string(), "another-turn").is_err());
        assert!(
            Checkpoint::decode(
                value,
                &codex_protocol::ThreadId::new().to_string(),
                "original-turn"
            )
            .is_err()
        );
        Ok(())
    }
}
