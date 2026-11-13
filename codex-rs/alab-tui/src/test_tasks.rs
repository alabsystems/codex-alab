// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Task-lifetime context for bounded native fixture failures.

#![cfg(feature = "test-support")]

use codex_alab_core::session::Shared;
use std::borrow::Borrow;
use std::time::Duration;

/// Observe completion on a blocking worker so the fixture's current-thread
/// runtime remains free to serve the actual launch probe. The 12-second budget
/// matches the existing observer margin around the client's two 5-second calls.
/// It begins in the worker; queueing time is additional. Completion alone does
/// not prove success: callers must still inspect the launch witness.
pub async fn wait_for_launch_probe(
    shared: impl Borrow<Shared> + Send + 'static,
) -> Result<bool, tokio::task::JoinError> {
    wait_for_launch_probe_with_budget(shared, Duration::from_secs(12)).await
}

async fn wait_for_launch_probe_with_budget(
    shared: impl Borrow<Shared> + Send + 'static,
    budget: Duration,
) -> Result<bool, tokio::task::JoinError> {
    tokio::task::spawn_blocking(move || shared.borrow().wait_launch_probed(budget)).await
}

#[derive(Debug)]
pub struct ProxyClosureTimeout {
    elapsed: tokio::time::error::Elapsed,
    proxy_finished: bool,
}

impl std::fmt::Display for ProxyClosureTimeout {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} while waiting for backend closure (proxy_finished={})",
            self.elapsed, self.proxy_finished
        )
    }
}

impl std::error::Error for ProxyClosureTimeout {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.elapsed)
    }
}

pub fn proxy_closure_timeout(
    elapsed: tokio::time::error::Elapsed,
    proxy_finished: bool,
) -> ProxyClosureTimeout {
    ProxyClosureTimeout {
        elapsed,
        proxy_finished,
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use codex_alab_aterm::Client;
    use codex_alab_aterm::Error;
    use codex_alab_aterm::Options;
    use codex_alab_aterm::fake::FakeAterm;
    use codex_alab_aterm::fake::Mode;
    use codex_alab_core::session::HandPid;
    use codex_alab_core::session::HumanSeen;
    use codex_alab_core::session::LaunchWitness;
    use std::sync::Arc;

    fn witness(hand: Option<String>, server_pid: Option<u32>) -> LaunchWitness {
        LaunchWitness {
            hand,
            human: HumanSeen::Unknown,
            read_at_ms: codex_alab_core::session::now_ms(),
            server_pid,
            hand_pid: HandPid::Absent,
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn launch_probe_wait_leaves_the_runtime_free_for_authenticated_rpc() {
        let root = crate::test_paths::short_tempdir().expect("private fake host root");
        let fake = FakeAterm::start(root.path(), Mode::Current)
            .await
            .expect("fake host");
        fake.set_status("hand", "fixture-probe");
        let shared = Arc::new(Shared::new());
        let publish = Arc::clone(&shared);
        let endpoint = fake.endpoint();
        // This task cannot connect or publish until the current thread yields.
        let probe = tokio::spawn(async move {
            let client = Client::connect(endpoint, Options::default())
                .await
                .expect("authenticated connection");
            let status = client.status().await.expect("actual status response");
            assert_eq!(status.hand.as_deref(), Some("fixture-probe"));
            assert!(publish.set_launch_witness(Some(witness(status.hand, client.server_pid()))));
            client.close();
        });
        assert!(
            wait_for_launch_probe(Arc::clone(&shared))
                .await
                .expect("joined waiter")
        );
        probe.await.expect("RPC probe task");
        assert_eq!(
            shared.launch_witness().unwrap().hand.as_deref(),
            Some("fixture-probe")
        );
        let requests = fake.requests();
        assert!(requests.iter().any(|request| request == "version"));
        assert!(requests.iter().any(|request| request.ends_with(" status")));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn completed_launch_probe_is_observed_without_another_publication() {
        let shared = Arc::new(Shared::new());
        let expected = witness(Some("already-observed".into()), None);
        assert!(shared.set_launch_witness(Some(expected.clone())));
        assert!(
            wait_for_launch_probe(Arc::clone(&shared))
                .await
                .expect("joined waiter")
        );
        assert_eq!(shared.launch_witness(), Some(expected));
        assert_eq!(Arc::strong_count(&shared), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn failed_authenticated_probe_completes_without_a_launch_witness() {
        let root = crate::test_paths::short_tempdir().expect("private fake host root");
        let fake = FakeAterm::start(root.path(), Mode::Current)
            .await
            .expect("fake host");
        fake.drop_next_reply("version");
        let shared = Arc::new(Shared::new());
        let publish = Arc::clone(&shared);
        let endpoint = fake.endpoint();
        let probe = tokio::spawn(async move {
            let error = match Client::connect(endpoint, Options::default()).await {
                Err(error) => error,
                Ok(_) => panic!("dropped version response cannot authenticate"),
            };
            assert!(publish.set_launch_witness(None));
            error
        });
        assert!(
            wait_for_launch_probe(Arc::clone(&shared))
                .await
                .expect("joined waiter")
        );
        assert!(matches!(
            probe.await.expect("failed RPC probe task"),
            Error::Closed
        ));
        assert!(shared.launch_probed());
        assert!(shared.launch_witness().is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unfinished_launch_probe_times_out_and_releases_owned_state() {
        let shared = Arc::new(Shared::new());
        let weak = Arc::downgrade(&shared);
        assert!(
            !wait_for_launch_probe_with_budget(Arc::clone(&shared), Duration::from_millis(10))
                .await
                .expect("joined waiter")
        );
        assert!(!shared.launch_probed());
        assert_eq!(Arc::strong_count(&shared), 1);
        drop(shared);
        assert!(weak.upgrade().is_none());
    }
}
