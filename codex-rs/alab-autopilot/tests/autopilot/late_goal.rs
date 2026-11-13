// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! THE GOAL EXTENSION AS THE APP SERVER INSTALLS IT, for a test registry built before its
//! thread manager. `app-server/src/extensions.rs` installs `codex_goal_extension` right after
//! the autopilot, so in the codex binary a goal set active again continues an idle thread at
//! once, inside `GoalService::set_thread_goal`'s runtime effects (`continue_if_idle`), and an
//! active goal is stopped usage-limited at a usage or quota wall (`on_turn_error`). Without it
//! a test resumes a goal that nothing continues, and an act booked after the resume looks
//! right where the binary's goal turn has already begun (review of CX-B, CXR-1).
//!
//! The extension needs the thread manager, which `TestCodex` makes after the registry, so the
//! harness registers this forwarder where the binary has the extension (after the autopilot),
//! then installs the extension against the built manager and starts its runtime on the
//! primary thread ([`LateGoal::bind`]). The goal's tools are not offered to the model.

use std::sync::Arc;
use std::sync::OnceLock;

use codex_analytics::AnalyticsEventsClient;
use codex_core::config::Config;
use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionFuture;
use codex_extension_api::ExtensionRegistry;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::ThreadIdleInput;
use codex_extension_api::ThreadLifecycleContributor;
use codex_extension_api::ThreadStartInput;
use codex_extension_api::ThreadStopInput;
use codex_extension_api::TurnAbortInput;
use codex_extension_api::TurnErrorInput;
use codex_extension_api::TurnLifecycleContributor;
use codex_extension_api::TurnStartInput;
use codex_extension_api::TurnStopInput;
use codex_goal_extension::GoalExtensionConfig;
use codex_goal_extension::GoalService;
use codex_protocol::items::TurnItem;
use codex_protocol::protocol::SessionSource;
use core_test_support::test_codex::TestCodex;

/// The goal extension's thread and turn hooks, once [`LateGoal::bind`] has installed it.
#[derive(Default)]
pub(crate) struct LateGoal {
    registry: OnceLock<ExtensionRegistry<Config>>,
}

impl LateGoal {
    pub(crate) fn register(self: &Arc<Self>, builder: &mut ExtensionRegistryBuilder<Config>) {
        builder.thread_lifecycle_contributor(Arc::clone(self) as _);
        builder.turn_lifecycle_contributor(Arc::clone(self) as _);
    }

    /// Install the goal extension on `test`'s thread manager and goal database, sharing the
    /// autopilot's `goals`, and start its runtime on the primary thread, as the thread's own
    /// start does in the binary.
    pub(crate) async fn bind(&self, test: &TestCodex, goals: Arc<GoalService>) {
        let db = test
            .codex
            .state_db()
            .unwrap_or_else(|| panic!("the goal database"));
        let mut builder = ExtensionRegistryBuilder::<Config>::new();
        codex_goal_extension::install_with_backend(
            &mut builder,
            db,
            AnalyticsEventsClient::disabled(),
            /*metrics_client*/ None,
            Arc::downgrade(&test.thread_manager),
            goals,
            |config: &Config| GoalExtensionConfig {
                enabled: true,
                max_goal_token_budget: config.max_goal_token_budget,
            },
        );
        let registry = builder.build();
        let session = ExtensionData::new(test.session_configured.thread_id.to_string());
        for contributor in registry.thread_lifecycle_contributors() {
            contributor
                .on_thread_start(ThreadStartInput {
                    config: &test.config,
                    session_source: &SessionSource::Exec,
                    persistent_thread_state_available: true,
                    environments: &[],
                    mcp_resource_client: None,
                    extension_metrics: None,
                    session_store: &session,
                    thread_store: test.codex.thread_extension_data(),
                })
                .await;
        }
        assert!(
            self.registry.set(registry).is_ok(),
            "the goal extension is bound once"
        );
    }

    fn threads(&self) -> &[Arc<dyn ThreadLifecycleContributor<Config>>] {
        self.registry
            .get()
            .map_or(&[], ExtensionRegistry::thread_lifecycle_contributors)
    }

    fn turns(&self) -> &[Arc<dyn TurnLifecycleContributor>] {
        self.registry
            .get()
            .map_or(&[], ExtensionRegistry::turn_lifecycle_contributors)
    }
}

impl ThreadLifecycleContributor<Config> for LateGoal {
    fn on_thread_start<'a>(
        &'a self,
        input: ThreadStartInput<'a, Config>,
    ) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            for contributor in self.threads() {
                contributor
                    .on_thread_start(ThreadStartInput {
                        config: input.config,
                        session_source: input.session_source,
                        persistent_thread_state_available: input.persistent_thread_state_available,
                        environments: input.environments,
                        mcp_resource_client: input.mcp_resource_client.clone(),
                        extension_metrics: input.extension_metrics.clone(),
                        session_store: input.session_store,
                        thread_store: input.thread_store,
                    })
                    .await;
            }
        })
    }

    fn on_thread_idle<'a>(&'a self, input: ThreadIdleInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            for contributor in self.threads() {
                contributor
                    .on_thread_idle(ThreadIdleInput {
                        cause: input.cause,
                        session_store: input.session_store,
                        thread_store: input.thread_store,
                    })
                    .await;
            }
        })
    }

    fn on_thread_stop<'a>(&'a self, input: ThreadStopInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            for contributor in self.threads() {
                contributor
                    .on_thread_stop(ThreadStopInput {
                        session_store: input.session_store,
                        thread_store: input.thread_store,
                    })
                    .await;
            }
        })
    }
}

impl TurnLifecycleContributor for LateGoal {
    fn on_turn_start<'a>(&'a self, input: TurnStartInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            for contributor in self.turns() {
                contributor
                    .on_turn_start(TurnStartInput {
                        turn_id: input.turn_id,
                        collaboration_mode: input.collaboration_mode,
                        token_usage_at_turn_start: input.token_usage_at_turn_start,
                        session_store: input.session_store,
                        thread_store: input.thread_store,
                        turn_store: input.turn_store,
                    })
                    .await;
            }
        })
    }

    fn on_item_completed<'a>(
        &'a self,
        thread_store: &'a ExtensionData,
        turn_store: &'a ExtensionData,
        item: &'a TurnItem,
    ) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            for contributor in self.turns() {
                contributor
                    .on_item_completed(thread_store, turn_store, item)
                    .await;
            }
        })
    }

    fn on_turn_stop<'a>(&'a self, input: TurnStopInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            for contributor in self.turns() {
                contributor
                    .on_turn_stop(TurnStopInput {
                        session_store: input.session_store,
                        thread_store: input.thread_store,
                        turn_store: input.turn_store,
                    })
                    .await;
            }
        })
    }

    fn on_turn_abort<'a>(&'a self, input: TurnAbortInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            for contributor in self.turns() {
                contributor
                    .on_turn_abort(TurnAbortInput {
                        reason: input.reason.clone(),
                        session_store: input.session_store,
                        thread_store: input.thread_store,
                        turn_store: input.turn_store,
                    })
                    .await;
            }
        })
    }

    fn on_turn_error<'a>(&'a self, input: TurnErrorInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            for contributor in self.turns() {
                contributor
                    .on_turn_error(TurnErrorInput {
                        turn_id: input.turn_id,
                        error: input.error.clone(),
                        error_details: input.error_details,
                        session_store: input.session_store,
                        thread_store: input.thread_store,
                        turn_store: input.turn_store,
                    })
                    .await;
            }
        })
    }
}
