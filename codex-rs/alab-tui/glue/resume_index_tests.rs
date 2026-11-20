// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::LoginScope;
use crate::resume_picker::ProviderFilter;
use crate::resume_picker::SessionStatus;
use crate::resume_picker::thread_list_params;
use codex_app_server_protocol::ThreadSortKey;
use codex_protocol::ThreadId;
use codex_rollout::RolloutConfig;
use codex_state::SqliteConfig;
use codex_state::StateRuntime;
use codex_utils_absolute_path::AbsolutePathBuf;
use serde_json::json;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::time::UNIX_EPOCH;

fn write_rollout(
    root: &Path,
    filename_id: ThreadId,
    revision: Option<ThreadId>,
    header_id: ThreadId,
    source: &str,
    text: &str,
) -> std::io::Result<PathBuf> {
    let suffix = revision
        .map(|revision| format!("_{revision}"))
        .unwrap_or_default();
    let path = root.join(format!(
        "rollout-2025-01-01T03-00-00-{filename_id}{suffix}.jsonl"
    ));
    let meta = json!({"timestamp":"2025-01-01T03:00:00Z","type":"session_meta","payload":{
        "id":header_id,"timestamp":"2025-01-01T03:00:00Z","cwd":root,"originator":"resume-test","cli_version":"0.170.0",
        "source":source,"model_provider":"fixture","creator_account_id":"creator","memory_mode":"disabled"}});
    let user = json!({"timestamp":"2025-01-01T03:00:01Z","type":"event_msg","payload":{"type":"user_message","message":text,"kind":"plain"}});
    std::fs::write(&path, format!("{meta}\n{user}\n"))?;
    Ok(path)
}

fn test_config(home: &Path) -> anyhow::Result<RolloutConfig> {
    Ok(RolloutConfig {
        codex_home: home.to_path_buf(),
        sqlite: SqliteConfig::new_for_testing(AbsolutePathBuf::try_from(home)?),
        cwd: home.to_path_buf(),
        model_provider_id: "fixture".into(),
        generate_memories: true,
    })
}

async fn indexed_page(
    db: &Arc<StateRuntime>,
    config: &RolloutConfig,
    cursor: Option<&codex_rollout::Cursor>,
) -> std::io::Result<codex_rollout::ThreadsPage> {
    codex_rollout::RolloutRecorder::list_threads_from_state_db(
        Some(db.clone()),
        config,
        1,
        cursor,
        codex_rollout::ThreadSortKey::UpdatedAt,
        codex_rollout::SortDirection::Desc,
        &[],
        None,
        None,
        &config.model_provider_id,
        None,
    )
    .await
}

