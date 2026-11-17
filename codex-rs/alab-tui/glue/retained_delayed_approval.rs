// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! The original delayed queue and typing deadline. Decoded DATA cannot create
//! a request, private item, feature policy or a new modal. Replayed rows retain
//! those resources; only their same actual callback owners permit restoration.
use super::*;
use crate::app::app_server_requests::alab_retained_owners::ApprovalOwner;
use crate::bottom_pane::alab_retained_approval::RequestData;
use codex_alab_tui::upgrade_editor_clock::Snapshot as ClockSnapshot;
use serde::Deserialize;
use serde::Serialize;
use serde_json::value::RawValue;
use std::io;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Data {
    schema: u32,
    requests: Vec<RequestData>,
    last_activity: Option<ClockSnapshot>,
    generation: u64,
}
/// Actual composer activity is recorded independently of every DATA decoder.
/// A late view restore may not reset a new typing pause, even for unchanged text.
struct ActivityState {
    value: Option<u64>,
    restored_clock: Option<String>,
    // Some(None) is the original explicit no-pause state while actual panes
    // share custody. None keeps the original native field for an unlinked pane.
    shared_activity: Option<Option<Instant>>,
}
pub(crate) struct ActivityGeneration {
    // Local mutation evidence is never shared or serialized. Pump typing must
    // not make an untouched destination look edited, and blank Vim edits must
    // not look pristine merely because the text stayed empty.
    local: Option<u64>,
    actual: Option<std::sync::Arc<std::sync::Mutex<ActivityState>>>,
}
impl Default for ActivityGeneration {
    fn default() -> Self {
        Self {
            local: Some(0),
            actual: Some(std::sync::Arc::new(std::sync::Mutex::new(ActivityState {
                value: Some(0),
                restored_clock: None,
                shared_activity: None,
            }))),
        }
    }
}
impl ActivityGeneration {
    fn state(&self) -> io::Result<std::sync::MutexGuard<'_, ActivityState>> {
        Ok(self
            .actual
            .as_ref()
            .ok_or_else(|| invalid("actual approval clock was handed off"))?
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner))
    }
    pub(crate) fn editor_event(&mut self) {
        self.local = self.local.and_then(|value| value.checked_add(1));
    }
    pub(crate) fn changed(&mut self, now: Instant) {
        self.editor_event();
        if let Ok(mut state) = self.state() {
            state.value = state.value.and_then(|v| v.checked_add(1));
            if state.shared_activity.is_some() {
                state.shared_activity = Some(Some(now));
            }
        }
    }
    pub(crate) fn local_value(&self) -> io::Result<u64> {
        self.local
            .ok_or_else(|| invalid("local editor activity exhausted"))
    }
    fn value(&self) -> io::Result<u64> {
        self.state()?
            .value
            .ok_or_else(|| invalid("approval activity generation exhausted"))
    }
    pub(crate) fn activity(&self, native: Option<Instant>) -> Option<Instant> {
        self.state()
            .ok()
            .and_then(|state| state.shared_activity)
            .unwrap_or(native)
    }
}
fn invalid(value: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, value.to_string())
}
impl Data {
    fn clock_data(&self) -> io::Result<String> {
        serde_json::to_string(&(&self.last_activity, self.generation)).map_err(invalid)
    }
    fn clock_already_installed(&self, pane: &BottomPane) -> io::Result<bool> {
        let _generation = pane.alab_delay_activity.value()?;
        let expected = self.clock_data()?;
        match pane.alab_delay_activity.state()?.restored_clock.as_deref() {
            Some(actual) if actual == expected => Ok(true),
            Some(_) => Err(invalid(
                "delayed original clock differs from the clock already installed on this pane",
            )),
            None => Ok(false),
        }
    }
    fn decode(raw: &RawValue, max: usize) -> io::Result<Self> {
        if raw.get().len() > max {
            return Err(invalid("delayed approval graph exceeds whole UI bound"));
        }
        let value: Self = serde_json::from_str(raw.get()).map_err(invalid)?;
        if value.schema != 1 || value.requests.len() > 256 {
            return Err(invalid("invalid delayed approval graph"));
        }
        if serde_json::to_value(&value).map_err(invalid)?
            != serde_json::from_str::<serde_json::Value>(raw.get()).map_err(invalid)?
        {
            return Err(invalid("delayed graph has omitted/foreign native fields"));
        }
        // Validate against the actual authenticated same-host clock before any
        // timer/state mutation. Missing activity means original no-pause state.
        if let Some(stamp) = &value.last_activity {
            let _original = stamp.restore().map_err(invalid)?;
        }
        Ok(value)
    }
}
pub(crate) fn requests_data(raw: &RawValue, max: usize) -> io::Result<Vec<RequestData>> {
    Ok(Data::decode(raw, max)?.requests)
}
pub(crate) fn editing_retaining(
    raw: &RawValue,
    keep: &[bool],
    max: usize,
) -> io::Result<Box<RawValue>> {
    let mut value = Data::decode(raw, max)?;
    if keep.len() != value.requests.len() {
        return Err(invalid(
            "delayed callback mask changed original queue count",
        ));
    }
    value.requests = value
        .requests
        .into_iter()
        .zip(keep)
        .filter_map(|(request, keep)| keep.then_some(request))
        .collect();
    serde_json::value::to_raw_value(&value).map_err(invalid)
}
pub(crate) struct PreparedDelayed(Data);
impl PreparedDelayed {
    pub(crate) fn decode(raw: &RawValue, max: usize) -> io::Result<Self> {
        Ok(Self(Data::decode(raw, max)?))
    }
    /// The actual constructor calls before draining its original callback
    /// events. Only a pristine native pane can be primed. Later real typing
    /// stays authoritative, including through repeated transfer observations.
    pub(crate) fn prime_before_original_replay(&self, pane: &mut BottomPane) -> io::Result<()> {
        if self.0.clock_already_installed(pane)? {
            return Ok(());
        }
        if !pane.delayed_approval_requests.is_empty() || !pane.view_stack.is_empty() {
            return Err(invalid(
                "original approval clock must precede callback replay",
            ));
        }
        self.restore_native_clock_once(pane)
    }
    pub(crate) fn validate_destination(&self, pane: &BottomPane) -> io::Result<()> {
        let primed = self.0.clock_already_installed(pane)?;
        if (!primed && pane.alab_delay_activity.value()? != 0)
            || self.0.requests.len() > pane.delayed_approval_requests.len()
        {
            return Err(invalid(
                "delayed original destination changed before restoration",
            ));
        }
        for (old, actual) in self.0.requests.iter().zip(&pane.delayed_approval_requests) {
            if !old.matches_native(&actual.request)? {
                return Err(invalid("delayed original request order/content changed"));
            }
        }
        Ok(())
    }
    pub(crate) fn install_bound(
        self,
        pane: &mut BottomPane,
        owners: &[ApprovalOwner<'_>],
    ) -> io::Result<()> {
        self.validate_destination(pane)?;
        if owners.len() != self.0.requests.len()
            || !owners
                .iter()
                .zip(&self.0.requests)
                .zip(&pane.delayed_approval_requests)
                .all(|((owner, request), actual)| {
                    owner.matches(request)
                        && actual
                            .alab_original_input
                            .as_ref()
                            .is_some_and(|item| owner.same_actual_item(item))
                })
        {
            return Err(invalid(
                "delayed graph lacks exact actual queued reply owners",
            ));
        }
        self.install_native_clock(pane)
    }
    fn install_native_clock(self, pane: &mut BottomPane) -> io::Result<()> {
        self.install_native_clock_at(pane, Instant::now())
    }
    fn install_native_clock_at(self, pane: &mut BottomPane, now: Instant) -> io::Result<()> {
        self.restore_native_clock_once(pane)?;
        // Keep the SAME actual queue, private item bindings and per-row Features.
        // The original timer owns promotion/FIFO-to-LIFO conversion; restoring
        // DATA never directly constructs a modal or a new callback.
        pane.maybe_show_delayed_approval_requests_at(now);
        Ok(())
    }
    fn restore_native_clock_once(&self, pane: &mut BottomPane) -> io::Result<()> {
        if self.0.clock_already_installed(pane)? {
            return Ok(());
        }
        if pane.alab_delay_activity.value()? != 0 {
            return Err(invalid(
                "new actual composer activity precedes original clock installation",
            ));
        }
        let activity = self
            .0
            .last_activity
            .as_ref()
            .map(ClockSnapshot::restore)
            .transpose()
            .map_err(invalid)?;
        let marker = self.0.clock_data()?;
        pane.last_composer_activity_at = activity;
        let mut state = pane.alab_delay_activity.state()?;
        state.value = Some(self.0.generation);
        state.restored_clock = Some(marker);
        if state.shared_activity.is_some() {
            state.shared_activity = Some(activity);
        }
        Ok(())
    }
}
impl BottomPane {
    fn alab_validate_clock_destination(&self) -> io::Result<()> {
        if self.alab_delay_activity.value()? != 0
            || self.alab_delay_activity.state()?.restored_clock.is_some()
            || self.last_composer_activity_at.is_some()
            || !self.delayed_approval_requests.is_empty()
            || !self.view_stack.is_empty()
        {
            return Err(invalid(
                "actual approval clock destination already processed input or replay",
            ));
        }
        Ok(())
    }
    /// Both actual panes can receive original events during startup. Share the
    /// SAME private clock until the original pump is finally consumed. The
    /// actual original activity method updates this resource from either pane.
    pub(crate) fn alab_share_delayed_clock_from(
        &mut self,
        source: &mut BottomPane,
    ) -> io::Result<()> {
        let _generation = source.alab_delay_activity.value()?;
        let actual = source
            .alab_delay_activity
            .actual
            .as_ref()
            .ok_or_else(|| invalid("original pump clock missing"))?;
        if self
            .alab_delay_activity
            .actual
            .as_ref()
            .is_some_and(|mine| std::sync::Arc::ptr_eq(mine, actual))
        {
            return Ok(());
        }
        self.alab_validate_clock_destination()?;
        if !source.delayed_approval_requests.is_empty() {
            return Err(invalid(
                "original pump still owns delayed callback resources",
            ));
        }
        let activity = source
            .alab_delay_activity
            .activity(source.last_composer_activity_at);
        actual
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .shared_activity = Some(activity);
        self.alab_delay_activity.actual = Some(actual.clone());
        self.last_composer_activity_at = activity;
        Ok(())
    }
    /// Final original pump consumption moves its remaining custody once. An
    /// earlier sharing step is recognized only by the same actual Arc.
    pub(crate) fn alab_transfer_delayed_clock_from(
        &mut self,
        source: &mut BottomPane,
    ) -> io::Result<()> {
        let _generation = source.alab_delay_activity.value()?;
        let actual = source
            .alab_delay_activity
            .actual
            .as_ref()
            .ok_or_else(|| invalid("original pump clock missing"))?;
        let already_shared = self
            .alab_delay_activity
            .actual
            .as_ref()
            .is_some_and(|mine| std::sync::Arc::ptr_eq(mine, actual));
        if !already_shared {
            self.alab_validate_clock_destination()?;
        }
        if !source.delayed_approval_requests.is_empty() {
            return Err(invalid(
                "original pump still owns delayed callback resources",
            ));
        }
        let activity = source
            .alab_delay_activity
            .activity(source.last_composer_activity_at);
        self.alab_delay_activity.actual = source.alab_delay_activity.actual.take();
        self.last_composer_activity_at = activity;
        source.last_composer_activity_at = None;
        Ok(())
    }
    pub(crate) fn alab_refresh_shared_activity(&mut self) {
        self.last_composer_activity_at = self
            .alab_delay_activity
            .activity(self.last_composer_activity_at);
    }
    pub(crate) fn alab_capture_delayed_approvals(&self, max: usize) -> io::Result<Box<RawValue>> {
        let value = Data {
            schema: 1,
            requests: self
                .delayed_approval_requests
                .iter()
                .map(|row| RequestData::capture(&row.request))
                .collect(),
            last_activity: self
                .alab_delay_activity
                .activity(self.last_composer_activity_at)
                .map(ClockSnapshot::capture)
                .transpose()
                .map_err(invalid)?,
            generation: self.alab_delay_activity.value()?,
        };
        let raw = serde_json::value::to_raw_value(&value).map_err(invalid)?;
        let _decoded = Data::decode(&raw, max)?;
        Ok(raw)
    }
}
#[cfg(test)]
#[path = "retained_delayed_approval_tests.rs"]
mod tests;
