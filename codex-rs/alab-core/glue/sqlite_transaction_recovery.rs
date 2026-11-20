// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A failed SQLite write can roll back natively before SQLx observes rollback.
//! Never hand its still-tracked transaction to another borrower of the pool.

use sqlx::Connection;
use sqlx::Error;
use sqlx::SqliteConnection;

pub(super) async fn reusable(connection: &mut SqliteConnection) -> Result<bool, Error> {
    if connection.is_in_transaction() {
        // Dropping a normal transaction queues rollback on SQLx's worker. Drain
        // it before deciding whether this connection has a stranded depth.
        connection.ping().await?;
    }
    Ok(!connection.is_in_transaction())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::connection::JournalModePolicy;
    use crate::sqlite::connection::connect_once;
    use sqlx::SqlitePool;
    use sqlx::sqlite::SqliteConnectOptions;
    use sqlx::sqlite::SqliteJournalMode;
    use sqlx::sqlite::SqlitePoolOptions;
    use std::path::Path;

    async fn pool(path: &Path, protected: bool) -> SqlitePool {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Delete)
            .page_size(512);
        if protected {
            connect_once(&options, JournalModePolicy::InitializeDelete, false)
                .await
                .expect("open production pool")
        } else {
            SqlitePoolOptions::new()
                .max_connections(1)
                .connect_with(options)
                .await
                .expect("open unprotected SQLx pool")
        }
    }

    async fn inject_full(pool: &SqlitePool) {
        let mut connection = pool.acquire().await.expect("borrow connection");
        sqlx::raw_sql(
            "CREATE TABLE goal (objective TEXT, tokens_used INTEGER);\
             INSERT INTO goal VALUES ('preserve the objective', 11);\
             CREATE TABLE payload (body BLOB);\
             CREATE TEMP TABLE connection_marker (value INTEGER)",
        )
        .execute(&mut *connection)
        .await
        .expect("seed durable goal and connection identity");
        let pages: i64 = sqlx::query_scalar("PRAGMA page_count")
            .fetch_one(&mut *connection)
            .await
            .expect("measure tiny database");
        // PRAGMA does not accept a bound parameter; this value is a database-produced i64.
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "PRAGMA max_page_count={pages}"
        )))
        .execute(&mut *connection)
        .await
        .expect("inject bounded capacity, not filesystem exhaustion");
        let mut transaction = connection
            .begin_with("BEGIN IMMEDIATE")
            .await
            .expect("begin goal transaction");
        sqlx::query("UPDATE goal SET tokens_used = 29")
            .execute(&mut *transaction)
            .await
            .expect("uncommitted accounting");
        let error = sqlx::query("INSERT INTO payload VALUES (zeroblob(1048576))")
            .execute(&mut *transaction)
            .await
            .expect_err("SQLite must refuse to grow beyond its page limit");
        assert_eq!(
            error
                .as_database_error()
                .and_then(sqlx::error::DatabaseError::code),
            Some("13".into()),
            "the injection must exercise SQLITE_FULL"
        );
        drop(transaction);
        connection.ping().await.expect("drain queued rollback");
        assert!(
            connection.is_in_transaction(),
            "SQLx must retain the stale depth for this regression to be sensitive"
        );
        let error = sqlx::query("ROLLBACK")
            .execute(&mut *connection)
            .await
            .expect_err("SQLite already rolled the failed transaction back");
        assert!(error.to_string().contains("no transaction is active"));
        connection.return_to_pool().await;
    }

    #[tokio::test]
    async fn full_write_is_rolled_back_and_next_borrower_can_commit() {
        for protected in [false, true] {
            let directory = tempfile::tempdir().expect("scratch directory");
            let pool = pool(&directory.path().join("bounded.sqlite"), protected).await;
            inject_full(&pool).await;
            let mut connection = pool.acquire().await.expect("borrow after failed write");
            let goal: (String, i64) = sqlx::query_as("SELECT objective,tokens_used FROM goal")
                .fetch_one(&mut *connection)
                .await
                .expect("durable goal remains readable");
            assert_eq!(goal, ("preserve the objective".to_owned(), 11));
            let marker: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM sqlite_temp_schema WHERE name='connection_marker')",
            )
            .fetch_one(&mut *connection)
            .await
            .expect("observe physical connection reuse");
            assert_eq!(marker, !protected);
            sqlx::query("PRAGMA max_page_count=1024")
                .execute(&mut *connection)
                .await
                .expect("restore capacity");
            if protected {
                let mut transaction = connection
                    .begin_with("BEGIN IMMEDIATE")
                    .await
                    .expect("fresh transaction after storage recovery");
                sqlx::query("UPDATE goal SET tokens_used = tokens_used + 7")
                    .execute(&mut *transaction)
                    .await
                    .expect("account once");
                transaction
                    .commit()
                    .await
                    .expect("commit recovered accounting");
                assert_eq!(
                    sqlx::query_scalar::<_, i64>("SELECT tokens_used FROM goal")
                        .fetch_one(&mut *connection)
                        .await
                        .expect("read committed accounting"),
                    18
                );
            } else {
                assert!(matches!(
                    connection.begin_with("BEGIN IMMEDIATE").await,
                    Err(Error::InvalidSavePointStatement)
                ));
            }
            drop(connection);
            pool.close().await;
            assert!(
                std::fs::read_dir(directory.path())
                    .expect("list bounded test files")
                    .all(
                        |entry| entry.expect("entry").metadata().expect("metadata").len()
                            < 2 * 1024 * 1024
                    )
            );
        }
    }

    #[tokio::test]
    async fn ordinary_dropped_transactions_keep_a_reusable_connection() {
        let directory = tempfile::tempdir().expect("scratch directory");
        let pool = pool(&directory.path().join("normal.sqlite"), true).await;
        let mut connection = pool.acquire().await.expect("borrow connection");
        sqlx::query("CREATE TEMP TABLE connection_marker (value INTEGER)")
            .execute(&mut *connection)
            .await
            .expect("mark connection");
        let mut transaction = connection
            .begin_with("BEGIN IMMEDIATE")
            .await
            .expect("begin normal transaction");
        sqlx::query("INSERT INTO connection_marker VALUES (7)")
            .execute(&mut *transaction)
            .await
            .expect("uncommitted write");
        drop(transaction);
        connection.return_to_pool().await;
        let mut connection = pool.acquire().await.expect("borrow after normal rollback");
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM connection_marker")
                .fetch_one(&mut *connection)
                .await
                .expect("normal rollback must preserve the original connection"),
            0
        );
        assert!(!connection.is_in_transaction());
        drop(connection);
        pool.close().await;
    }
}
