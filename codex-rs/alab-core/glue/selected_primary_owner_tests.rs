// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual configured manager/Core/source-lifetime controls. These deliberately
//! do not manufacture AcceptedOrigin, RuntimeCommit or an installed UI primary.
use super::*;
use crate::message_processor::alab_task_mcp::tests::Harness;
use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use wiremock::MockServer;

fn source<T>(result: std::result::Result<T, JSONRPCErrorError>) -> Result<T> {
    result.map_err(|error| anyhow::anyhow!("Source primary owner error, code {}", error.code))
}

#[tokio::test]
async fn empty_selection_is_versioned_runtime_custody_even_while_actual_core_exists() -> Result<()>
{
    let server = MockServer::start().await;
    let h = Harness::new(&server.uri()).await?;
    let result = async {
        let (actual_processor, _) = h.processor.alab_selection_publication_parts_for_tests();
        let processor = Arc::new(actual_processor);
        let actual = processor.thread_manager.get_thread(h.parent_id).await?;
        let op = InitialSessionOperation::new(
            InitialSessionPlan::new(Kind::Selection, None),
            h.input.clone(),
            processor.clone(),
        );
        let CurrentPrimary::Empty(empty) = source(op.current_primary())? else {
            anyhow::bail!("construction alone selected a UI primary");
        };
        ensure!(empty.runtime_id_data().len() == 32 && empty.version_data() == 0);
        ensure!(empty.belongs_to(&h.input));
        let CurrentPrimary::Empty(second) = source(op.current_primary())? else {
            anyhow::bail!("observation changed empty selection");
        };
        ensure!(empty.same_version(&second));
        let guard = source(empty.lock_unchanged())?;
        drop(guard); // Synchronous Source guard only, never across await.
        let replacement = InitialSessionOperation::new(
            InitialSessionPlan::new(Kind::Selection, None),
            h.input.clone(),
            processor.clone(),
        );
        let CurrentPrimary::Empty(other) = source(replacement.current_primary())? else {
            anyhow::bail!("new owner chose a Core");
        };
        ensure!(!empty.same_version(&other));
        ensure!(empty.runtime_id_data() != other.runtime_id_data());
        let still_actual = processor.thread_manager.get_thread(h.parent_id).await?;
        ensure!(
            Arc::ptr_eq(&actual, &still_actual),
            "empty must not mean no live Core or recreate one"
        );
        op.stop();
        ensure!(
            empty.recheck_owned().is_err() && empty.lock_unchanged().is_err(),
            "actual owner closure must deny old empty resource"
        );
        Ok::<(), anyhow::Error>(())
    }
    .await;
    let cleanup = h.shutdown().await;
    result?;
    cleanup
}

#[tokio::test]
async fn weak_primary_marker_preserves_identity_without_retaining_a_real_lifetime_owner()
-> Result<()> {
    let server = MockServer::start().await;
    let h = Harness::new(&server.uri()).await?;
    let result = async {
        let (actual_processor, _) = h.processor.alab_selection_publication_parts_for_tests();
        let actual = actual_processor
            .thread_manager
            .get_thread(h.parent_id)
            .await?;
        let lease = source(
            actual_processor
                .alab_acquire_primary(h.parent_id, actual.clone(), &h.input)
                .await,
        )?;
        let weak = lease.downgrade();
        let same = lease.clone();
        ensure!(
            weak.upgrade()
                .context("actual weak owner missing")?
                .same_owner(&lease)
        );
        drop(lease);
        ensure!(
            weak.upgrade()
                .context("clone did not keep same actual owner")?
                .same_owner(&same)
        );
        drop(same);
        ensure!(
            weak.upgrade().is_none(),
            "weak Scope marker must not keep the old lease alive"
        );
        let current = actual_processor
            .thread_manager
            .get_thread(h.parent_id)
            .await?;
        ensure!(
            Arc::ptr_eq(&actual, &current),
            "ordinary live Core is independent of lease-marker custody"
        );
        Ok::<(), anyhow::Error>(())
    }
    .await;
    let cleanup = h.shutdown().await;
    result?;
    cleanup
}

#[path = "retained_selected_lease_controls.rs"]
mod lease_controls;
