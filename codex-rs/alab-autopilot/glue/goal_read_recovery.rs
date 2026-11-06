// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Compiled in codex-state, where SQLite's typed error chain is already available.

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
