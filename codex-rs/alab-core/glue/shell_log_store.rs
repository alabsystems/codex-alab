// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use std::io;
use std::path::Path;
use std::path::PathBuf;

pub(crate) use codex_alab_core::shell_log_store::LogDirectory;
use codex_protocol::ThreadId;
use codex_thread_store::ThreadStoreError;
use codex_thread_store::ThreadStoreResult;

pub(crate) fn directory(home: &Path, thread: ThreadId) -> io::Result<LogDirectory> {
    LogDirectory::open(home, &thread.to_string(), true)
}

pub(crate) fn existing_directory(
    home: &Path,
    thread: ThreadId,
) -> io::Result<Option<LogDirectory>> {
    match LogDirectory::open(home, &thread.to_string(), false) {
        Ok(directory) => Ok(Some(directory)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

pub(crate) fn delete(home: &Path, thread: ThreadId) -> io::Result<()> {
    if let Some(directory) = existing_directory(home, thread)? {
        directory.retire()?;
    }
    Ok(())
}

pub(crate) async fn delete_threads(home: PathBuf, threads: Vec<ThreadId>) -> ThreadStoreResult<()> {
    tokio::task::spawn_blocking(move || {
        for id in threads {
            delete(&home, id).map_err(|err| ThreadStoreError::Internal {
                message: format!("failed to retire retained shell output for thread {id}: {err}"),
            })?;
        }
        Ok(())
    })
    .await
    .map_err(|err| ThreadStoreError::Internal {
        message: format!("shell output retirement did not finish: {err}"),
    })?
}

#[cfg(test)]
#[path = "shell_log_store_tests.rs"]
mod tests;