#[tokio::test]
async fn all_local_restores_missing_rollouts_before_indexed_paging() -> anyhow::Result<()> {
    let home = tempfile::tempdir()?;
    let config = test_config(home.path())?;
    let db = StateRuntime::init(config.sqlite.clone(), config.model_provider_id.clone()).await?;
    db.mark_backfill_complete(None).await?;
    let sessions = home.path().join("sessions/2025/01/01");
    std::fs::create_dir_all(&sessions)?;
    let a = ThreadId::from_u128(11);
    let b = ThreadId::from_u128(12);
    let wrong = ThreadId::from_u128(13);
    let later = ThreadId::from_u128(14);
    let b_path = write_rollout(&sessions, b, None, b, "cli", "Indexed B")?;
    let params = thread_list_params(
        None,
        None,
        SessionStatus::Active,
        ProviderFilter::All,
        ThreadSortKey::UpdatedAt,
        true,
        true,
    );
    let mut checked = None;
    LoginScope::AllLocal
        .repair_initial_page(&params, true, Some(&config), 1, &mut checked)
        .await;
    let before = db.get_thread(b).await?.expect("B indexed");
    assert_eq!(before.rollout_path, b_path);
    write_rollout(
        &sessions,
        b,
        Some(ThreadId::from_u128(100)),
        b,
        "cli",
        "Unselected B revision",
    )?;
    write_rollout(&sessions, a, None, a, "exec", "Original A")?;
    let selected = write_rollout(
        &sessions,
        a,
        Some(ThreadId::from_u128(101)),
        a,
        "exec",
        "Restored A",
    )?;
    std::fs::File::open(&selected)?
        .set_modified(UNIX_EPOCH + Duration::from_secs(1_640_995_200))?;
    write_rollout(&sessions, wrong, None, later, "exec", "Mismatched header")?;
    codex_rollout::append_thread_name(home.path(), a, "Named A").await?;
    let mut continuation = params.clone();
    continuation.cursor = Some("a real continuation is never repaired".into());
    for (scope, local, request, generation) in [
        (LoginScope::Current, true, &params, 2),
        (LoginScope::Unavailable, true, &params, 2),
        (LoginScope::AllLocal, false, &params, 2),
        (LoginScope::AllLocal, true, &continuation, 2),
        (LoginScope::AllLocal, true, &params, 1),
    ] {
        scope
            .repair_initial_page(request, local, Some(&config), generation, &mut checked)
            .await;
    }
    // The previous DB-only first page found B and never reached filesystem fallback.
    let indexed = indexed_page(&db, &config, None).await?;
    assert_eq!(indexed.items[0].thread_id, Some(b));
    assert!(indexed.next_cursor.is_none());

    // A traversal failure must leave this healthy page usable. It also consumes the current
    // generation, including a retry of the same page with valid filesystem configuration.
    let mut inaccessible = config.clone();
    inaccessible.codex_home = b_path.clone(); // regular file as the parent of sessions/
    assert_eq!(
        codex_alab_tui::LocalThreadStore::repair_local_resume_index(&inaccessible, false)
            .await
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::NotADirectory
    );
    LoginScope::AllLocal
        .repair_initial_page(&params, true, Some(&inaccessible), 2, &mut checked)
        .await;
    LoginScope::AllLocal
        .repair_initial_page(&params, true, Some(&config), 2, &mut checked)
        .await;
    let healthy_page = indexed_page(&db, &config, None).await?;
    assert_eq!(healthy_page.items[0].thread_id, Some(b));
    assert!(healthy_page.next_cursor.is_none());
    let mut mixed_first_page = params.clone();
    mixed_first_page.use_state_db_only = false;
    // A different unreadable day must not hide A or change B's selected database row.
    #[cfg(unix)]
    let (other_day, day_permissions) = {
        use std::os::unix::fs::PermissionsExt;
        let path = home.path().join("sessions/2024/01/01");
        std::fs::create_dir_all(&path)?;
        let permissions = std::fs::metadata(&path)?.permissions();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0))?;
        (path, permissions)
    };
    LoginScope::AllLocal
        .repair_initial_page(&mixed_first_page, true, Some(&config), 3, &mut checked)
        .await;
    #[cfg(unix)]
    std::fs::set_permissions(other_day, day_permissions)?;
    assert_eq!(checked, Some(3));
    let restored = db
        .get_thread(a)
        .await?
        .expect("restored A must precede initial DB page");
    assert_eq!(restored.rollout_path, selected);
    assert_eq!(restored.first_user_message.as_deref(), Some("Restored A"));
    assert_eq!(restored.name.as_deref(), Some("Named A"));
    assert_eq!(restored.creator_account_id.as_deref(), Some("creator"));
    assert_eq!(
        db.get_thread_memory_mode(a).await?.as_deref(),
        Some("disabled")
    );
    // Another writer can finish after our ID snapshot but before our insert.
    // Exercise that exact conflict with a different selected path and memory preference.
    let mut losing_insert = before.clone();
    losing_insert.rollout_path = selected;
    assert!(
        !db.insert_thread_if_absent_with_memory_mode(&losing_insert, "enabled")
            .await?
    );
    assert_eq!(
        db.get_thread_memory_mode(b).await?.as_deref(),
        Some("disabled")
    );
    assert_eq!(db.get_thread(b).await?, Some(before));
    assert!(db.get_thread(wrong).await?.is_none());
    assert!(db.get_thread(later).await?.is_none());
    let first = indexed_page(&db, &config, None).await?;
    assert_eq!(first.items[0].thread_id, Some(b));
    let cursor = first
        .next_cursor
        .expect("older restored history has a second page");
    let second = indexed_page(&db, &config, Some(&cursor)).await?;
    assert_eq!(second.items[0].thread_id, Some(a));
    assert!(second.next_cursor.is_none());

    // A damaged auxiliary names index does not make a readable rollout unavailable.
    let names = home.path().join("session_index.jsonl");
    std::fs::remove_file(&names)?;
    std::fs::create_dir(&names)?;
    let unnamed = ThreadId::from_u128(30);
    write_rollout(
        &sessions,
        unnamed,
        None,
        unnamed,
        "cli",
        "Title from saved work",
    )?;
    LoginScope::AllLocal
        .repair_initial_page(&params, true, Some(&config), 4, &mut checked)
        .await;
    let unnamed = db
        .get_thread(unnamed)
        .await?
        .expect("unnamed session still restored");
    assert_eq!(
        unnamed.first_user_message.as_deref(),
        Some("Title from saved work")
    );
    Ok(())
}

