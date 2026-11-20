// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Keep provisional snapshot files owned across cancellation and validation.

use std::io::Write;

use anyhow::Context;
use anyhow::Result;
use codex_utils_absolute_path::AbsolutePathBuf;
use tempfile::TempPath;

pub(super) async fn write(path: &AbsolutePathBuf, contents: String) -> Result<TempPath> {
    let path = path.clone();
    tokio::task::spawn_blocking(move || {
        let parent = path.as_path().parent().context("snapshot has no parent")?;
        let name = path.as_path().file_name().context("snapshot has no name")?;
        std::fs::create_dir_all(parent)?;
        // The caller supplied the generation name. Builder uses exclusive creation
        // and owner-only permissions, even with no additional random characters.
        let mut file = tempfile::Builder::new()
            .prefix(name)
            .rand_bytes(0)
            .tempfile_in(parent)
            .with_context(|| format!("create shell snapshot at {}", path.display()))?;
        file.write_all(contents.as_bytes())
            .with_context(|| format!("write shell snapshot at {}", path.display()))?;
        Ok(file.into_temp_path())
    })
    .await?
}

// Existing capture tests request a specific output path, including replacement
// of a previous capture. Exercise the same writer and credential handling there.
#[cfg(test)]
pub(super) async fn write_shell_snapshot(
    shell: &super::Shell,
    output_path: &AbsolutePathBuf,
    cwd: &AbsolutePathBuf,
    shell_environment_policy: &super::ShellEnvironmentPolicy,
    credential_broker: Option<&super::SnapshotCredentialBroker>,
    sandbox: Option<&super::ShellSnapshotSandbox>,
) -> Result<Option<super::SnapshotCredentials>> {
    let parent = output_path.parent().context("snapshot has no parent")?;
    let temp_path = parent.join(format!("test.tmp-{}", uuid::Uuid::new_v4()));
    let (staged, credentials) = super::stage_shell_snapshot(
        shell,
        &temp_path,
        cwd,
        shell_environment_policy,
        credential_broker,
        sandbox,
    )
    .await?;
    staged.persist(output_path)?;
    Ok(credentials)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn staging_and_publish_conflicts_preserve_the_existing_generation() -> Result<()> {
        let scratch = tempfile::tempdir()?;
        let dir = AbsolutePathBuf::from_absolute_path(scratch.path())?;
        let staged_path = dir.join("thread.tmp-1");
        let final_path = dir.join("thread.1.sh");
        let owner = write(&staged_path, "first".to_owned()).await?;

        let collision = write(&staged_path, "second".to_owned()).await.unwrap_err();
        assert_eq!(
            collision
                .downcast_ref::<std::io::Error>()
                .map(std::io::Error::kind),
            Some(std::io::ErrorKind::AlreadyExists)
        );
        assert_eq!(std::fs::read(&staged_path)?, b"first");

        std::fs::write(&final_path, "published")?;
        let refused = owner.persist_noclobber(&final_path).unwrap_err();
        assert_eq!(refused.error.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&staged_path)?, b"first");
        drop(refused);
        assert!(!staged_path.try_exists()?);
        assert_eq!(std::fs::read(&final_path)?, b"published");
        Ok(())
    }
}
