// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Retained session callbacks must not own a retired runtime's native descriptors.

use std::sync::Arc;
use std::time::Duration;

use codex_alab_core::mail::TriggerKind;
use codex_alab_core::session::Shared;

use super::Supervisor;
use super::SupervisorOptions;
use crate::extension::native_test;

fn descriptor_count() -> std::io::Result<usize> {
    #[cfg(target_os = "macos")]
    let directory = "/dev/fd";
    #[cfg(target_os = "linux")]
    let directory = "/proc/self/fd";
    std::fs::read_dir(directory)?.try_fold(0, |count, entry| entry.map(|_| count + 1))
}

#[test]
fn retained_callbacks_do_not_keep_retired_runtime_descriptors_open() {
    // The existing exact-child collector keeps other libtests out of this process's
    // descriptor count. No Core session, fake server or descriptor limit is needed.
    native_test(|| async {
        tokio::task::spawn_blocking(|| {
            let make_runtime = || {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("owned runtime")
            };
            // Initialize any process-wide Tokio resources before taking the baseline.
            drop(make_runtime());
            let before = descriptor_count().expect("this process's descriptor baseline");
            let mut retained = Vec::new();
            for _ in 0..32 {
                let runtime = make_runtime();
                let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
                let supervisor = {
                    let _entered = runtime.enter();
                    let supervisor = Supervisor::new(SupervisorOptions {
                        shared,
                        endpoint: Arc::new(|| None),
                        client: Default::default(),
                    });
                    // Direct installation is also used by the exit-order fixtures.
                    supervisor.install_release();
                    supervisor.install_context();
                    supervisor.install_mail();
                    supervisor
                };
                let weak = Arc::downgrade(&supervisor);
                runtime.spawn({
                    let supervisor = Arc::clone(&supervisor);
                    async move {
                        std::future::pending::<()>().await;
                        drop(supervisor);
                    }
                });
                drop(supervisor);
                runtime.block_on(tokio::task::yield_now());
                assert!(
                    weak.upgrade().is_some(),
                    "the pending task owns its supervisor"
                );
                drop(runtime);
                assert!(
                    weak.upgrade().is_none(),
                    "runtime shutdown drops its task owner"
                );
                retained.push(shared);
            }
            assert_eq!(
                descriptor_count().expect("this process's descriptors after runtime shutdown"),
                before,
                "retained callbacks must not retain each retired I/O driver"
            );
            for shared in retained {
                assert!(shared.release_claim(Duration::ZERO));
                shared.set_claim(Some("retired-owner".into()));
                assert!(shared.terminal_context(Duration::ZERO).is_none());
                assert_eq!(
                    shared
                        .read_mail(TriggerKind::Task, 1, Duration::ZERO)
                        .unwrap_err(),
                    "the supervisor is gone"
                );
            }
        })
        .await
        .expect("runtime ownership check");
    });
}