#[tokio::test]
async fn all_local_restores_a_rollout_beyond_the_filesystem_scan_limit() -> anyhow::Result<()> {
    async fn disk_page(
        config: &RolloutConfig,
        cursor: Option<&codex_rollout::Cursor>,
    ) -> std::io::Result<codex_rollout::ThreadsPage> {
        codex_rollout::get_threads(
            &config.codex_home,
            1,
            cursor,
            codex_rollout::ThreadSortKey::CreatedAt,
            &[],
            None,
            None,
            &config.model_provider_id,
        )
        .await
    }

    let home = tempfile::tempdir()?;
    let config = test_config(home.path())?;
    let db = StateRuntime::init(config.sqlite.clone(), config.model_provider_id.clone()).await?;
    db.mark_backfill_complete(None).await?;
    let original_day = home.path().join("sessions/2025/01/01");
    let newer_day = home.path().join("sessions/2025/01/02");
    std::fs::create_dir_all(&original_day)?;
    std::fs::create_dir_all(&newer_day)?;
    let a = ThreadId::from_u128(1);
    let b = ThreadId::from_u128(2);
    write_rollout(&original_day, b, None, b, "cli", "Indexed B")?;
    codex_alab_tui::LocalThreadStore::repair_local_resume_index(&config, false).await?;
    let indexed_b = db
        .get_thread(b)
        .await?
        .expect("B was indexed before restore");
    let restored = write_rollout(&original_day, a, None, a, "exec", "Restored A")?;
    std::fs::File::open(&restored)?
        .set_modified(UNIX_EPOCH + Duration::from_secs(1_640_995_200))?;

    // B and A have the same creation timestamp. B's higher ID supplies a real cursor just before A.
    let head = disk_page(&config, None).await?;
    assert_eq!(head.items[0].thread_id, Some(b));
    let cursor = head.next_cursor.expect("two source histories");
    assert_eq!(
        disk_page(&config, Some(&cursor)).await?.items[0].thread_id,
        Some(a)
    );

    // An indexed thread's revision bodies are never needed for discovery. The 9,999 newer
    // filenames plus B's selected rollout place A exactly one file outside the 10,000-file window.
    for revision in 0..9_999 {
        let revision = ThreadId::from_u128(3 + revision);
        std::fs::File::create_new(
            newer_day.join(format!("rollout-2025-01-02T03-00-00-{b}_{revision}.jsonl")),
        )?;
    }
    let bounded = disk_page(&config, Some(&cursor)).await?;
    assert!(bounded.reached_scan_cap);
    assert!(bounded.items.is_empty());
    assert!(db.get_thread(a).await?.is_none());

    codex_alab_tui::LocalThreadStore::repair_local_resume_index(&config, false).await?;
    assert_eq!(db.get_thread(b).await?, Some(indexed_b));
    let first = indexed_page(&db, &config, None).await?;
    assert_eq!(first.items[0].thread_id, Some(b));
    let cursor = first.next_cursor.expect("restored A should be pageable");
    let second = indexed_page(&db, &config, Some(&cursor)).await?;
    assert_eq!(second.items[0].thread_id, Some(a));
    assert_eq!(second.items[0].path, restored);
    assert!(second.next_cursor.is_none());
    Ok(())
}

