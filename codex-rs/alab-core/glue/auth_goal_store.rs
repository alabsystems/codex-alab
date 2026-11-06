// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Account-recovery provenance belongs to the goal transaction. Every later edit,
//! even a same-value edit, revokes it through SQLite triggers.

use super::*;

async fn schema(connection: &mut sqlx::SqliteConnection) -> anyhow::Result<()> {
    for statement in [
        "CREATE TABLE IF NOT EXISTS alab_goal_auth_walls (thread_id TEXT PRIMARY KEY, goal_id TEXT NOT NULL, turn_id TEXT NOT NULL, phase TEXT NOT NULL, accepted_turn TEXT, goal_status TEXT NOT NULL DEFAULT '')",
        "CREATE TABLE IF NOT EXISTS alab_goal_auth_captures (thread_id TEXT NOT NULL, turn_id TEXT NOT NULL, PRIMARY KEY(thread_id,turn_id))",
        "CREATE TRIGGER IF NOT EXISTS alab_goal_auth_capture_insert AFTER INSERT ON alab_goal_auth_walls BEGIN INSERT OR IGNORE INTO alab_goal_auth_captures(thread_id,turn_id) VALUES(NEW.thread_id,NEW.turn_id); END",
        "CREATE TRIGGER IF NOT EXISTS alab_goal_auth_capture_delete AFTER DELETE ON alab_goal_auth_walls BEGIN INSERT OR IGNORE INTO alab_goal_auth_captures(thread_id,turn_id) VALUES(OLD.thread_id,OLD.turn_id); END",
        "CREATE TABLE IF NOT EXISTS alab_goal_auth_deferrals (thread_id TEXT PRIMARY KEY, turn_id TEXT NOT NULL, goal_id TEXT NOT NULL)",
        "CREATE TRIGGER IF NOT EXISTS alab_goal_auth_deferral_clear AFTER DELETE ON thread_goal_continuation_deferrals BEGIN DELETE FROM alab_goal_auth_deferrals WHERE thread_id=OLD.thread_id; END",
        "CREATE TRIGGER IF NOT EXISTS alab_goal_auth_wall_update AFTER UPDATE ON thread_goals BEGIN DELETE FROM alab_goal_auth_walls WHERE thread_id=OLD.thread_id; END",
        "CREATE TRIGGER IF NOT EXISTS alab_goal_auth_wall_delete AFTER DELETE ON thread_goals BEGIN DELETE FROM alab_goal_auth_walls WHERE thread_id=OLD.thread_id; END",
        "CREATE TRIGGER IF NOT EXISTS alab_goal_auth_wall_insert AFTER INSERT ON thread_goals BEGIN DELETE FROM alab_goal_auth_walls WHERE thread_id=NEW.thread_id; END",
    ] {
        sqlx::query(statement).execute(&mut *connection).await?;
    }
    Ok(())
}

async fn snapshot(
    connection: &mut sqlx::SqliteConnection,
    thread: ThreadId,
) -> anyhow::Result<Option<crate::ThreadGoal>> {
    let row = sqlx::query("SELECT thread_id,goal_id,objective,status,token_budget,tokens_used,time_used_seconds,created_at_ms,updated_at_ms FROM thread_goals WHERE thread_id=?")
        .bind(thread.to_string()).fetch_optional(connection).await?;
    row.as_ref().map(thread_goal_from_row).transpose()
}

async fn defer_auth(
    connection: &mut sqlx::SqliteConnection,
    thread: ThreadId,
    wall: &str,
) -> anyhow::Result<()> {
    let inserted = sqlx::query(
        "INSERT OR IGNORE INTO thread_goal_continuation_deferrals(thread_id) SELECT ? WHERE EXISTS(SELECT 1 FROM thread_goals WHERE thread_id=?)",
    )
    .bind(thread.to_string())
    .bind(thread.to_string())
    .execute(&mut *connection)
    .await?
    .rows_affected();
    // A pre-existing unowned hold belongs to native goal scheduling, not to
    // this recovery. Never claim it merely because it uses the same row.
    let auth_owned: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM alab_goal_auth_deferrals WHERE thread_id=?)",
    )
    .bind(thread.to_string())
    .fetch_one(&mut *connection)
    .await?;
    if inserted == 0 && !auth_owned {
        return Ok(());
    }
    sqlx::query("INSERT OR REPLACE INTO alab_goal_auth_deferrals(thread_id,turn_id,goal_id) SELECT thread_id,turn_id,CASE WHEN phase='standalone' THEN '' ELSE goal_id END FROM alab_goal_auth_walls WHERE thread_id=? AND turn_id=?")
        .bind(thread.to_string())
        .bind(wall)
        .execute(connection)
        .await?;
    Ok(())
}

async fn auth_context(
    connection: &mut sqlx::SqliteConnection,
    thread: ThreadId,
    wall: &str,
) -> anyhow::Result<Option<Option<String>>> {
    let pending: Option<(String, String)> = sqlx::query_as("SELECT p.goal_id,p.phase FROM alab_goal_auth_walls p LEFT JOIN thread_goals g ON g.thread_id=p.thread_id WHERE p.thread_id=? AND p.turn_id=? AND ((p.phase='standalone' AND ((p.goal_id='' AND g.thread_id IS NULL) OR (g.goal_id=p.goal_id AND g.status=p.goal_status AND g.status!='active'))) OR (g.goal_id=p.goal_id AND ((p.phase='blocked' AND g.status='blocked') OR (p.phase='limited' AND g.status='usage_limited') OR (p.phase IN ('restored','accepted') AND g.status='active'))))")
            .bind(thread.to_string()).bind(wall).fetch_optional(connection).await?;
    Ok(pending.map(|(goal, phase)| (phase != "standalone").then_some(goal)))
}

