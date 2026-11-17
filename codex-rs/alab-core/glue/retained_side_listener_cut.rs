// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! SAME original listener's actual pre-occurrence live item set. This holds
//! no subscriber, Core lease, Native role or serialized identity constructor.
use super::*;
use codex_app_server_protocol::JSONRPCErrorError;
use std::collections::HashSet;
#[derive(Clone, Default)]
pub(crate) struct Cut {
    pub(crate) before: HashSet<(String, String)>,
}
pub(crate) struct State {
    receipt: Option<(ThreadId, std::sync::Arc<()>, std::sync::Arc<Cut>)>,
    changed: watch::Sender<()>,
    unavailable: bool,
}
impl Default for State {
    fn default() -> Self {
        Self {
            receipt: None,
            changed: watch::channel(()).0,
            unavailable: false,
        }
    }
}
impl State {
    pub(super) fn clear(&mut self) {
        self.unavailable = self.receipt.is_some();
        self.receipt = None;
        self.changed.send_replace(());
    }
    pub(super) fn observe(
        &mut self,
        thread: ThreadId,
        receipt: &std::sync::Arc<()>,
        active: Option<Turn>,
        live: Result<Vec<codex_app_server_protocol::ItemStartedNotification>, JSONRPCErrorError>,
    ) {
        let mut before = HashSet::new();
        if let Some(turn) = active {
            for item in turn.items {
                before.insert((turn.id.clone(), item.payload.id().into()));
            }
        }
        // An over-budget original live snapshot must stay unavailable, rather
        // than dropping part of its real before-set and later showing it anew.
        let Ok(live) = live else {
            self.unavailable = true;
            self.receipt = None;
            self.changed.send_replace(());
            return;
        };
        for item in live {
            before.insert((item.turn_id, item.item.payload.id().into()));
        }
        self.unavailable = false;
        self.receipt = Some((thread, receipt.clone(), std::sync::Arc::new(Cut { before })));
        self.changed.send_replace(());
    }
    pub(crate) fn observation(
        &self,
        thread: ThreadId,
        actual: &codex_core::RecordedSideBoundary,
    ) -> Result<Option<std::sync::Arc<Cut>>, JSONRPCErrorError> {
        if self.unavailable {
            return Err(crate::error_code::invalid_request(
                "original Side listener cut unavailable",
            ));
        }
        match &self.receipt {
            Some((id, receipt, cut)) if *id == thread && actual.same_delivery(receipt) => {
                Ok(Some(cut.clone()))
            }
            Some(_) => Err(crate::error_code::invalid_request(
                "original Side listener occurrence changed",
            )),
            None => Ok(None),
        }
    }
    pub(crate) fn subscribe(&self) -> watch::Receiver<()> {
        self.changed.subscribe()
    }
}
impl Cut {
    pub(crate) fn filter_turns(&self, turns: &mut [Turn]) {
        for turn in turns {
            turn.items.retain(|item| {
                !self
                    .before
                    .contains(&(turn.id.clone(), item.payload.id().to_owned()))
            });
        }
    }
    pub(crate) fn filter_live(
        &self,
        items: &mut Vec<codex_app_server_protocol::ItemStartedNotification>,
    ) {
        items.retain(|item| {
            !self
                .before
                .contains(&(item.turn_id.clone(), item.item.payload.id().to_owned()))
        });
    }
}