#[tokio::test]
async fn all_local_retries_incomplete_selected_rollouts_without_changing_existing_metadata()
-> anyhow::Result<()> {
    let home = tempfile::tempdir()?;
    let config = test_config(home.path())?;
    let db = StateRuntime::init(config.sqlite.clone(), config.model_provider_id.clone()).await?;
    db.mark_backfill_complete(None).await?;
    let directory = home.path().join("sessions/2025/01/01");
    std::fs::create_dir_all(&directory)?;
    let id = ThreadId::from_u128(41);
    let selected = write_rollout(&directory, id, None, id, "exec", "Later restored user")?;
    let bytes = std::fs::read_to_string(&selected)?;
    let (header, _) = bytes.split_once('\n').unwrap();
    std::fs::write(&selected, format!("{header}\n"))?;
    codex_alab_tui::LocalThreadStore::repair_local_resume_index(&config, false).await?;
    assert!(db.get_thread(id).await?.is_none());

    // This is the actual state a preceding version stored on a header-only first visit.
    let old =
        codex_rollout::extract_metadata_from_rollout(&selected, &config.model_provider_id).await?;
    assert!(
        db.insert_thread_if_absent_with_memory_mode(&old.metadata, "disabled")
            .await?
    );
    let mut expected = db.get_thread(id).await?.unwrap();
    let competing = write_rollout(
        &directory,
        id,
        Some(ThreadId::from_u128(99)),
        id,
        "exec",
        "Unselected newer history",
    )?;
    codex_alab_tui::LocalThreadStore::repair_local_resume_index(&config, false).await?;
    assert_eq!(db.get_thread(id).await?, Some(expected.clone()));
    std::fs::write(&selected, &bytes)?;
    db.repair_empty_resume_preview(id, &competing, false, "wrong path", Some("wrong"))
        .await?;
    db.repair_empty_resume_preview(id, &selected, true, "wrong status", Some("wrong"))
        .await?;
    assert_eq!(db.get_thread(id).await?, Some(expected.clone()));
    codex_alab_tui::LocalThreadStore::repair_local_resume_index(&config, false).await?;
    expected.preview = Some("Later restored user".into());
    expected.first_user_message = expected.preview.clone();
    assert_eq!(db.get_thread(id).await?, Some(expected.clone()));
    db.repair_empty_resume_preview(id, &selected, false, "late writer", Some("wrong"))
        .await?;
    assert_eq!(db.get_thread(id).await?, Some(expected));
    assert_eq!(
        db.get_thread_memory_mode(id).await?.as_deref(),
        Some("disabled")
    );
    assert_eq!(std::fs::read_to_string(&selected)?, bytes);
    Ok(())
}

#[tokio::test]
async fn all_local_recovers_only_the_retained_prefix_of_a_reverted_thread() -> anyhow::Result<()> {
    check_restored_prefix(false).await
}

#[tokio::test]
async fn all_local_uses_a_matching_replica_for_an_unreadable_inherited_preview()
-> anyhow::Result<()> {
    check_restored_prefix(true).await
}

