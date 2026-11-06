// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Bind quota recovery to the executing host without rewriting saved provenance.

use codex_alab_core::quota::ExecQuotaMember;
use codex_alab_core::quota::HeadlessExec;
use codex_extension_api::ExtensionFuture;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::ThreadLifecycleContributor;
use codex_extension_api::ThreadStartInput;
use codex_extension_api::TurnLifecycleContributor;
use codex_extension_api::TurnStartInput;
use std::sync::Arc;

pub(super) fn install<C: Sync>(builder: &mut ExtensionRegistryBuilder<C>, runtime_is_exec: bool) {
    install_for(codex_alab_core::is_alab_binary(), builder, runtime_is_exec);
}

fn install_for<C: Sync>(
    active: bool,
    builder: &mut ExtensionRegistryBuilder<C>,
    runtime_is_exec: bool,
) {
    if !active {
        return;
    }
    let host = Arc::new(Host { runtime_is_exec });
    builder.thread_lifecycle_contributor(host.clone());
    builder.turn_lifecycle_contributor(host);
}

struct Host {
    runtime_is_exec: bool,
}

impl<C: Sync> ThreadLifecycleContributor<C> for Host {
    fn on_thread_start<'a>(&'a self, input: ThreadStartInput<'a, C>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if self.runtime_is_exec {
                input.thread_store.insert(ExecQuotaMember);
            } else {
                input.thread_store.remove::<ExecQuotaMember>();
            }
            // A resumed root keeps its original source. Internal tasks and child
            // agents keep their separate lifecycle even under an exec host.
            // A cold-restored member retains its explicit recovery provenance.
            if self.runtime_is_exec
                && (!input.session_source.is_non_root_agent()
                    || input
                        .thread_store
                        .get::<codex_core::ExecRestoredMember>()
                        .is_some())
            {
                let _ = input.thread_store.insert(HeadlessExec);
            } else {
                let _ = input.thread_store.remove::<HeadlessExec>();
            }
        })
    }
}

impl TurnLifecycleContributor for Host {
    fn on_turn_start<'a>(&'a self, input: TurnStartInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if self.runtime_is_exec && input.thread_store.get::<ExecQuotaMember>().is_some() {
                input.turn_store.insert(ExecQuotaMember);
            } else {
                input.turn_store.remove::<ExecQuotaMember>();
            }
            // The default BeforeTaskRegistration phase also covers compaction.
            if self.runtime_is_exec && input.thread_store.get::<HeadlessExec>().is_some() {
                let _ = input.turn_store.insert(HeadlessExec);
            } else {
                let _ = input.turn_store.remove::<HeadlessExec>();
            }
        })
    }
}

#[cfg(test)]
#[path = "quota_runtime_host_tests.rs"]
mod tests;