impl GoalStore {
    /// Claim the deferral only for the current auth witness. A stale caller
    /// cannot replace a newer failed turn's ownership.
    pub async fn alab_defer_auth_recovery(
        &self,
        thread: ThreadId,
        wall: &str,
    ) -> anyhow::Result<()> {
        let mut tx = self.pool.begin_immediate().await?;
        schema(&mut tx).await?;
        let current: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM alab_goal_auth_walls WHERE thread_id=? AND turn_id=?)",
        )
        .bind(thread.to_string())
        .bind(wall)
        .fetch_one(&mut *tx)
        .await?;
        anyhow::ensure!(current, "auth recovery witness was replaced");
        defer_auth(&mut tx, thread, wall).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Retire only the named auth witness and its owned deferral. Ownership
    /// survives a human goal edit so obsolete custody can be released without
    /// changing the edited goal. A newer wall keeps both its proof and hold.
    pub async fn alab_retire_auth_recovery(
        &self,
        thread: ThreadId,
        wall: &str,
        expected_goal: Option<&str>,
    ) -> anyhow::Result<bool> {
        let mut tx = self.pool.begin_immediate().await?;
        schema(&mut tx).await?;
        let removed =
            sqlx::query("DELETE FROM alab_goal_auth_walls WHERE thread_id=? AND turn_id=? AND CASE WHEN phase='standalone' THEN '' ELSE goal_id END=?")
                .bind(thread.to_string())
                .bind(wall)
                .bind(expected_goal.unwrap_or(""))
                .execute(&mut *tx)
                .await?
                .rows_affected();
        let released = sqlx::query("DELETE FROM thread_goal_continuation_deferrals WHERE thread_id=? AND EXISTS(SELECT 1 FROM alab_goal_auth_deferrals a WHERE a.thread_id=thread_goal_continuation_deferrals.thread_id AND a.turn_id=? AND a.goal_id=?)")
            .bind(thread.to_string()).bind(wall).bind(expected_goal.unwrap_or("")).execute(&mut *tx).await?.rows_affected();
        tx.commit().await?;
        Ok(removed != 0 || released != 0)
    }
    /// Capture passive standalone provenance once for this actual native wall.
    /// This creates neither a goal nor a continuation hold. A human edit or
    /// superseding auth wall cannot be undone by a late retry of the capture.
    pub async fn alab_capture_auth_standalone(
        &self,
        thread: ThreadId,
        wall: &str,
    ) -> anyhow::Result<bool> {
        let mut tx = self.pool.begin_immediate().await?;
        schema(&mut tx).await?;
        let existing: Option<(String, String, String, String)> = sqlx::query_as(
            "SELECT goal_id,turn_id,phase,goal_status FROM alab_goal_auth_walls WHERE thread_id=?",
        )
        .bind(thread.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        let goal = snapshot(&mut tx, thread).await?;
        if goal.as_ref().is_some_and(|goal| {
            matches!(
                goal.status,
                crate::ThreadGoalStatus::Active | crate::ThreadGoalStatus::BudgetLimited
            )
        }) {
            return Ok(false);
        }
        if let Some((goal_id, prior_wall, phase, status)) = existing {
            let matches_goal = goal.as_ref().map_or(goal_id.is_empty(), |goal| {
                goal.goal_id == goal_id && goal.status.as_str() == status
            });
            if prior_wall != wall || phase != "standalone" || !matches_goal {
                return Ok(false);
            }
            sqlx::query(
                "INSERT OR IGNORE INTO alab_goal_auth_captures(thread_id,turn_id) VALUES(?,?)",
            )
            .bind(thread.to_string())
            .bind(wall)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            return Ok(true);
        }
        let captured: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM alab_goal_auth_captures WHERE thread_id=? AND turn_id=?)",
        )
        .bind(thread.to_string())
        .bind(wall)
        .fetch_one(&mut *tx)
        .await?;
        if captured {
            return Ok(false);
        }
        sqlx::query("INSERT INTO alab_goal_auth_captures(thread_id,turn_id) VALUES(?,?)")
            .bind(thread.to_string())
            .bind(wall)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO alab_goal_auth_walls(thread_id,goal_id,turn_id,phase,goal_status) VALUES(?,?,?,'standalone',?)")
            .bind(thread.to_string()).bind(goal.as_ref().map(|goal|goal.goal_id.as_str()).unwrap_or(""))
            .bind(wall).bind(goal.as_ref().map(|goal|goal.status.as_str()).unwrap_or(""))
            .execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(true)
    }

    pub async fn alab_record_auth_standalone(
        &self,
        thread: ThreadId,
        wall: &str,
    ) -> anyhow::Result<()> {
        let mut tx = self.pool.begin_immediate().await?;
        schema(&mut tx).await?;
        let goal = snapshot(&mut tx, thread).await?;
        if goal
            .as_ref()
            .is_some_and(|goal| goal.status == crate::ThreadGoalStatus::Active)
        {
            return Ok(());
        }
        let status = goal.as_ref().map(|goal| goal.status.as_str()).unwrap_or("");
        sqlx::query("INSERT OR REPLACE INTO alab_goal_auth_walls(thread_id,goal_id,turn_id,phase,goal_status) VALUES(?,?,?,'standalone',?)")
            .bind(thread.to_string()).bind(goal.as_ref().map(|goal| goal.goal_id.as_str()).unwrap_or("")).bind(wall).bind(status)
            .execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }
    pub async fn alab_update_for_auth_wall(
        &self,
        thread: ThreadId,
        update: GoalUpdate,
        wall: Option<&str>,
    ) -> anyhow::Result<Option<crate::ThreadGoal>> {
        let Some(wall) = wall else {
            return self.update_thread_goal(thread, update).await;
        };
        let (status, phase) = match update.status {
            Some(crate::ThreadGoalStatus::Blocked) => ("blocked", "blocked"),
            Some(crate::ThreadGoalStatus::UsageLimited) => ("usage_limited", "limited"),
            _ => anyhow::bail!("account recovery must stop at an authentication or quota wall"),
        };
        if update.objective.is_some() || update.token_budget.is_some() {
            anyhow::bail!("account recovery stop must preserve objective and budget");
        }
        let Some(goal_id) = update.expected_goal_id else {
            return Ok(None);
        };
        let mut tx = self.pool.begin_immediate().await?;
        schema(&mut tx).await?;
        let changed = sqlx::query("UPDATE thread_goals SET status=?,updated_at_ms=? WHERE thread_id=? AND goal_id=? AND status='active'")
            .bind(status).bind(chrono::Utc::now().timestamp_millis()).bind(thread.to_string()).bind(&goal_id)
            .execute(&mut *tx).await?.rows_affected();
        if changed == 0 {
            return Ok(None);
        }
        sqlx::query("INSERT OR REPLACE INTO alab_goal_auth_walls(thread_id,goal_id,turn_id,phase) VALUES(?,?,?,?)")
            .bind(thread.to_string()).bind(goal_id).bind(wall).bind(phase).execute(&mut *tx).await?;
        let goal = snapshot(&mut tx, thread).await?;
        tx.commit().await?;
        Ok(goal)
    }

    pub async fn alab_auth_goal_pending(
        &self,
        thread: ThreadId,
        wall: &str,
    ) -> anyhow::Result<bool> {
        Ok(self.alab_auth_goal_context(thread, wall).await?.is_some())
    }

    pub async fn alab_auth_goal_context(
        &self,
        thread: ThreadId,
        wall: &str,
    ) -> anyhow::Result<Option<Option<String>>> {
        let mut tx = self.pool.begin_immediate().await?;
        schema(&mut tx).await?;
        let pending = auth_context(&mut tx, thread, wall).await?;
        tx.commit().await?;
        Ok(pending)
    }

    /// An accepted native start supersedes older auth custody in this thread.
    /// The caller must own the host's serialized native lifecycle gate and
    /// supply its current accepted turn ID. This is not a capture fallback:
    /// stale recovery callbacks must never invoke it for an older native ID.
    pub async fn alab_native_auth_turn_started(
        &self,
        thread: ThreadId,
        current: &str,
    ) -> anyhow::Result<()> {
        let mut tx = self.pool.begin_immediate().await?;
        schema(&mut tx).await?;
        sqlx::query("DELETE FROM alab_goal_auth_walls WHERE thread_id=? AND turn_id!=?")
            .bind(thread.to_string())
            .bind(current)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM thread_goal_continuation_deferrals WHERE thread_id=? AND EXISTS(SELECT 1 FROM alab_goal_auth_deferrals a WHERE a.thread_id=thread_goal_continuation_deferrals.thread_id AND a.turn_id!=?)")
            .bind(thread.to_string()).bind(current).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Linearize the verified native admission with every goal writer, including
    /// another process. The caller keeps this read-only writer lease through
    /// only synchronous authority checks and native admission, then drops it
    /// before returning to core. It must do no network/persistence await here.
    pub async fn alab_auth_admission_lease(
        &self,
        thread: ThreadId,
        wall: &str,
        expected_goal: Option<&str>,
    ) -> anyhow::Result<Option<sqlx::Transaction<'static, sqlx::Sqlite>>> {
        let mut tx = self.pool.begin_immediate().await?;
        schema(&mut tx).await?;
        let context = auth_context(&mut tx, thread, wall).await?;
        if context.as_ref().map(|goal| goal.as_deref()) != Some(expected_goal) {
            return Ok(None);
        }
        Ok(Some(tx))
    }

    pub async fn alab_restore_auth_goal(
        &self,
        thread: ThreadId,
        wall: &str,
    ) -> anyhow::Result<Option<crate::ThreadGoal>> {
        self.alab_restore_auth_goal_with_deferral(thread, wall, true)
            .await
    }

    pub async fn alab_restore_auth_goal_with_deferral(
        &self,
        thread: ThreadId,
        wall: &str,
        durable_deferral: bool,
    ) -> anyhow::Result<Option<crate::ThreadGoal>> {
        let mut tx = self.pool.begin_immediate().await?;
        schema(&mut tx).await?;
        let proof: Option<(String, String)> = sqlx::query_as("SELECT p.goal_id,p.phase FROM alab_goal_auth_walls p JOIN thread_goals g ON g.thread_id=p.thread_id AND g.goal_id=p.goal_id WHERE p.thread_id=? AND p.turn_id=? AND ((p.phase='blocked' AND g.status='blocked') OR (p.phase='limited' AND g.status='usage_limited') OR (p.phase IN ('restored','accepted') AND g.status='active')) AND (g.token_budget IS NULL OR g.tokens_used < g.token_budget)")
            .bind(thread.to_string()).bind(wall).fetch_optional(&mut *tx).await?;
        let Some((goal_id, phase)) = proof else {
            return Ok(None);
        };
        // The native runtime sees this before it observes Active, so it cannot
        // create a second automatic goal turn beside the login recovery.
        if durable_deferral {
            defer_auth(&mut tx, thread, wall).await?;
        }
        if matches!(phase.as_str(), "blocked" | "limited") {
            sqlx::query("UPDATE thread_goals SET status='active',updated_at_ms=? WHERE thread_id=? AND goal_id=? AND status IN ('blocked','usage_limited')")
                .bind(chrono::Utc::now().timestamp_millis()).bind(thread.to_string()).bind(&goal_id)
                .execute(&mut *tx).await?;
            // This new witness supports retry after refused native admission.
            // The update trigger consumed the old blocked witness atomically.
            sqlx::query("INSERT INTO alab_goal_auth_walls(thread_id,goal_id,turn_id,phase) VALUES(?,?,?,'restored')")
                .bind(thread.to_string()).bind(goal_id).bind(wall).execute(&mut *tx).await?;
        }
        let goal = snapshot(&mut tx, thread).await?;
        tx.commit().await?;
        Ok(goal)
    }

    pub async fn alab_accept_auth_goal(
        &self,
        thread: ThreadId,
        wall: &str,
        turn: &str,
    ) -> anyhow::Result<bool> {
        let mut tx = self.pool.begin_immediate().await?;
        schema(&mut tx).await?;
        let changed = sqlx::query("UPDATE alab_goal_auth_walls SET phase='accepted',accepted_turn=? WHERE thread_id=? AND turn_id=? AND phase IN ('restored','accepted') AND (accepted_turn IS NULL OR accepted_turn=?) AND EXISTS(SELECT 1 FROM thread_goals g WHERE g.thread_id=alab_goal_auth_walls.thread_id AND g.goal_id=alab_goal_auth_walls.goal_id AND g.status='active' AND (g.token_budget IS NULL OR g.tokens_used<g.token_budget))")
            .bind(turn).bind(thread.to_string()).bind(wall).bind(turn).execute(&mut *tx).await?.rows_affected();
        tx.commit().await?;
        Ok(changed == 1)
    }

    /// Only the native active-turn accounting path supplies `turn`. All other
    /// goal writers keep the ordinary update trigger's invalidation semantics.
    pub async fn alab_account_auth_goal_usage(
        &self,
        thread: ThreadId,
        time: i64,
        tokens: i64,
        mode: GoalAccountingMode,
        expected: Option<&str>,
        turn: Option<&str>,
    ) -> anyhow::Result<GoalAccountingOutcome> {
        let Some(turn) = turn else {
            return self
                .account_thread_goal_usage(thread, time, tokens, mode, expected)
                .await;
        };
        let mut tx = self.pool.begin_immediate().await?;
        schema(&mut tx).await?;
        let proof: Option<(String, String)> = sqlx::query_as("SELECT p.goal_id,p.turn_id FROM alab_goal_auth_walls p JOIN thread_goals g ON g.thread_id=p.thread_id AND g.goal_id=p.goal_id WHERE p.thread_id=? AND p.phase='accepted' AND p.accepted_turn=? AND g.status='active'")
            .bind(thread.to_string()).bind(turn).fetch_optional(&mut *tx).await?;
        let Some((goal_id, wall)) = proof.filter(|(goal, _)| expected == Some(goal.as_str()))
        else {
            tx.rollback().await?;
            return self
                .account_thread_goal_usage(thread, time, tokens, mode, expected)
                .await;
        };
        let time = time.max(0);
        let tokens = tokens.max(0);
        if time == 0 && tokens == 0 {
            let goal = snapshot(&mut tx, thread).await?;
            tx.commit().await?;
            return Ok(GoalAccountingOutcome::Unchanged(goal));
        }
        sqlx::query("UPDATE thread_goals SET time_used_seconds=time_used_seconds+?,tokens_used=tokens_used+?,status=CASE WHEN token_budget IS NOT NULL AND tokens_used+?>=token_budget THEN 'budget_limited' ELSE status END,updated_at_ms=? WHERE thread_id=? AND goal_id=? AND status='active'")
            .bind(time).bind(tokens).bind(tokens).bind(chrono::Utc::now().timestamp_millis()).bind(thread.to_string()).bind(&goal_id)
            .execute(&mut *tx).await?;
        let goal = snapshot(&mut tx, thread)
            .await?
            .ok_or_else(|| anyhow::anyhow!("accounted auth goal vanished"))?;
        if goal.status == crate::ThreadGoalStatus::Active {
            sqlx::query("INSERT INTO alab_goal_auth_walls(thread_id,goal_id,turn_id,phase,accepted_turn) VALUES(?,?,?,'accepted',?)")
                .bind(thread.to_string()).bind(goal_id).bind(wall).bind(turn).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(GoalAccountingOutcome::Updated(goal))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[expect(
        clippy::disallowed_methods,
        reason = "test-only raw-schema fixture requires one private in-memory connection; the production shim opens file-backed pools with five connections"
    )]
    async fn store() -> (GoalStore, ThreadId) {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("fixture pool");
        store_with_pool(pool).await
    }

    async fn store_with_pool(pool: sqlx::SqlitePool) -> (GoalStore, ThreadId) {
        sqlx::query("CREATE TABLE thread_goals(thread_id TEXT PRIMARY KEY,goal_id TEXT NOT NULL,objective TEXT NOT NULL,status TEXT NOT NULL,token_budget INTEGER,tokens_used INTEGER NOT NULL,time_used_seconds INTEGER NOT NULL,created_at_ms INTEGER NOT NULL,updated_at_ms INTEGER NOT NULL)")
            .execute(&pool).await.expect("fixture goals");
        sqlx::query("PRAGMA foreign_keys=ON")
            .execute(&pool)
            .await
            .expect("enforce real goal foreign key");
        sqlx::query("CREATE TABLE thread_goal_continuation_deferrals(thread_id TEXT PRIMARY KEY NOT NULL REFERENCES thread_goals(thread_id) ON DELETE CASCADE)")
            .execute(&pool)
            .await
            .expect("fixture deferrals");
        let thread = ThreadId::from_string("00000000-0000-0000-0000-000000000123").expect("thread");
        sqlx::query(
            "INSERT INTO thread_goals VALUES(?,'goal','original','active',100,10,0,1000,1000)",
        )
        .bind(thread.to_string())
        .execute(&pool)
        .await
        .expect("fixture goal");
        (GoalStore::new(Arc::new(pool)), thread)
    }

    async fn wall(store: &GoalStore, thread: ThreadId) {
        store
            .alab_update_for_auth_wall(
                thread,
                GoalUpdate {
                    objective: None,
                    status: Some(crate::ThreadGoalStatus::Blocked),
                    token_budget: None,
                    expected_goal_id: Some("goal".into()),
                },
                Some("wall"),
            )
            .await
            .expect("native auth stop")
            .expect("blocked goal");
    }

    #[tokio::test]
    async fn quota_witness_restores_only_its_unchanged_goal_and_keeps_accounting() {
        for edit in [false, true] {
            let (store, thread) = store().await;
            let limited = store
                .alab_update_for_auth_wall(
                    thread,
                    GoalUpdate {
                        objective: None,
                        status: Some(crate::ThreadGoalStatus::UsageLimited),
                        token_budget: None,
                        expected_goal_id: Some("goal".into()),
                    },
                    Some("quota-turn"),
                )
                .await
                .unwrap()
                .unwrap();
            assert_eq!(limited.status, crate::ThreadGoalStatus::UsageLimited);
            assert_eq!(
                store
                    .alab_auth_goal_context(thread, "quota-turn")
                    .await
                    .unwrap(),
                Some(Some("goal".into()))
            );
            if edit {
                // Even an identical edit is the owner's newer decision.
                store
                    .update_thread_goal(
                        thread,
                        GoalUpdate {
                            objective: Some(limited.objective.clone()),
                            status: None,
                            token_budget: None,
                            expected_goal_id: Some("goal".into()),
                        },
                    )
                    .await
                    .unwrap();
                assert!(
                    store
                        .alab_restore_auth_goal(thread, "quota-turn")
                        .await
                        .unwrap()
                        .is_none()
                );
                continue;
            }
            let restored = store
                .alab_restore_auth_goal(thread, "quota-turn")
                .await
                .unwrap()
                .unwrap();
            assert_eq!(restored.status, crate::ThreadGoalStatus::Active);
            assert_eq!(restored.goal_id, limited.goal_id);
            assert_eq!(restored.objective, limited.objective);
            assert_eq!(restored.tokens_used, limited.tokens_used);
            assert_eq!(restored.token_budget, limited.token_budget);
            assert_eq!(restored.time_used_seconds, limited.time_used_seconds);
            assert!(
                store
                    .alab_accept_auth_goal(thread, "quota-turn", "quota-turn")
                    .await
                    .unwrap()
            );
            assert!(
                !store
                    .alab_accept_auth_goal(thread, "quota-turn", "another-turn")
                    .await
                    .unwrap()
            );
        }
    }

    #[tokio::test]
    async fn an_ordinary_usage_limited_goal_has_no_account_recovery_authority() {
        let (store, thread) = store().await;
        store
            .update_thread_goal(
                thread,
                GoalUpdate {
                    objective: None,
                    status: Some(crate::ThreadGoalStatus::UsageLimited),
                    token_budget: None,
                    expected_goal_id: Some("goal".into()),
                },
            )
            .await
            .unwrap();
        assert!(
            store
                .alab_restore_auth_goal(thread, "quota-turn")
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .alab_auth_goal_context(thread, "quota-turn")
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn accepted_new_native_turn_retires_only_older_auth_custody() {
        let (store, thread) = store().await;
        store
            .update_thread_goal(
                thread,
                GoalUpdate {
                    objective: None,
                    status: Some(crate::ThreadGoalStatus::Paused),
                    token_budget: None,
                    expected_goal_id: Some("goal".into()),
                },
            )
            .await
            .expect("paused human goal");
        assert!(
            store
                .alab_capture_auth_standalone(thread, "A")
                .await
                .expect("capture A")
        );
        store
            .alab_defer_auth_recovery(thread, "A")
            .await
            .expect("A owned hold");
        store
            .alab_native_auth_turn_started(thread, "A")
            .await
            .expect("same native recovery");
        assert_eq!(
            store
                .alab_auth_goal_context(thread, "A")
                .await
                .expect("A proof"),
            Some(None)
        );
        assert!(
            store
                .has_thread_goal_continuation_deferral(thread)
                .await
                .expect("A hold")
        );
        store
            .alab_native_auth_turn_started(thread, "B")
            .await
            .expect("fresh accepted native turn");
        assert!(
            store
                .alab_auth_goal_context(thread, "A")
                .await
                .expect("retired A")
                .is_none()
        );
        assert!(
            !store
                .has_thread_goal_continuation_deferral(thread)
                .await
                .expect("old hold released")
        );
        assert!(
            !store
                .alab_capture_auth_standalone(thread, "A")
                .await
                .expect("A cannot resurrect")
        );
        assert!(
            store
                .alab_capture_auth_standalone(thread, "B")
                .await
                .expect("B can capture")
        );
        store
            .alab_defer_auth_recovery(thread, "B")
            .await
            .expect("B hold");
        assert!(
            !store
                .alab_retire_auth_recovery(thread, "A", None)
                .await
                .expect("late A cleanup")
        );
        store
            .alab_native_auth_turn_started(thread, "B")
            .await
            .expect("same B preserves its proof");
        assert_eq!(
            store
                .alab_auth_goal_context(thread, "B")
                .await
                .expect("B proof"),
            Some(None)
        );
        assert!(
            store
                .has_thread_goal_continuation_deferral(thread)
                .await
                .expect("B hold retained")
        );
        assert_eq!(
            store
                .get_thread_goal(thread)
                .await
                .expect("goal")
                .expect("present")
                .status,
            crate::ThreadGoalStatus::Paused
        );
    }

    #[tokio::test]
    #[expect(
        clippy::disallowed_methods,
        reason = "test-only writer-lock proof requires exactly two file connections and zero busy timeout; the production shim's five-connection pool would invalidate the lease-return barrier"
    )]
    async fn admission_lease_orders_an_actual_second_connection_goal_edit() {
        let dir = tempfile::tempdir().expect("database directory");
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(2)
            .connect_with(
                sqlx::sqlite::SqliteConnectOptions::new()
                    .filename(dir.path().join("goals.sqlite"))
                    .create_if_missing(true)
                    .foreign_keys(true)
                    .busy_timeout(std::time::Duration::ZERO),
            )
            .await
            .expect("real file database");
        let (store, thread) = store_with_pool(pool).await;
        wall(&store, thread).await;
        store
            .alab_restore_auth_goal(thread, "wall")
            .await
            .expect("restore associated goal");
        let lease = store
            .alab_auth_admission_lease(thread, "wall", Some("goal"))
            .await
            .expect("lease read")
            .expect("current exact proof");
        let mut writer = store
            .pool
            .acquire()
            .await
            .expect("independent writer connection");
        sqlx::query("PRAGMA busy_timeout=0")
            .execute(&mut *writer)
            .await
            .expect("immediate lock evidence");
        let error = sqlx::query("UPDATE thread_goals SET status='paused' WHERE thread_id=?")
            .bind(thread.to_string())
            .execute(&mut *writer)
            .await
            .expect_err("live admission blocks human writer");
        let code = error
            .as_database_error()
            .and_then(sqlx::error::DatabaseError::code)
            .expect("SQLite lock code");
        assert!(
            matches!(code.as_ref(), "5" | "6" | "SQLITE_BUSY" | "SQLITE_LOCKED"),
            "{error}"
        );
        // This models the callback's synchronous native acceptance. No foreign
        // writer can revoke the proof until that admission has linearized.
        drop(lease);
        // The other connection is still borrowed by `writer`, so this acquire
        // witnesses the lease connection's actual rollback and pool return.
        let released = store.pool.acquire().await.expect("dropped lease returned");
        drop(released);
        sqlx::query("UPDATE thread_goals SET status='paused' WHERE thread_id=?")
            .bind(thread.to_string())
            .execute(&mut *writer)
            .await
            .expect("same human writer succeeds after lease");
        drop(writer);
        assert!(
            store
                .alab_auth_admission_lease(thread, "wall", Some("goal"))
                .await
                .expect("revoked admission")
                .is_none()
        );
        assert_eq!(
            store
                .get_thread_goal(thread)
                .await
                .expect("goal")
                .expect("present")
                .status,
            crate::ThreadGoalStatus::Paused
        );
    }

    #[tokio::test]
    async fn admission_lease_requires_the_exact_native_wall_and_goal_without_regranting() {
        let (store, thread) = store().await;
        wall(&store, thread).await;
        for (origin, goal) in [
            ("wrong", Some("goal")),
            ("wall", Some("replacement")),
            ("wall", None),
        ] {
            assert!(
                store
                    .alab_auth_admission_lease(thread, origin, goal)
                    .await
                    .expect("mismatch")
                    .is_none()
            );
        }
        sqlx::query("UPDATE thread_goals SET status='paused'")
            .execute(store.pool.as_ref())
            .await
            .expect("human revokes");
        assert!(
            store
                .alab_auth_admission_lease(thread, "wall", Some("goal"))
                .await
                .expect("revoked")
                .is_none()
        );
        assert!(
            store
                .alab_auth_goal_context(thread, "wall")
                .await
                .expect("no recreated witness")
                .is_none()
        );
        assert!(
            !store
                .alab_capture_auth_standalone(thread, "wall")
                .await
                .expect("no downgrade after revocation")
        );
    }

    #[tokio::test]
    async fn passive_capture_without_a_goal_is_idempotent_and_never_installs_a_hold() {
        let (store, thread) = store().await;
        sqlx::query("DELETE FROM thread_goals")
            .execute(store.pool.as_ref())
            .await
            .expect("goalless");
        assert!(
            store
                .alab_capture_auth_standalone(thread, "wall-A")
                .await
                .expect("capture")
        );
        assert!(
            store
                .alab_capture_auth_standalone(thread, "wall-A")
                .await
                .expect("retry same capture")
        );
        assert_eq!(
            store
                .alab_auth_goal_context(thread, "wall-A")
                .await
                .expect("proof"),
            Some(None)
        );
        assert!(
            !store
                .has_thread_goal_continuation_deferral(thread)
                .await
                .expect("no hold")
        );
        assert!(store.get_thread_goal(thread).await.expect("read").is_none());
        assert!(
            store
                .alab_retire_auth_recovery(thread, "wall-A", None)
                .await
                .expect("retire")
        );
        assert!(
            !store
                .alab_capture_auth_standalone(thread, "wall-A")
                .await
                .expect("cannot regrant")
        );
        assert!(
            store
                .alab_capture_auth_standalone(thread, "wall-B")
                .await
                .expect("fresh native wall")
        );
    }

    #[tokio::test]
    async fn passive_capture_preserves_inactive_goal_and_cannot_undo_a_human_edit() {
        for status in ["paused", "complete"] {
            let (store, thread) = store().await;
            sqlx::query("UPDATE thread_goals SET status=?")
                .bind(status)
                .execute(store.pool.as_ref())
                .await
                .expect("inactive goal");
            let before = store.get_thread_goal(thread).await.expect("read");
            assert!(
                store
                    .alab_capture_auth_standalone(thread, "wall-A")
                    .await
                    .expect("capture")
            );
            assert_eq!(
                store.get_thread_goal(thread).await.expect("goal unchanged"),
                before
            );
            assert!(
                !store
                    .has_thread_goal_continuation_deferral(thread)
                    .await
                    .expect("no hold")
            );
            sqlx::query("UPDATE thread_goals SET status=?")
                .bind(status)
                .execute(store.pool.as_ref())
                .await
                .expect("same value human edit");
            assert!(
                !store
                    .alab_capture_auth_standalone(thread, "wall-A")
                    .await
                    .expect("human revocation stands")
            );
            assert!(
                store
                    .alab_auth_goal_context(thread, "wall-A")
                    .await
                    .expect("revoked")
                    .is_none()
            );
            assert!(
                store
                    .alab_capture_auth_standalone(thread, "wall-B")
                    .await
                    .expect("new actual wall")
            );
        }
    }

    #[tokio::test]
    async fn passive_capture_cannot_downgrade_active_budget_limited_or_associated_goals() {
        for status in ["active", "budget_limited"] {
            let (store, thread) = store().await;
            sqlx::query("UPDATE thread_goals SET status=?")
                .bind(status)
                .execute(store.pool.as_ref())
                .await
                .expect("status");
            let before = store.get_thread_goal(thread).await.expect("goal");
            assert!(
                !store
                    .alab_capture_auth_standalone(thread, "wall")
                    .await
                    .expect("reject")
            );
            assert_eq!(
                store.get_thread_goal(thread).await.expect("unchanged"),
                before
            );
        }
        let (store, thread) = store().await;
        wall(&store, thread).await;
        assert!(
            !store
                .alab_capture_auth_standalone(thread, "wall")
                .await
                .expect("associated stays associated")
        );
        assert_eq!(
            store
                .alab_auth_goal_context(thread, "wall")
                .await
                .expect("associated proof"),
            Some(Some("goal".into()))
        );
    }

    #[tokio::test]
    async fn late_passive_capture_cannot_overwrite_or_resurrect_another_native_wall() {
        let (store, thread) = store().await;
        sqlx::query("UPDATE thread_goals SET status='paused'")
            .execute(store.pool.as_ref())
            .await
            .expect("paused");
        store
            .alab_record_auth_standalone(thread, "wall-A")
            .await
            .expect("real native A");
        store
            .alab_record_auth_standalone(thread, "wall-B")
            .await
            .expect("real native B replaces A");
        assert!(
            !store
                .alab_capture_auth_standalone(thread, "wall-A")
                .await
                .expect("B keeps proof")
        );
        assert_eq!(
            store
                .alab_auth_goal_context(thread, "wall-B")
                .await
                .expect("B proof"),
            Some(None)
        );
        assert!(
            store
                .alab_capture_auth_standalone(thread, "wall-B")
                .await
                .expect("same valid B")
        );
        sqlx::query("UPDATE thread_goals SET status='paused'")
            .execute(store.pool.as_ref())
            .await
            .expect("human edit after both native walls");
        assert!(
            !store
                .alab_capture_auth_standalone(thread, "wall-A")
                .await
                .expect("A cannot resurrect")
        );
        assert!(
            !store
                .alab_capture_auth_standalone(thread, "wall-B")
                .await
                .expect("B cannot resurrect")
        );
        assert!(
            store
                .alab_capture_auth_standalone(thread, "wall-C")
                .await
                .expect("new native C")
        );
    }

    #[tokio::test]
    async fn simultaneous_passive_captures_cannot_both_own_the_thread() {
        let (store, thread) = store().await;
        sqlx::query("DELETE FROM thread_goals")
            .execute(store.pool.as_ref())
            .await
            .expect("goalless");
        let (a, b) = tokio::join!(
            store.alab_capture_auth_standalone(thread, "wall-A"),
            store.alab_capture_auth_standalone(thread, "wall-B")
        );
        assert_ne!(a.expect("A capture"), b.expect("B capture"));
    }

    #[tokio::test]
    async fn goalless_standalone_recovery_keeps_its_witness_without_a_future_goal_hold() {
        let (store, thread) = store().await;
        sqlx::query("DELETE FROM thread_goals")
            .execute(store.pool.as_ref())
            .await
            .expect("no goal exists");
        assert!(
            sqlx::query("INSERT INTO thread_goal_continuation_deferrals(thread_id) VALUES(?)")
                .bind(thread.to_string())
                .execute(store.pool.as_ref())
                .await
                .is_err(),
            "fixture enforces the production foreign key"
        );
        store
            .alab_record_auth_standalone(thread, "standalone")
            .await
            .expect("standalone proof");
        store
            .alab_defer_auth_recovery(thread, "standalone")
            .await
            .expect("no deferral is needed without a goal");
        assert_eq!(
            store
                .alab_auth_goal_context(thread, "standalone")
                .await
                .expect("proof retained"),
            Some(None)
        );
        assert!(
            !store
                .has_thread_goal_continuation_deferral(thread)
                .await
                .expect("no future hold")
        );
        assert_eq!(
            store
                .get_thread_goal(thread)
                .await
                .expect("no goal fabricated"),
            None
        );
        sqlx::query(
            "INSERT INTO thread_goals VALUES(?,'new','new human goal','active',100,0,0,1000,1000)",
        )
        .bind(thread.to_string())
        .execute(store.pool.as_ref())
        .await
        .expect("new human goal");
        assert!(
            store
                .alab_auth_goal_context(thread, "standalone")
                .await
                .expect("new goal revokes standalone")
                .is_none()
        );
        assert!(
            !store
                .alab_retire_auth_recovery(thread, "standalone", None)
                .await
                .expect("old custody grants nothing")
        );
        assert!(
            !store
                .has_thread_goal_continuation_deferral(thread)
                .await
                .expect("new goal unblocked")
        );
    }

    #[tokio::test]
    async fn retiring_auth_custody_preserves_the_goal_and_revokes_its_witness() {
        let (store, thread) = store().await;
        wall(&store, thread).await;
        store
            .alab_restore_auth_goal(thread, "wall")
            .await
            .expect("restore");
        let before = store
            .get_thread_goal(thread)
            .await
            .expect("read")
            .expect("goal");
        assert!(
            store
                .alab_retire_auth_recovery(thread, "wall", Some("goal"))
                .await
                .expect("retire")
        );
        assert_eq!(
            store
                .get_thread_goal(thread)
                .await
                .expect("read")
                .expect("goal"),
            before
        );
        assert!(
            !store
                .has_thread_goal_continuation_deferral(thread)
                .await
                .expect("released")
        );
        assert!(
            store
                .alab_auth_goal_context(thread, "wall")
                .await
                .expect("proof")
                .is_none()
        );
        assert!(
            store
                .alab_restore_auth_goal(thread, "wall")
                .await
                .expect("cannot restore again")
                .is_none()
        );
        assert!(
            !store
                .alab_retire_auth_recovery(thread, "wall", Some("goal"))
                .await
                .expect("idempotent")
        );
    }

    #[tokio::test]
    async fn retired_witness_can_release_its_deferral_after_a_human_goal_edit() {
        let (store, thread) = store().await;
        wall(&store, thread).await;
        store
            .alab_restore_auth_goal(thread, "wall")
            .await
            .expect("restore");
        sqlx::query("UPDATE thread_goals SET status='paused',objective='human edit'")
            .execute(store.pool.as_ref())
            .await
            .expect("manual edit");
        let before = store
            .get_thread_goal(thread)
            .await
            .expect("read")
            .expect("goal");
        assert!(
            store
                .alab_auth_goal_context(thread, "wall")
                .await
                .expect("revoked")
                .is_none()
        );
        assert!(
            store
                .alab_retire_auth_recovery(thread, "wall", Some("goal"))
                .await
                .expect("retire owned deferral")
        );
        assert!(
            !store
                .has_thread_goal_continuation_deferral(thread)
                .await
                .expect("released")
        );
        assert_eq!(
            store
                .get_thread_goal(thread)
                .await
                .expect("read")
                .expect("goal"),
            before
        );
    }

    #[tokio::test]
    async fn stale_retirement_cannot_clear_a_new_walls_witness_or_deferral() {
        let (store, thread) = store().await;
        wall(&store, thread).await;
        store
            .alab_restore_auth_goal(thread, "wall")
            .await
            .expect("restore A");
        sqlx::query("UPDATE thread_goals SET goal_id='replacement',objective='new goal'")
            .execute(store.pool.as_ref())
            .await
            .expect("new goal");
        store
            .alab_update_for_auth_wall(
                thread,
                GoalUpdate {
                    objective: None,
                    status: Some(crate::ThreadGoalStatus::Blocked),
                    token_budget: None,
                    expected_goal_id: Some("replacement".into()),
                },
                Some("wall-B"),
            )
            .await
            .expect("wall B")
            .expect("blocked B");
        store
            .alab_restore_auth_goal(thread, "wall-B")
            .await
            .expect("restore B");
        let before = store
            .get_thread_goal(thread)
            .await
            .expect("read")
            .expect("goal");
        assert!(
            !store
                .alab_retire_auth_recovery(thread, "wall", Some("goal"))
                .await
                .expect("stale retire")
        );
        assert!(
            store
                .has_thread_goal_continuation_deferral(thread)
                .await
                .expect("B holds")
        );
        assert_eq!(
            store
                .alab_auth_goal_context(thread, "wall-B")
                .await
                .expect("B proof"),
            Some(Some("replacement".into()))
        );
        assert!(
            store
                .alab_defer_auth_recovery(thread, "wall")
                .await
                .is_err()
        );
        assert_eq!(
            store
                .get_thread_goal(thread)
                .await
                .expect("read")
                .expect("goal"),
            before
        );
    }

    #[tokio::test]
    async fn stale_retirement_cannot_clear_a_new_goal_on_the_same_native_turn() {
        let (store, thread) = store().await;
        wall(&store, thread).await;
        store
            .alab_restore_auth_goal(thread, "wall")
            .await
            .expect("restore A");
        sqlx::query("UPDATE thread_goals SET goal_id='replacement',objective='new goal'")
            .execute(store.pool.as_ref())
            .await
            .expect("new goal");
        store
            .alab_update_for_auth_wall(
                thread,
                GoalUpdate {
                    objective: None,
                    status: Some(crate::ThreadGoalStatus::Blocked),
                    token_budget: None,
                    expected_goal_id: Some("replacement".into()),
                },
                Some("wall"),
            )
            .await
            .expect("wall B")
            .expect("blocked B");
        store
            .alab_restore_auth_goal(thread, "wall")
            .await
            .expect("restore B");
        let before = store
            .get_thread_goal(thread)
            .await
            .expect("read")
            .expect("goal");
        assert!(
            !store
                .alab_retire_auth_recovery(thread, "wall", Some("goal"))
                .await
                .expect("stale retire")
        );
        assert!(
            store
                .has_thread_goal_continuation_deferral(thread)
                .await
                .expect("B holds")
        );
        assert_eq!(
            store
                .alab_auth_goal_context(thread, "wall")
                .await
                .expect("B proof"),
            Some(Some("replacement".into()))
        );
        assert_eq!(
            store
                .get_thread_goal(thread)
                .await
                .expect("read")
                .expect("goal"),
            before
        );
    }

    #[tokio::test]
    async fn auth_recovery_cannot_claim_a_preexisting_native_deferral() {
        let (store, thread) = store().await;
        sqlx::query("INSERT INTO thread_goal_continuation_deferrals(thread_id) VALUES(?)")
            .bind(thread.to_string())
            .execute(store.pool.as_ref())
            .await
            .expect("native hold");
        wall(&store, thread).await;
        store
            .alab_restore_auth_goal(thread, "wall")
            .await
            .expect("restore");
        store
            .alab_defer_auth_recovery(thread, "wall")
            .await
            .expect("defer existing recovery");
        assert!(
            store
                .alab_retire_auth_recovery(thread, "wall", Some("goal"))
                .await
                .expect("retire exact witness")
        );
        assert!(
            store
                .has_thread_goal_continuation_deferral(thread)
                .await
                .expect("native hold survives")
        );
        assert!(
            store
                .alab_auth_goal_context(thread, "wall")
                .await
                .expect("auth proof retired")
                .is_none()
        );
    }

    #[tokio::test]
    async fn native_deferral_clear_prevents_stale_auth_ownership_of_a_later_hold() {
        let (store, thread) = store().await;
        wall(&store, thread).await;
        store
            .alab_restore_auth_goal(thread, "wall")
            .await
            .expect("restore");
        store
            .clear_thread_goal_continuation_deferral(thread)
            .await
            .expect("native clear");
        sqlx::query("INSERT INTO thread_goal_continuation_deferrals(thread_id) VALUES(?)")
            .bind(thread.to_string())
            .execute(store.pool.as_ref())
            .await
            .expect("later unrelated hold");
        assert!(
            store
                .alab_retire_auth_recovery(thread, "wall", Some("goal"))
                .await
                .expect("retire witness")
        );
        assert!(
            store
                .has_thread_goal_continuation_deferral(thread)
                .await
                .expect("unrelated hold survives")
        );
    }

    #[tokio::test]
    async fn transient_restore_does_not_leave_a_deferral_for_a_recreated_runtime() {
        let (store, thread) = store().await;
        wall(&store, thread).await;
        let restored = store
            .alab_restore_auth_goal_with_deferral(thread, "wall", false)
            .await
            .expect("transient restore")
            .expect("same goal");
        assert_eq!(restored.status, crate::ThreadGoalStatus::Active);
        let recreated = GoalStore::new(Arc::clone(&store.pool));
        assert!(
            !recreated
                .has_thread_goal_continuation_deferral(thread)
                .await
                .expect("deferral")
        );
        assert_eq!(
            recreated
                .get_thread_goal(thread)
                .await
                .expect("read")
                .expect("goal")
                .status,
            crate::ThreadGoalStatus::Active
        );
    }

    #[tokio::test]
    async fn exact_auth_proof_restores_once_with_deferral_and_no_accounting_reset() {
        let (store, thread) = store().await;
        wall(&store, thread).await;
        assert_eq!(
            store
                .alab_auth_goal_context(thread, "wall")
                .await
                .expect("proof"),
            Some(Some("goal".into()))
        );
        assert!(
            store
                .alab_restore_auth_goal(thread, "wrong")
                .await
                .expect("wrong origin")
                .is_none()
        );
        let restored = store
            .alab_restore_auth_goal(thread, "wall")
            .await
            .expect("restore")
            .expect("matching proof");
        assert_eq!(restored.status, crate::ThreadGoalStatus::Active);
        assert_eq!(restored.tokens_used, 10);
        assert_eq!(restored.objective, "original");
        assert!(
            store
                .has_thread_goal_continuation_deferral(thread)
                .await
                .expect("deferral")
        );
        assert!(
            store
                .alab_restore_auth_goal(thread, "wall")
                .await
                .expect("retry admission")
                .is_some()
        );
    }

    #[tokio::test]
    async fn every_subsequent_goal_edit_revokes_auth_restoration() {
        for edit in [
            "UPDATE thread_goals SET status='blocked'",
            "UPDATE thread_goals SET status='paused'",
            "UPDATE thread_goals SET status='complete'",
            "UPDATE thread_goals SET goal_id='replacement'",
            "UPDATE thread_goals SET tokens_used=100",
            "UPDATE thread_goals SET updated_at_ms=updated_at_ms",
        ] {
            let (store, thread) = store().await;
            wall(&store, thread).await;
            sqlx::query(edit)
                .execute(store.pool.as_ref())
                .await
                .expect("independent goal edit");
            assert!(
                store
                    .alab_auth_goal_context(thread, "wall")
                    .await
                    .expect("proof revoked")
                    .is_none(),
                "{edit}"
            );
            assert!(
                store
                    .alab_restore_auth_goal(thread, "wall")
                    .await
                    .expect("safe refusal")
                    .is_none(),
                "{edit}"
            );
        }
        let (store, thread) = store().await;
        wall(&store, thread).await;
        store
            .alab_restore_auth_goal(thread, "wall")
            .await
            .expect("restore");
        sqlx::query("UPDATE thread_goals SET status='active'")
            .execute(store.pool.as_ref())
            .await
            .expect("same-value manual edit");
        assert!(
            !store
                .alab_auth_goal_pending(thread, "wall")
                .await
                .expect("invalidated restored witness")
        );
    }

    #[tokio::test]
    async fn standalone_proof_is_revoked_by_new_or_edited_goals() {
        let (store, thread) = store().await;
        store
            .alab_record_auth_standalone(thread, "wall")
            .await
            .expect("active goal is not standalone");
        assert!(
            store
                .alab_auth_goal_context(thread, "wall")
                .await
                .expect("context")
                .is_none()
        );
        sqlx::query("UPDATE thread_goals SET status='complete'")
            .execute(store.pool.as_ref())
            .await
            .expect("old completed goal");
        store
            .alab_record_auth_standalone(thread, "standalone")
            .await
            .expect("standalone proof");
        assert_eq!(
            store
                .alab_auth_goal_context(thread, "standalone")
                .await
                .expect("context"),
            Some(None)
        );
        sqlx::query("UPDATE thread_goals SET status='complete'")
            .execute(store.pool.as_ref())
            .await
            .expect("same-value edit");
        assert!(
            store
                .alab_auth_goal_context(thread, "standalone")
                .await
                .expect("revoked")
                .is_none()
        );
        sqlx::query("DELETE FROM thread_goals")
            .execute(store.pool.as_ref())
            .await
            .expect("remove old goal");
        store
            .alab_record_auth_standalone(thread, "empty")
            .await
            .expect("empty goal proof");
        assert_eq!(
            store
                .alab_auth_goal_context(thread, "empty")
                .await
                .expect("context"),
            Some(None)
        );
        sqlx::query(
            "INSERT INTO thread_goals VALUES(?,'new','new task','active',100,0,0,1000,1000)",
        )
        .bind(thread.to_string())
        .execute(store.pool.as_ref())
        .await
        .expect("new goal");
        assert!(
            store
                .alab_auth_goal_context(thread, "empty")
                .await
                .expect("revoked")
                .is_none()
        );
    }

    #[tokio::test]
    async fn only_the_accepted_native_turn_preserves_proof_through_accounting() {
        let (store, thread) = store().await;
        wall(&store, thread).await;
        store
            .alab_restore_auth_goal(thread, "wall")
            .await
            .expect("restore");
        assert!(
            store
                .alab_accept_auth_goal(thread, "wall", "accepted")
                .await
                .expect("accept")
        );
        assert!(
            !store
                .alab_accept_auth_goal(thread, "wall", "other")
                .await
                .expect("wrong turn")
        );
        store
            .alab_account_auth_goal_usage(
                thread,
                2,
                5,
                GoalAccountingMode::ActiveOnly,
                Some("goal"),
                Some("accepted"),
            )
            .await
            .expect("trusted accounting");
        assert!(
            store
                .alab_auth_goal_pending(thread, "wall")
                .await
                .expect("proof retained")
        );
        assert_eq!(
            store
                .get_thread_goal(thread)
                .await
                .expect("goal")
                .expect("present")
                .tokens_used,
            15
        );
        store
            .alab_account_auth_goal_usage(
                thread,
                0,
                1,
                GoalAccountingMode::ActiveOnly,
                Some("goal"),
                Some("other"),
            )
            .await
            .expect("unrelated accounting");
        assert!(
            !store
                .alab_auth_goal_pending(thread, "wall")
                .await
                .expect("untrusted update invalidated")
        );
    }

    #[tokio::test]
    async fn accepted_accounting_revokes_proof_when_budget_is_spent() {
        let (store, thread) = store().await;
        wall(&store, thread).await;
        store
            .alab_restore_auth_goal(thread, "wall")
            .await
            .expect("restore");
        store
            .alab_accept_auth_goal(thread, "wall", "accepted")
            .await
            .expect("accept");
        store
            .alab_account_auth_goal_usage(
                thread,
                0,
                90,
                GoalAccountingMode::ActiveOnly,
                Some("goal"),
                Some("accepted"),
            )
            .await
            .expect("budget crossing");
        assert!(
            !store
                .alab_auth_goal_pending(thread, "wall")
                .await
                .expect("proof revoked")
        );
        assert_eq!(
            store
                .get_thread_goal(thread)
                .await
                .expect("goal")
                .expect("present")
                .status,
            crate::ThreadGoalStatus::BudgetLimited
        );
    }

    #[tokio::test]
    async fn concurrent_manual_block_wins_over_auth_restore() {
        let (store, thread) = store().await;
        wall(&store, thread).await;
        let (restored, changed) = tokio::join!(
            store.alab_restore_auth_goal(thread, "wall"),
            sqlx::query("UPDATE thread_goals SET status='blocked'").execute(store.pool.as_ref())
        );
        restored.expect("serialized restore");
        changed.expect("serialized manual edit");
        assert_eq!(
            store
                .get_thread_goal(thread)
                .await
                .expect("goal")
                .expect("present")
                .status,
            crate::ThreadGoalStatus::Blocked
        );
        assert!(
            !store
                .alab_auth_goal_pending(thread, "wall")
                .await
                .expect("proof")
        );
    }
}