async fn check_restored_prefix(use_replica: bool) -> anyhow::Result<()> {
    let home = tempfile::tempdir()?;
    let config = test_config(home.path())?;
    let db = StateRuntime::init(config.sqlite.clone(), config.model_provider_id.clone()).await?;
    db.mark_backfill_complete(None).await?;
    let directory = home.path().join("sessions/2025/01/01");
    std::fs::create_dir_all(&directory)?;
    for (n, retained) in [(51_u128, true), (52, false)] {
        let id = ThreadId::from_u128(n);
        let original = write_rollout(&directory, id, None, id, "exec", "Retained first turn")?;
        let original_bytes = std::fs::read_to_string(&original)?;
        let mut records = original_bytes
            .lines()
            .map(serde_json::from_str::<serde_json::Value>)
            .collect::<Result<Vec<_>, _>>()?;
        records[0]["ordinal"] = json!(0);
        records[0]["payload"]["history_mode"] = json!("paginated");
        records[1]["ordinal"] = json!(1);
        let mut discarded = records[1].clone();
        discarded["ordinal"] = json!(2);
        discarded["payload"]["message"] = json!("Discarded user turn");
        let header = format!("{}\n", records[0]);
        let prefix = format!("{header}{}\n", records[1]);
        let ancestor = format!("{prefix}{discarded}\n");
        std::fs::write(&original, &ancestor)?;
        let selected = write_rollout(
            &directory,
            id,
            Some(ThreadId::from_u128(n + 100)),
            id,
            "exec",
            "unused",
        )?;
        let (ordinal, offset) = if retained {
            (2, prefix.len())
        } else {
            (1, header.len())
        };
        records[0]["ordinal"] = json!(ordinal);
        records[0]["payload"]["history_base"] =
            json!({"thread_id":id,"end_ordinal_exclusive":ordinal,"end_byte_offset":offset});
        let leaf = format!("{}\n", records[0]);
        std::fs::write(&selected, &leaf)?;
        let archived = home
            .path()
            .join("archived_sessions")
            .join(original.file_name().unwrap());
        let mut active = ancestor.clone();
        if use_replica {
            // Keep the header, first user record byte length and cutoff intact. The error
            // occurs after the resolver opens the preferred file and validates its size.
            let user = records[1].to_string();
            active = active.replacen(&user, &"x".repeat(user.len()), 1);
            assert_ne!(active, ancestor);
            std::fs::write(&original, &active)?;
            codex_alab_tui::LocalThreadStore::repair_local_resume_index(&config, false).await?;
            assert!(db.get_thread(id).await?.is_none());
            std::fs::create_dir_all(archived.parent().unwrap())?;
            let wrong_owner =
                ancestor.replacen(&id.to_string(), &ThreadId::from_u128(91).to_string(), 1);
            assert_ne!(wrong_owner, ancestor);
            std::fs::write(&archived, &wrong_owner)?;
            codex_alab_tui::LocalThreadStore::repair_local_resume_index(&config, false).await?;
            assert!(db.get_thread(id).await?.is_none());
            assert_eq!(std::fs::read_to_string(&archived)?, wrong_owner);
            std::fs::write(&archived, &ancestor)?;
        }
        codex_alab_tui::LocalThreadStore::repair_local_resume_index(&config, false).await?;
        if retained {
            let restored = db
                .get_thread(id)
                .await?
                .expect("visible inherited user turn");
            assert_eq!(restored.rollout_path, selected);
            assert_eq!(restored.preview.as_deref(), Some("Retained first turn"));
            assert_eq!(restored.first_user_message, restored.preview);
            assert_eq!(restored.creator_account_id.as_deref(), Some("creator"));
        } else {
            assert!(db.get_thread(id).await?.is_none());
        }
        assert_eq!(std::fs::read_to_string(&selected)?, leaf);
        assert_eq!(std::fs::read_to_string(&original)?, active);
        if use_replica {
            assert_eq!(std::fs::read_to_string(&archived)?, ancestor);
        }
    }
    Ok(())
}
