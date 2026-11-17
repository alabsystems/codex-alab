// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Compiled in codex-state, where SQLite's typed error chain is already available.

#[derive(Debug)]
pub struct MutationOutcomeUnknown;

impl std::fmt::Display for MutationOutcomeUnknown {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("goal mutation committed before its result read failed")
    }
}

impl std::error::Error for MutationOutcomeUnknown {}

/// Tag a read which occurs after an autocommit mutation. Its underlying typed
/// error may ordinarily prove a command never ran, but this command already did.
pub fn after_mutation_commit(error: anyhow::Error) -> anyhow::Error {
    error.context(MutationOutcomeUnknown)
}

/// Only transient contention or interrupted reads authorize another admission read.
/// Display text is never used to classify a refusal, corruption, or credential error.
pub fn retryable(error: &anyhow::Error) -> bool {
    for source in error.chain() {
        if let Some(error) = source.downcast_ref::<std::io::Error>() {
            return matches!(
                error.kind(),
                std::io::ErrorKind::Interrupted
                    | std::io::ErrorKind::WouldBlock
                    | std::io::ErrorKind::TimedOut
            );
        }
        if let Some(error) = source.downcast_ref::<libsqlite3_sys::Error>() {
            return matches!(
                error.code,
                libsqlite3_sys::ErrorCode::DatabaseBusy | libsqlite3_sys::ErrorCode::DatabaseLocked
            );
        }
        let Some(error) = source.downcast_ref::<sqlx::Error>() else {
            continue;
        };
        return match error {
            sqlx::Error::PoolTimedOut => true,
            sqlx::Error::Io(error) => matches!(
                error.kind(),
                std::io::ErrorKind::Interrupted
                    | std::io::ErrorKind::WouldBlock
                    | std::io::ErrorKind::TimedOut
            ),
            sqlx::Error::Database(database) => database
                .code()
                .and_then(|code| code.parse::<i32>().ok())
                .is_some_and(|code| matches!(code & 0xff, 5 | 6)),
            _ => false,
        };
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_typed_transient_goal_reads_retry() {
        assert!(retryable(&anyhow::Error::from(sqlx::Error::PoolTimedOut)));
        for code in [5, 6, 5 | (2 << 8)] {
            assert!(retryable(&anyhow::Error::from(libsqlite3_sys::Error::new(
                code
            ))));
        }
        for code in [1, 8, 10, 11, 13, 14, 26] {
            assert!(!retryable(&anyhow::Error::from(
                libsqlite3_sys::Error::new(code)
            )));
        }
        assert!(!retryable(&anyhow::Error::from(sqlx::Error::PoolClosed)));
        assert!(!retryable(&anyhow::Error::from(sqlx::Error::Decode(
            Box::new(std::io::Error::from(std::io::ErrorKind::Interrupted))
        ))));
        assert!(!retryable(&anyhow::Error::from(
            sqlx::Error::ColumnDecode {
                index: "goal_id".into(),
                source: Box::new(std::io::Error::from(std::io::ErrorKind::Interrupted)),
            }
        )));
        assert!(!retryable(&anyhow::Error::from(sqlx::Error::Tls(
            Box::new(std::io::Error::from(std::io::ErrorKind::Interrupted))
        ))));
        assert!(retryable(
            &anyhow::Error::from(sqlx::Error::PoolTimedOut).context("goal lookup")
        ));
        assert!(!retryable(&anyhow::anyhow!(
            "database is locked; try again"
        )));
        assert!(!retryable(&anyhow::Error::from(std::io::Error::from(
            std::io::ErrorKind::PermissionDenied
        ))));
        assert!(!retryable(&anyhow::Error::from(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            std::io::Error::from(std::io::ErrorKind::Interrupted),
        ))));
        assert!(retryable(
            &anyhow::Error::from(std::io::Error::from(std::io::ErrorKind::Interrupted))
                .context("goal read")
        ));
    }
}

/// Failures which prove an additive SQLite transaction did not commit. Generic
/// I/O errors, cancelled acknowledgements and display strings confer no replay.
pub fn write_definitely_uncommitted(error: &anyhow::Error) -> bool {
    for source in error.chain() {
        if let Some(error) = source.downcast_ref::<libsqlite3_sys::Error>() {
            return matches!(
                error.code,
                libsqlite3_sys::ErrorCode::DatabaseBusy
                    | libsqlite3_sys::ErrorCode::DatabaseLocked
                    | libsqlite3_sys::ErrorCode::DiskFull
            );
        }
        if let Some(error) = source.downcast_ref::<sqlx::Error>() {
            return match error {
                sqlx::Error::PoolTimedOut
                | sqlx::Error::PoolClosed
                | sqlx::Error::InvalidSavePointStatement => true,
                sqlx::Error::Database(database) => database
                    .code()
                    .and_then(|code| code.parse::<i32>().ok())
                    .is_some_and(|code| matches!(code & 0xff, 5 | 6 | 13)),
                _ => false,
            };
        }
    }
    false
}

/// A failed single goal-mutation transaction also proves rollback for SQLite
/// constraint/RAISE(ABORT) errors. Additive callers keep their narrower replay
/// classification because they may have broader transaction context.
pub fn mutation_definitely_uncommitted(error: &anyhow::Error) -> bool {
    if error.is::<MutationOutcomeUnknown>() {
        return false;
    }
    if write_definitely_uncommitted(error) {
        return true;
    }
    error.chain().any(|source| {
        if let Some(error) = source.downcast_ref::<libsqlite3_sys::Error>() {
            return error.code == libsqlite3_sys::ErrorCode::ConstraintViolation;
        }
        source
            .downcast_ref::<sqlx::Error>()
            .and_then(|error| match error {
                sqlx::Error::Database(database) => database.code(),
                _ => None,
            })
            .and_then(|code| code.parse::<i32>().ok())
            .is_some_and(|code| code & 0xff == 19)
    })
}

#[cfg(test)]
mod write_tests {
    use super::*;

    #[test]
    fn additive_replay_requires_typed_noncommit_evidence() {
        for code in [5, 6, 13, 5 | (2 << 8)] {
            assert!(write_definitely_uncommitted(&anyhow::Error::from(
                libsqlite3_sys::Error::new(code)
            )));
        }
        for code in [1, 8, 10, 11, 14, 26] {
            assert!(!write_definitely_uncommitted(&anyhow::Error::from(
                libsqlite3_sys::Error::new(code)
            )));
        }
        assert!(write_definitely_uncommitted(&anyhow::Error::from(
            sqlx::Error::InvalidSavePointStatement
        )));
        assert!(!write_definitely_uncommitted(&anyhow::anyhow!("disk full")));
        assert!(!write_definitely_uncommitted(&anyhow::Error::from(
            sqlx::Error::Io(std::io::Error::from(std::io::ErrorKind::Other))
        )));
        assert!(mutation_definitely_uncommitted(&anyhow::Error::from(
            libsqlite3_sys::Error::new(19)
        )));
        assert!(!write_definitely_uncommitted(&anyhow::Error::from(
            libsqlite3_sys::Error::new(19)
        )));
        let after_commit =
            after_mutation_commit(anyhow::Error::from(libsqlite3_sys::Error::new(5)));
        assert!(!mutation_definitely_uncommitted(&after_commit));
    }
}
