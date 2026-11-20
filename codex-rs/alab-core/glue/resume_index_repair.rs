// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::visit_rollout_filenames_with_policy;
use crate::ARCHIVED_SESSIONS_SUBDIR;
use crate::SESSIONS_SUBDIR;
use crate::plain_rollout_path;
use codex_protocol::ThreadId;
use std::collections::HashMap;
use std::io;
use std::ops::ControlFlow;
use std::path::Path;
use std::path::PathBuf;

/// Find missing rows without a filesystem page limit. Existing empty rows can only be
/// repaired from SQLite's selected rollout (including its matching compressed sibling).
pub async fn local_resume_rollouts(
    home: &Path,
    archived: bool,
    indexed: &HashMap<String, Option<PathBuf>>,
) -> io::Result<Vec<(ThreadId, PathBuf)>> {
    let root = home.join(if archived {
        ARCHIVED_SESSIONS_SUBDIR
    } else {
        SESSIONS_SUBDIR
    });
    match tokio::fs::symlink_metadata(&root).await {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => return Ok(Vec::new()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    }
    let mut newest = HashMap::new();
    visit_rollout_filenames_with_policy::<()>(
        &root,
        None,
        |path, error| denied_child(&root, path, error),
        |file| !matches!(indexed.get(&file.thread_id().to_string()), Some(None)),
        |file, path| {
            let id = file.thread_id();
            if let Some(Some(selected)) = indexed.get(&id.to_string())
                && plain_rollout_path(selected) != plain_rollout_path(&path)
            {
                return ControlFlow::Continue(());
            }
            // Match explicit UUID restore for a missing row.
            let key = (file.timestamp(), file.rollout_id().to_string());
            if newest.get(&id).is_none_or(|(previous, _)| key > *previous) {
                newest.insert(id, (key, path));
            }
            ControlFlow::Continue(())
        },
    )
    .await?;
    let mut candidates = newest
        .into_iter()
        .map(|(id, (_, path))| (id, path))
        .collect::<Vec<_>>();
    candidates.sort_unstable_by(|(_, left), (_, right)| left.cmp(right));
    Ok(candidates)
}

fn denied_child(root: &Path, path: &Path, error: &io::Error) -> bool {
    if path != root && error.kind() == io::ErrorKind::PermissionDenied {
        tracing::debug!(%error, path = %path.display(), "skipping unreadable local session history");
        true
    } else {
        false
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::fs::symlink;

    #[tokio::test]
    async fn discovery_skips_denied_children_but_keeps_root_and_uuid_lookup_strict()
    -> io::Result<()> {
        let home = tempfile::tempdir()?;
        let sessions = home.path().join(SESSIONS_SUBDIR);
        let readable = sessions.join("2025/01/01");
        let denied = sessions.join("2024");
        let outside = home.path().join("outside");
        for dir in [&readable, &denied, &outside] {
            std::fs::create_dir_all(dir)?;
        }
        fn rollout(dir: &Path, id: ThreadId) -> io::Result<PathBuf> {
            let path = dir.join(format!("rollout-2025-01-01T03-00-00-{id}.jsonl"));
            std::fs::write(&path, b"")?;
            Ok(path)
        }
        let indexed = ThreadId::from_u128(1);
        let restored = ThreadId::from_u128(2);
        rollout(&readable, indexed)?;
        let expected = rollout(&readable, restored)?;
        let hidden = ThreadId::from_u128(3);
        let hidden_path = rollout(&denied, hidden)?;
        rollout(&outside, ThreadId::from_u128(4))?;
        symlink(&outside, sessions.join("shortcut"))?;
        let known = HashMap::from([(indexed.to_string(), None)]);

        let permission = io::Error::from(io::ErrorKind::PermissionDenied);
        assert!(denied_child(&sessions, &denied, &permission));
        assert!(!denied_child(&sessions, &sessions, &permission));
        assert!(!denied_child(
            &sessions,
            &denied,
            &io::Error::other("read failure")
        ));

        let permissions = std::fs::metadata(&denied)?.permissions();
        std::fs::set_permissions(&denied, std::fs::Permissions::from_mode(0))?;
        let denied_read = std::fs::read_dir(&denied);
        let discovered = local_resume_rollouts(home.path(), false, &known).await;
        let strict =
            super::super::find_thread_path_by_id_from_filenames(&sessions, &restored.to_string())
                .await;
        std::fs::set_permissions(&denied, permissions)?;
        if denied_read.is_ok() {
            // A privileged test runner may read mode 000. The external symlink must still be skipped.
            assert_eq!(strict?, Some(expected.clone()));
            assert_eq!(
                discovered?,
                vec![(hidden, hidden_path), (restored, expected)]
            );
            return Ok(());
        }
        assert_eq!(
            denied_read.unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(discovered?, vec![(restored, expected)]);
        assert_eq!(strict.unwrap_err().kind(), io::ErrorKind::PermissionDenied);

        let permissions = std::fs::metadata(&sessions)?.permissions();
        std::fs::set_permissions(&sessions, std::fs::Permissions::from_mode(0))?;
        let unavailable = local_resume_rollouts(home.path(), false, &known).await;
        std::fs::set_permissions(&sessions, permissions)?;
        assert_eq!(
            unavailable.unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        Ok(())
    }
}
