// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Non-serializable original view/resource pairs. A decoded ViewData cannot
//! construct this wrapper or the actual reply items borrowed by installation.
use super::*;
use crate::app::app_server_requests::alab_retained_owners::ApprovalOwner;
use crate::app::app_server_requests::alab_retained_owners::UserInputOwner;
use crate::app::retained_original_input::Item;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ToolRequestUserInputParams;
use std::sync::Arc;

pub(crate) enum Requests {
    UserInput(Vec<ToolRequestUserInputParams>),
    Approval(Vec<crate::bottom_pane::alab_retained_approval::RequestData>),
}
impl Requests {
    pub(crate) fn len(&self) -> usize {
        match self {
            Self::UserInput(v) => v.len(),
            Self::Approval(v) => v.len(),
        }
    }
    pub(crate) fn thread_ids(&self) -> Vec<String> {
        match self {
            Self::UserInput(v) => v.iter().map(|v| v.thread_id.clone()).collect(),
            Self::Approval(v) => v.iter().map(|v| v.thread_id_data().to_string()).collect(),
        }
    }
}
pub(crate) enum Owners<'a> {
    UserInput(Vec<UserInputOwner<'a>>),
    Approval(Vec<ApprovalOwner<'a>>),
}
impl<'a> Owners<'a> {
    pub(crate) fn from_actual(
        requests: &Requests,
        items: &'a [Arc<Item>],
        client: &AppServerRequestHandle,
    ) -> io::Result<Self> {
        if requests.len() != items.len() {
            return Err(invalid("native view lost actual callback resource count"));
        }
        match requests {
            Requests::UserInput(expected) => {
                let owners = items
                    .iter()
                    .map(|item| UserInputOwner::from_actual_item(item, client))
                    .collect::<io::Result<Vec<_>>>()?;
                if !owners
                    .iter()
                    .zip(expected)
                    .all(|(owner, request)| owner.matches(request))
                {
                    return Err(invalid(
                        "native question differs from actual original resource",
                    ));
                }
                Ok(Self::UserInput(owners))
            }
            Requests::Approval(expected) => {
                let owners = items
                    .iter()
                    .map(|item| ApprovalOwner::from_actual(item, client))
                    .collect::<io::Result<Vec<_>>>()?;
                if !owners
                    .iter()
                    .zip(expected)
                    .all(|(owner, request)| owner.matches(request))
                {
                    return Err(invalid(
                        "native approval differs from actual original resource",
                    ));
                }
                Ok(Self::Approval(owners))
            }
        }
    }
    pub(crate) fn ids(&self) -> Vec<RequestId> {
        match self {
            Self::UserInput(v) => v.iter().map(|v| v.request_id_data().clone()).collect(),
            Self::Approval(v) => v.iter().map(|v| v.request_id_data().clone()).collect(),
        }
    }
}
pub(crate) struct Captured {
    data: ViewData,
    items: Vec<Arc<Item>>,
}
impl Captured {
    pub(crate) fn into_parts(self) -> (ViewData, Vec<Arc<Item>>) {
        (self.data, self.items)
    }
}
fn invalid(value: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, value.to_string())
}

