// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Local actual-store delivery custody. A distinct real notification gets a
//! distinct allocation even when two output chunks contain identical text.
//! No pointer/sequence is sent to Source/Native or used as permission.
use super::*;
use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;
use std::collections::HashMap;
#[derive(Debug)]
pub(in crate::app) struct Receipt {
    owner: Arc<()>,
    sequence: u64,
}
#[derive(Debug)]
pub(in crate::app) struct Store {
    owner: Arc<()>,
    next: u64,
    input: Option<usize>,
    current: Option<(usize, Arc<Receipt>)>,
    buffered: HashMap<usize, Arc<Receipt>>,
    faulted: bool,
}
impl Default for Store {
    fn default() -> Self {
        Self {
            owner: Arc::new(()),
            next: 0,
            input: None,
            current: None,
            buffered: HashMap::new(),
            faulted: false,
        }
    }
}
fn address(value: &ServerNotification) -> usize {
    std::ptr::from_ref(value) as usize
}
fn command(notification: &ServerNotification) -> bool {
    match notification {
        ServerNotification::ItemStarted(x) => {
            matches!(x.item.payload, ThreadItemPayload::CommandExecution { .. })
        }
        ServerNotification::ItemCompleted(x) => {
            matches!(x.item.payload, ThreadItemPayload::CommandExecution { .. })
        }
        ServerNotification::CommandExecutionOutputDelta(_)
        | ServerNotification::TurnCompleted(_) => true,
        _ => false,
    }
}
impl Store {
    pub(in crate::app) fn begin(&mut self, notification: &ServerNotification) {
        self.input = command(notification).then(|| address(notification));
        self.current = None;
    }
    pub(in crate::app) fn buffered(
        &mut self,
        notification: &ServerNotification,
        previous: &std::collections::VecDeque<ThreadBufferedEvent>,
    ) {
        if !command(notification) {
            return;
        }
        // Prune before installing the NEW allocation. Freed Box address reuse
        // can never inherit its predecessor's receipt. Rebase merely moves the
        // original boxes and does not invoke this new-delivery constructor.
        self.buffered.retain(|pointer,_|previous.iter().any(|event|matches!(event,ThreadBufferedEvent::Notification(value) if address(value)==*pointer)));
        let Some(next) = self.next.checked_add(1) else {
            self.faulted = true;
            return;
        };
        self.next = next;
        let receipt = Arc::new(Receipt {
            owner: self.owner.clone(),
            sequence: next,
        });
        self.buffered.insert(address(notification), receipt.clone());
        self.current = self.input.map(|input| (input, receipt));
    }
    pub(in crate::app) fn current(
        &self,
        notification: &ServerNotification,
    ) -> Result<Option<Arc<Receipt>>> {
        if self.faulted {
            bail!("actual side delivery sequence exhausted")
        }
        Ok(self
            .current
            .as_ref()
            .filter(|(input, _)| *input == address(notification))
            .map(|(_, receipt)| receipt.clone()))
    }
    pub(in crate::app) fn stored(
        &self,
        event: &ThreadBufferedEvent,
    ) -> Result<Option<Arc<Receipt>>> {
        if self.faulted {
            bail!("actual side delivery sequence unavailable")
        }
        let ThreadBufferedEvent::Notification(notification) = event else {
            return Ok(None);
        };
        if !command(notification) {
            return Ok(None);
        }
        self.buffered
            .get(&address(notification))
            .cloned()
            .map(Some)
            .ok_or_else(|| eyre!("original command notification lost actual store delivery"))
    }
}
#[derive(Debug, Default)]
pub(in crate::app) struct Applied {
    owner: Option<Arc<()>>,
    through: u64,
    failed: bool,
}
impl Applied {
    pub(in crate::app) fn fail(&mut self) {
        self.failed = true;
    }
    pub(in crate::app) fn check(&self) -> Result<()> {
        if self.failed {
            bail!("actual side command application remains unknown")
        }
        Ok(())
    }
    pub(in crate::app) fn newer(&self, receipt: &Receipt) -> Result<bool> {
        self.check()?;
        if self
            .owner
            .as_ref()
            .is_some_and(|owner| !Arc::ptr_eq(owner, &receipt.owner))
        {
            bail!("side command changed actual event store")
        }
        Ok(receipt.sequence > self.through)
    }
    pub(in crate::app) fn finish(&mut self, receipt: &Receipt) {
        self.owner = Some(receipt.owner.clone());
        self.through = receipt.sequence;
    }
}

#[cfg(test)]
#[path = "owned_local_side_delivery_tests.rs"]
mod tests;
