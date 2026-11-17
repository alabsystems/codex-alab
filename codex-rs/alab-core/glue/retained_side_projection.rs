// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! SAME accepted Side occurrence -> actual original listener cut -> canonical
//! local display. No metadata, empty history or file-selected substitute.
use super::source_selection::SourceProjection;
use super::*;
use crate::thread_state::alab_side_boundary::Cut;
#[path = "retained_side_display.rs"]
mod display;
// The physical retained Remote client accepts 128 MiB messages. Read capacity
// denial is an error, never partial history or a force-persist fallback.
const MAX_LOCAL_BYTES: usize = 128 << 20;
impl Projection {
    fn side_source(&self) -> Option<&Arc<SourceProjection>> {
        match &self.frontier {
            PhysicalFrontier::Source(source) => Some(source),
            _ => None,
        }
    }
    pub(super) async fn wait_side_listener(
        &self,
        binding: &Binding,
    ) -> Result<(), JSONRPCErrorError> {
        let Some(source) = self.side_source() else {
            return Ok(());
        };
        let Some(boundary) = source.side_boundary() else {
            return Ok(());
        };
        self.primary.recheck().await?;
        let id = self.primary.thread_id();
        let actual = self.primary.thread();
        let state = binding
            .processor
            .thread_state_manager
            .thread_state(id)
            .await;
        binding
            .processor
            .ensure_listener_task_running(id, actual.clone(), state.clone())
            .await?;
        let mut changed = state.lock().await.alab_side_boundary.subscribe();
        loop {
            boundary
                .check_owner(actual)
                .map_err(|e| invalid_request(e.to_string()))?;
            let cut = state
                .lock()
                .await
                .alab_side_boundary
                .observation(id, &boundary)?;
            if let Some(cut) = cut {
                source.store_side_cut(cut)?;
                return Ok(());
            }
            tokio::select! {
                ()=binding.input.withdrawn()=>return Err(invalid_request("Side listener owner withdrew")),
                ()=actual.wait_until_terminated()=>return Err(invalid_request("actual Side Core terminated before listener cut")),
                changed=changed.changed()=>changed.map_err(|_|invalid_request("actual Side listener observation unavailable"))?,
            }
        }
    }
    pub(super) fn side_cut(&self) -> Result<Option<Arc<Cut>>, JSONRPCErrorError> {
        self.side_source()
            .map_or(Ok(None), |source| source.side_cut())
    }
    pub(super) async fn side_turns(
        &self,
        actual: &CodexThread,
        mode: ThreadHistoryMode,
    ) -> Result<Option<Vec<Turn>>, JSONRPCErrorError> {
        let Some(source) = self.side_source() else {
            return Ok(None);
        };
        let Some(boundary) = source.side_boundary() else {
            return Ok(None);
        };
        let cut = source
            .side_cut()?
            .ok_or_else(|| invalid_request("ordered Side listener cut unavailable"))?;
        let records = boundary
            .canonical_local_items(actual, MAX_LOCAL_BYTES)
            .await
            .map_err(|e| invalid_request(e.to_string()))?;
        let display = display::build(actual, &boundary, &records, mode)?;
        let mut turns = display.turns;
        cut.filter_turns(&mut turns);
        let mut before = display.before;
        before.extend(cut.before.iter().cloned());
        let mut before=before.into_iter().map(|(turn_id,item_id)|crate::outgoing_message::alab_source_selection_claim::side_display::SideDisplayItem {turn_id,item_id}).collect::<Vec<_>>();
        before.sort();
        let data=crate::outgoing_message::alab_source_selection_claim::side_display::SideDisplayBoundary {schema:1,occurrence:boundary.marker_data().token_data().into(),before};
        if !data.valid() {
            return Err(invalid_request(
                "actual Side display boundary exceeds response capacity",
            ));
        }
        source.store_side_display(data)?;
        Ok(Some(turns))
    }
    pub(crate) fn filter_side_live(
        &self,
        items: &mut Vec<codex_app_server_protocol::ItemStartedNotification>,
    ) -> Result<(), JSONRPCErrorError> {
        if let Some(cut) = self.side_cut()? {
            cut.filter_live(items)
        }
        Ok(())
    }
    pub(super) fn filter_side_response(
        &self,
        response: &mut ThreadResumeResponse,
    ) -> Result<(), JSONRPCErrorError> {
        if let Some(cut) = self.side_cut()? {
            cut.filter_turns(&mut response.thread.turns);
            if let Some(page) = &mut response.initial_turns_page {
                cut.filter_turns(&mut page.data);
            }
        }
        Ok(())
    }
}

impl Projection {
    pub(crate) async fn fail_side_listener(
        &self,
        error: JSONRPCErrorError,
        outgoing: &Arc<OutgoingMessageSender>,
    ) {
        self.uncertain.store(true, Ordering::Release);
        if let Some(binding) = self.binding.upgrade() {
            binding
                .input
                .fail(std::io::Error::other("actual Side listener replay failed"));
        }
        self.result.send_replace(Some(Err(error)));
        self.tail(false, outgoing).await;
        self.tail_finished.store(true, Ordering::Release);
    }
}