impl ViewData {
    pub(crate) fn requests(&self, max_bytes: usize) -> io::Result<Requests> {
        match self {
            Self::UserInput { original } => {
                if original.get().len() > max_bytes {
                    return Err(invalid("native question exceeds capture budget"));
                }
                #[derive(Deserialize)]
                struct Data {
                    request: ToolRequestUserInputParams,
                    queue: std::collections::VecDeque<ToolRequestUserInputParams>,
                }
                let data: Data = serde_json::from_str(original.get()).map_err(invalid)?;
                Ok(Requests::UserInput(
                    std::iter::once(data.request).chain(data.queue).collect(),
                ))
            }
            Self::DelayedApproval { original } => Ok(Requests::Approval(
                crate::bottom_pane::alab_retained_delayed_approval::requests_data(
                    original, max_bytes,
                )?,
            )),
            Self::Approval { original } => Ok(Requests::Approval(
                crate::bottom_pane::alab_retained_approval::requests_data(original, max_bytes)?,
            )),
        }
    }
}
impl BottomPane {
    pub(crate) fn alab_capture_bound_view_stack(
        &self,
        max_bytes: usize,
    ) -> io::Result<(Vec<Captured>, Box<serde_json::value::RawValue>)> {
        let delayed_clock = self.alab_capture_delayed_approvals(max_bytes)?;
        let mut views = self
            .view_stack
            .iter()
            .map(|view| {
                let data = view.alab_capture_retained_view(max_bytes)?;
                let items = view.alab_original_input_items()?;
                Ok(Captured { data, items })
            })
            .collect::<io::Result<Vec<_>>>()?;
        if !self.delayed_approval_requests.is_empty() {
            let items = self
                .delayed_approval_requests
                .iter()
                .map(|row| {
                    row.alab_original_input
                        .clone()
                        .ok_or_else(|| invalid("delayed row has no genuine original reply owner"))
                })
                .collect::<io::Result<Vec<_>>>()?;
            views.push(Captured {
                data: ViewData::DelayedApproval {
                    original: delayed_clock.clone(),
                },
                items,
            });
        }
        Ok((views, delayed_clock))
    }
    pub(crate) fn alab_install_matching_bound_view(
        &mut self,
        data: ViewData,
        ids: &[RequestId],
        client: &AppServerRequestHandle,
        max_bytes: usize,
        max_generations: usize,
    ) -> io::Result<()> {
        if let ViewData::DelayedApproval { original } = data {
            return self.alab_install_delayed_bound(&original, ids, client, max_bytes);
        }
        let requests = data.requests(max_bytes)?;
        if let Requests::Approval(physical) = &requests {
            if physical.len() != ids.len() {
                return Err(invalid("native approval identity count changed"));
            }
            let order = std::iter::once(0)
                .chain((1..physical.len()).rev())
                .collect::<Vec<_>>();
            let fifo = order
                .iter()
                .map(|index| physical[*index].clone())
                .collect::<Vec<_>>();
            let fifo_ids = order
                .iter()
                .map(|index| ids[*index].clone())
                .collect::<Vec<_>>();
            self.alab_normalize_original_approval_rows(&fifo, &fifo_ids, client, max_bytes)?;
        }
        let mut candidates = self
            .view_stack
            .iter()
            .enumerate()
            .filter_map(|(index, view)| {
                view.alab_matches_bound_requests(&requests).then_some(index)
            });
        let index = candidates.next().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::WouldBlock,
                "actual original native view not installed",
            )
        })?;
        if candidates.next().is_some() {
            return Err(invalid("ambiguous actual native view"));
        }
        let view = self
            .view_stack
            .get_mut(index)
            .ok_or_else(|| invalid("actual original view moved"))?;
        let mut items = view.alab_original_input_items()?;
        items.truncate(requests.len());
        let owners = Owners::from_actual(&requests, &items, client)?;
        if owners.ids() != ids {
            return Err(invalid(
                "identical native parameters cannot inherit another original RPC",
            ));
        }
        view.alab_install_bound_view(data, &owners, &self.keymap, max_bytes, max_generations)
    }
}

impl BottomPane {
    pub(crate) fn alab_install_bound_view(
        &mut self,
        index: usize,
        data: ViewData,
        ids: &[RequestId],
        client: &AppServerRequestHandle,
        bytes: usize,
        generations: usize,
    ) -> io::Result<()> {
        if let ViewData::DelayedApproval { original } = data {
            return self.alab_install_delayed_bound(&original, ids, client, bytes);
        }
        let requests = data.requests(bytes)?;
        let view = self
            .view_stack
            .get_mut(index)
            .ok_or_else(|| invalid("actual native view index missing"))?;
        if !view.alab_matches_bound_requests(&requests) {
            return Err(invalid("actual native view changed kind/request"));
        }
        let mut items = view.alab_original_input_items()?;
        items.truncate(requests.len());
        let owners = Owners::from_actual(&requests, &items, client)?;
        if owners.ids() != ids {
            return Err(invalid("actual native view changed original RPC IDs"));
        }
        view.alab_install_bound_view(data, &owners, &self.keymap, bytes, generations)
    }
}

