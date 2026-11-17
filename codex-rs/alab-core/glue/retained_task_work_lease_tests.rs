// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Child of the existing real HTTP host tests. Checks accepted-work custody
//! using an actual host-issued private binding, not a fake Core/SDK owner.
use super::*;

#[tokio::test]
async fn abort_restores_original_work_and_retains_its_actual_private_binding() -> Result<()> {
    let model = wiremock::MockServer::start().await;
    let h = Harness::new(&model.uri()).await?;
    let mut worker = None;
    let result: Result<()> = async {
        let root_binding = {
            let registry = h
                .host
                .control
                .registry
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            registry.bindings.values().find_map(|(binding, _)| {
                let parent = binding
                    .parent
                    .read()
                    .unwrap_or_else(PoisonError::into_inner);
                parent
                    .as_ref()
                    .filter(|bound| Arc::ptr_eq(&bound.thread, &h.parent))
                    .map(|_| binding.clone())
            })
        }
        .context("real START did not retain its actual root binding")?;
        let (parent_id, parent, capture, features) = root_binding.actual_parent().await?;
        let prepared = h.host.control.prepare_child_binding()?;
        let issued = prepared.binding.clone();
        let issued_key = issued.key;
        let cancellation = CancellationToken::new();
        let work = CreateWork::new(
            CreateRequest {
                parent_id,
                parent,
                operation: OperationContext {
                    span: tracing::info_span!("actual_work_lease_test"),
                    parent_trace: None,
                    cancellation: cancellation.clone(),
                },
                arguments: parse_arguments(json!({"prompt":"lease custody control"}))
                    .map_err(anyhow::Error::msg)?,
                baseline: capture.prospective_create(),
                features,
                app_server_client_name: capture.client_name(),
                app_server_client_version: capture.client_version(),
                client_mcp_extensions: capture.extensions(),
            },
            prepared,
        );
        let (completion, _) = tokio::sync::watch::channel(None);
        let accepted = Arc::new(AcceptedCreate {
            work: std::sync::Mutex::new(Some(work)),
            restored: tokio::sync::Notify::new(),
            cancellation,
            completion,
        });
        let mut receive = accepted.completion.subscribe();
        let key = Uuid::new_v4();
        h.host
            .control
            .registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .operations
            .insert(key, accepted.clone());
        let guard = OperationGuard {
            operation: accepted.clone(),
            armed: true,
        };
        let leased_record = accepted.clone();
        let (entered, leased) = oneshot::channel();
        worker = Some(h.host.control.operations.spawn(async move {
            let _terminal = guard;
            let _lease = leased_record.lease().await;
            let _ = entered.send(());
            std::future::pending::<()>().await;
        }));
        bounded(leased)
            .await?
            .context("worker did not obtain actual work lease")?;
        let slot_is_leased = {
            accepted
                .work
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .is_none()
        };
        ensure!(
            slot_is_leased,
            "worker did not move actual Work out of its slot"
        );
        ensure!(
            !h.host.control.retire_restored(key, &accepted, |_| true),
            "retirement dropped a record whose actual Work was still leased",
        );
        let task = worker.take().context("leased worker absent")?;
        task.abort();
        let joined = task.await;
        ensure!(
            matches!(joined, Err(error) if error.is_cancelled()),
            "worker was not actually aborted"
        );
        let observed = bounded(async {
            loop {
                let observed = receive.borrow_and_update().clone();
                if let Some(observed) = observed {
                    return Ok::<_, anyhow::Error>(observed);
                }
                receive
                    .changed()
                    .await
                    .context("accepted work observer closed")?;
            }
        })
        .await??;
        ensure!(
            matches!(observed, Observation::Unobserved),
            "abort reported recipe success"
        );
        let mut restored = bounded(accepted.lease()).await?;
        let _actual_work = restored.work()?;
        let same_binding = {
            let registry = h
                .host
                .control
                .registry
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            registry
                .bindings
                .get(&issued_key)
                .is_some_and(|(binding, _)| Arc::ptr_eq(binding, &issued))
        };
        ensure!(
            same_binding,
            "abort dropped/replaced Work's actual issued private binding"
        );
        drop(restored);
        let removed = {
            h.host
                .control
                .registry
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .operations
                .remove(&key)
        };
        drop(removed);
        drop(accepted);
        let binding_retired = {
            !h.host
                .control
                .registry
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .bindings
                .contains_key(&issued_key)
        };
        ensure!(
            binding_retired,
            "the same binding did not retire when restored Work finally dropped"
        );
        Ok(())
    }
    .await;
    // A failing assertion still aborts/joins the actual holder before existing
    // host cleanup tries to lease retained work; it cannot strand this test.
    if let Some(task) = worker {
        task.abort();
        let _ = task.await;
    }
    let cleanup = h.shutdown().await;
    result.and(cleanup)
}