impl BottomPane {
    fn alab_install_delayed_bound(
        &mut self,
        raw: &serde_json::value::RawValue,
        ids: &[RequestId],
        client: &AppServerRequestHandle,
        bytes: usize,
    ) -> io::Result<()> {
        let expected =
            crate::bottom_pane::alab_retained_delayed_approval::requests_data(raw, bytes)?;
        if expected.is_empty() || expected.len() != ids.len() {
            return Err(invalid("delayed view requires actual callback rows"));
        }
        let prepared = crate::bottom_pane::alab_retained_delayed_approval::PreparedDelayed::decode(
            raw, bytes,
        )?;
        // This is idempotent only for the actual marker installed before
        // replay. A late unprimed modal cannot masquerade as correct timing.
        prepared.prime_before_original_replay(self)?;
        self.alab_normalize_original_approval_rows(&expected, ids, client, bytes)?;
        // Mixed actual delayed/promoted rows retain their true locations and
        // per-row Features. The original timer alone promotes due rows.
        self.maybe_show_delayed_approval_requests_at(Instant::now());
        Ok(())
    }
    fn alab_normalize_original_approval_rows(
        &mut self,
        expected: &[crate::bottom_pane::alab_retained_approval::RequestData],
        ids: &[RequestId],
        client: &AppServerRequestHandle,
        bytes: usize,
    ) -> io::Result<()> {
        let mut actual_rows = Vec::new();
        for row in &self.delayed_approval_requests {
            let item = row
                .alab_original_input
                .as_ref()
                .ok_or_else(|| invalid("actual delayed row lacks original item"))?;
            actual_rows.push((
                crate::bottom_pane::alab_retained_approval::RequestData::capture(&row.request),
                item.clone(),
            ));
        }
        for view in &self.view_stack {
            let Ok(ViewData::Approval { original }) = view.alab_capture_retained_view(bytes) else {
                continue;
            };
            let requests =
                crate::bottom_pane::alab_retained_approval::requests_data(&original, bytes)?;
            let items = view.alab_original_input_items()?;
            if requests.len() != items.len() {
                return Err(invalid("actual promoted queue lost reply bindings"));
            }
            actual_rows.extend(requests.into_iter().zip(items));
        }
        for (request, id) in expected.iter().zip(ids) {
            let found = actual_rows
                .iter()
                .filter(|(_, item)| item.id() == *id)
                .collect::<Vec<_>>();
            if found.len() != 1 {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "original delayed callback is missing or repeated in actual replay",
                ));
            }
            let (native, item) = found[0];
            let owner = ApprovalOwner::from_actual(item, client)?;
            if !owner.matches(request) || !owner.matches(native) {
                return Err(invalid("delayed callback native resource changed"));
            }
        }
        self.alab_reorder_actual_delayed_rows(ids)?;
        for view in &mut self.view_stack {
            view.alab_normalize_delayed_approvals(expected, ids, client)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod clock_controls {
    use super::*;
    #[test]
    fn real_empty_callback_stack_still_carries_actual_delayed_clock() -> io::Result<()> {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let pane =
            crate::bottom_pane::tests::test_pane(crate::app_event_sender::AppEventSender::new(tx));
        let (views, clock) = pane.alab_capture_bound_view_stack(1_048_576)?;
        assert!(views.is_empty());
        let value: serde_json::Value = serde_json::from_str(clock.get()).map_err(invalid)?;
        assert_eq!(value["requests"], serde_json::json!([]));
        assert_eq!(value["generation"], serde_json::json!(0));
        assert!(value["last_activity"].is_null());
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut actual =
            crate::bottom_pane::tests::test_pane(crate::app_event_sender::AppEventSender::new(tx));
        crate::bottom_pane::alab_retained_delayed_approval::PreparedDelayed::decode(
            &clock, 1_048_576,
        )?
        .prime_before_original_replay(&mut actual)?;
        assert!(actual.view_stack.is_empty());
        assert!(actual.delayed_approval_requests.is_empty());
        Ok(())
    }
}

impl BottomPane {
    /// Caller has already validated the exact same-client native Items and
    /// whole request content. Move whole actual rows, including Features;
    /// selecting a saved ID here alone supplies no callback/Source authority.
    fn alab_reorder_actual_delayed_rows(&mut self, ids: &[RequestId]) -> io::Result<()> {
        let mut captured = Vec::new();
        for id in ids {
            let positions = self
                .delayed_approval_requests
                .iter()
                .enumerate()
                .filter(|(_, row)| {
                    row.alab_original_input
                        .as_ref()
                        .is_some_and(|item| item.id() == *id)
                })
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            if positions.len() > 1 {
                return Err(invalid(
                    "actual delayed item repeated before queue normalization",
                ));
            }
            if let Some(&position) = positions.first() {
                captured.push(position);
            }
        }
        if captured.is_empty() {
            return Ok(());
        }
        let order = captured
            .iter()
            .copied()
            .chain(
                (0..self.delayed_approval_requests.len()).filter(|index| !captured.contains(index)),
            )
            .collect::<Vec<_>>();
        if order
            .iter()
            .copied()
            .eq(0..self.delayed_approval_requests.len())
        {
            return Ok(());
        }
        if order.len() != self.delayed_approval_requests.len()
            || order
                .iter()
                .copied()
                .collect::<std::collections::HashSet<_>>()
                .len()
                != order.len()
        {
            return Err(invalid("delayed queue normalization lost an actual row"));
        }
        let mut original = std::mem::take(&mut self.delayed_approval_requests)
            .into_iter()
            .map(Some)
            .collect::<Vec<_>>();
        self.delayed_approval_requests = order
            .into_iter()
            .map(|index| {
                original[index]
                    .take()
                    .expect("validated whole actual row order")
            })
            .collect();
        Ok(())
    }
}
#[cfg(test)]
mod actual_future_fifo_controls {
    use super::*;
    #[test]
    fn future_original_deadline_retains_saved_fifo_whole_rows_and_postcut_features()
    -> io::Result<()> {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut pane =
            crate::bottom_pane::tests::test_pane(crate::app_event_sender::AppEventSender::new(tx));
        let activity = Instant::now();
        pane.last_composer_activity_at = Some(activity);
        let mut expected = Vec::new();
        for (id, label, features) in [
            (123, "c", Features::default()),
            (121, "a", Features::with_defaults()),
            (124, "new", Features::default()),
            (122, "b", Features::with_defaults()),
        ] {
            let item = crate::app::retained_original_input::data_only_approval_item(id, label)?;
            let request = ApprovalRequest::Exec(ExecApprovalRequest {
                kind: Default::default(),
                thread_id: ThreadId::from_string(item.thread_id_data()).map_err(invalid)?,
                thread_label: None,
                id: label.into(),
                environment_id: None,
                command: vec![label.into()],
                reason: None,
                available_decisions: vec![
                    codex_app_server_protocol::CommandExecutionApprovalDecision::Accept,
                ],
                network_approval_context: None,
                additional_permissions: None,
            });
            item.with_scope(|| pane.push_approval_request(request, &features));
            expected.push((item, features));
        }
        assert_eq!(pane.delayed_approval_requests.len(), 4);
        assert!(pane.view_stack.is_empty());
        let saved = [
            RequestId::Integer(121),
            RequestId::Integer(122),
            RequestId::Integer(123),
        ];
        // Lower native queue policy only. The public common route above also
        // requires actual same-client callback owners/current Source cut; the
        // DATA-only Items deliberately cannot supply those proofs.
        pane.alab_reorder_actual_delayed_rows(&saved)?;
        let actual = pane
            .delayed_approval_requests
            .iter()
            .map(|row| row.alab_original_input.as_ref().unwrap().id())
            .collect::<Vec<_>>();
        assert_eq!(
            actual,
            vec![
                saved[0].clone(),
                saved[1].clone(),
                saved[2].clone(),
                RequestId::Integer(124)
            ]
        );
        for row in &pane.delayed_approval_requests {
            let actual = row.alab_original_input.as_ref().unwrap();
            let (_, features) = expected
                .iter()
                .find(|(item, _)| std::sync::Arc::ptr_eq(item, actual))
                .unwrap();
            assert_eq!(&row.features, features);
        }
        pane.maybe_show_delayed_approval_requests_at(
            activity + APPROVAL_PROMPT_TYPING_IDLE_DELAY - std::time::Duration::from_millis(1),
        );
        assert!(pane.view_stack.is_empty());
        assert_eq!(pane.delayed_approval_requests.len(), 4);
        Ok(())
    }
}
