// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Exact owning-Session metadata bound to original native resources. This leaf
//! does not create a client, callback, ConfigBuilder, job or cross-exec issuer.
use super::*;
use codex_protocol::openai_models::ReasoningEffort;
use serde::Deserialize;
use serde::Serialize;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::ffi::OsStringExt;
use std::sync::Weak;
#[path = "retained_session_aliases.rs"]
pub(crate) mod aliases;
use super::alab_session_models::model;
use aliases::GateRef;
use aliases::GenerationRef;
use history::alab_retained_history as paging;
fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}
fn required<'de, D, T>(d: D) -> std::result::Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(d)
}
const MAX_BYTES: usize = 16 * 1024 * 1024;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PathData {
    unix: Vec<u8>,
}
impl PathData {
    fn capture(actual: &std::path::Path) -> Self {
        Self {
            unix: actual.as_os_str().as_bytes().to_vec(),
        }
    }
    fn prepare(&self) -> io::Result<PathBuf> {
        if self.unix.len() > MAX_BYTES || self.unix.contains(&0) {
            return Err(invalid("native session path DATA bounds"));
        }
        Ok(PathBuf::from(std::ffi::OsString::from_vec(
            self.unix.clone(),
        )))
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CloudSelection {
    thread: ThreadId,
    model: String,
    #[serde(deserialize_with = "required")]
    effort: Option<ReasoningEffort>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Defaults {
    #[serde(deserialize_with = "required")]
    model: Option<String>,
    #[serde(deserialize_with = "required")]
    effort: Option<ReasoningEffort>,
    #[serde(deserialize_with = "required")]
    service_tier: Option<String>,
}
#[derive(Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Data {
    schema: u8,
    #[serde(deserialize_with = "required")]
    request_namespace: Option<Uuid>,
    next_request_id: i64,
    automatic_names: Vec<(ThreadId, GateRef)>,
    #[serde(deserialize_with = "required")]
    pending_side_skills_refresh: Option<Uuid>,
    pagination: Vec<(ThreadId, paging::Data)>,
    #[serde(deserialize_with = "required")]
    attachment_subscribed: Option<bool>,
    #[serde(deserialize_with = "required")]
    focused_conversation: Option<ThreadId>,
    #[serde(deserialize_with = "required")]
    attempted_revision: Option<String>,
    #[serde(deserialize_with = "required")]
    frozen_receipt: Option<(ThreadId, String)>,
    unread_revisions: Vec<(ThreadId, String)>,
    task_tool_threads: Vec<ThreadId>,
    #[serde(deserialize_with = "required")]
    task_capabilities_dir: Option<PathData>,
    task_search_generation: GenerationRef,
    #[serde(deserialize_with = "required")]
    remote_cwd: Option<PathData>,
    thread_mode: u8,
    #[serde(deserialize_with = "required")]
    model_provider_override: Option<String>,
    history_support: u8,
    thread_settings_update_supported: bool,
    cloud_selections: Vec<CloudSelection>,
    #[serde(deserialize_with = "required")]
    default_model: Option<String>,
    available_models: Vec<model::Data>,
    #[serde(deserialize_with = "required")]
    prefetched_models: Option<alab_session_models::Data>,
    daybreak: GateRef,
    #[serde(deserialize_with = "required")]
    managed_defaults: Option<Defaults>,
    #[serde(deserialize_with = "required")]
    external_agent_import: Option<String>,
    dynamic_mcp_present: bool,
    worktree_builder_present: bool,
    retained_pending_present: bool,
    retained_tenure_present: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireData {
    schema: u8,
    #[serde(deserialize_with = "required")]
    request_namespace: Option<Uuid>,
    next_request_id: i64,
    automatic_names: Vec<(ThreadId, GateRef)>,
    #[serde(deserialize_with = "required")]
    pending_side_skills_refresh: Option<Uuid>,
    pagination: Vec<(ThreadId, paging::Data)>,
    #[serde(deserialize_with = "required")]
    attachment_subscribed: Option<bool>,
    #[serde(deserialize_with = "required")]
    focused_conversation: Option<ThreadId>,
    #[serde(deserialize_with = "required")]
    attempted_revision: Option<String>,
    #[serde(deserialize_with = "required")]
    frozen_receipt: Option<(ThreadId, String)>,
    unread_revisions: Vec<(ThreadId, String)>,
    task_tool_threads: Vec<ThreadId>,
    #[serde(deserialize_with = "required")]
    task_capabilities_dir: Option<PathData>,
    task_search_generation: GenerationRef,
    #[serde(deserialize_with = "required")]
    remote_cwd: Option<PathData>,
    thread_mode: u8,
    #[serde(deserialize_with = "required")]
    model_provider_override: Option<String>,
    history_support: u8,
    thread_settings_update_supported: bool,
    cloud_selections: Vec<CloudSelection>,
    #[serde(deserialize_with = "required")]
    default_model: Option<String>,
    available_models: Vec<model::Data>,
    #[serde(deserialize_with = "required")]
    prefetched_models: Option<alab_session_models::Data>,
    daybreak: GateRef,
    #[serde(deserialize_with = "required")]
    managed_defaults: Option<Defaults>,
    #[serde(deserialize_with = "required")]
    external_agent_import: Option<String>,
    dynamic_mcp_present: bool,
    worktree_builder_present: bool,
    retained_pending_present: bool,
    retained_tenure_present: bool,
}
impl<'de> Deserialize<'de> for Data {
    fn deserialize<D: serde::Deserializer<'de>>(decoder: D) -> std::result::Result<Self, D::Error> {
        let raw = Box::<serde_json::value::RawValue>::deserialize(decoder)?;
        Self::decode_raw(raw.get().as_bytes()).map_err(serde::de::Error::custom)
    }
}
impl Data {
    /// Direct JSON only: native nested defaults/aliases/unknown fields cannot
    /// silently change the exact serialized native metadata shape.
    pub(crate) fn decode_raw(raw: &[u8]) -> io::Result<Self> {
        if raw.len() > MAX_BYTES {
            return Err(invalid("native Session metadata raw budget"));
        }
        let wire: WireData = serde_json::from_slice(raw).map_err(invalid)?;
        let data = Self {
            schema: wire.schema,
            request_namespace: wire.request_namespace,
            next_request_id: wire.next_request_id,
            automatic_names: wire.automatic_names,
            pending_side_skills_refresh: wire.pending_side_skills_refresh,
            pagination: wire.pagination,
            attachment_subscribed: wire.attachment_subscribed,
            focused_conversation: wire.focused_conversation,
            attempted_revision: wire.attempted_revision,
            frozen_receipt: wire.frozen_receipt,
            unread_revisions: wire.unread_revisions,
            task_tool_threads: wire.task_tool_threads,
            task_capabilities_dir: wire.task_capabilities_dir,
            task_search_generation: wire.task_search_generation,
            remote_cwd: wire.remote_cwd,
            thread_mode: wire.thread_mode,
            model_provider_override: wire.model_provider_override,
            history_support: wire.history_support,
            thread_settings_update_supported: wire.thread_settings_update_supported,
            cloud_selections: wire.cloud_selections,
            default_model: wire.default_model,
            available_models: wire.available_models,
            prefetched_models: wire.prefetched_models,
            daybreak: wire.daybreak,
            managed_defaults: wire.managed_defaults,
            external_agent_import: wire.external_agent_import,
            dynamic_mcp_present: wire.dynamic_mcp_present,
            worktree_builder_present: wire.worktree_builder_present,
            retained_pending_present: wire.retained_pending_present,
            retained_tenure_present: wire.retained_tenure_present,
        };
        let supplied: serde_json::Value = serde_json::from_slice(raw).map_err(invalid)?;
        if supplied != serde_json::to_value(&data).map_err(invalid)? {
            return Err(invalid(
                "native Session metadata wire differs from canonical captured shape",
            ));
        }
        data.validate_data()?;
        Ok(data)
    }
}

trait References {
    fn gate(&mut self, actual: &Arc<Semaphore>) -> io::Result<GateRef>;
    fn generation(&mut self, actual: &Arc<AtomicU64>) -> io::Result<GenerationRef>;
}
impl References for aliases::Capture {
    fn gate(&mut self, actual: &Arc<Semaphore>) -> io::Result<GateRef> {
        self.gate(actual)
    }
    fn generation(&mut self, actual: &Arc<AtomicU64>) -> io::Result<GenerationRef> {
        self.generation(actual)
    }
}
struct OriginalRefs<'a>(&'a aliases::OriginalResources);
impl References for OriginalRefs<'_> {
    fn gate(&mut self, actual: &Arc<Semaphore>) -> io::Result<GateRef> {
        self.0.gate_for(actual)
    }
    fn generation(&mut self, actual: &Arc<AtomicU64>) -> io::Result<GenerationRef> {
        self.0.generation_for(actual)
    }
}
fn sorted<T>(items: impl IntoIterator<Item = (ThreadId, T)>) -> Vec<(ThreadId, T)> {
    let mut items = items.into_iter().collect::<Vec<_>>();
    items.sort_by_key(|(thread, _)| thread.to_string());
    items
}
fn ordered(items: impl IntoIterator<Item = ThreadId>) -> bool {
    let mut prior = None;
    for id in items {
        let next = id.to_string();
        if prior.as_ref().is_some_and(|prior| prior >= &next) {
            return false;
        }
        prior = Some(next);
    }
    true
}
impl Data {
    fn capture(
        actual: &AppServerSession,
        refs: &mut impl References,
        models: Option<alab_session_models::Data>,
    ) -> io::Result<Self> {
        if actual.connection_owner.is_none() {
            return Err(invalid(
                "request-only view cannot supply physical Session metadata",
            ));
        }
        let automatic_names = sorted(
            actual
                .automatic_name_writes
                .iter()
                .map(|(thread, gate)| Ok((*thread, refs.gate(gate)?)))
                .collect::<io::Result<Vec<_>>>()?,
        );
        let mut tasks = actual.task_tool_threads.iter().copied().collect::<Vec<_>>();
        tasks.sort_by_key(ToString::to_string);
        let data = Self {
            schema: 1,
            request_namespace: actual.request_id_namespace,
            next_request_id: actual.next_request_id,
            automatic_names,
            pending_side_skills_refresh: actual.pending_side_skills_refresh,
            pagination: sorted(
                actual
                    .history_pagination
                    .iter()
                    .map(|(thread, actual)| (*thread, paging::Data::capture(actual))),
            ),
            attachment_subscribed: actual.attachment_subscribed,
            focused_conversation: actual.focused_conversation,
            attempted_revision: actual.attempted_revision.clone(),
            frozen_receipt: actual.frozen_receipt.clone(),
            unread_revisions: sorted(
                actual
                    .unread_revisions
                    .iter()
                    .map(|(thread, value)| (*thread, value.clone())),
            ),
            task_tool_threads: tasks,
            task_capabilities_dir: actual
                .task_tool_capabilities_dir
                .as_ref()
                .map(|path| PathData::capture(path.as_path())),
            task_search_generation: refs.generation(&actual.task_search_generation)?,
            remote_cwd: actual.remote_cwd_override.as_deref().map(PathData::capture),
            thread_mode: match actual.thread_params_mode {
                ThreadParamsMode::Embedded => 0,
                ThreadParamsMode::Remote => 1,
                ThreadParamsMode::Cloud => 2,
            },
            model_provider_override: actual.model_provider_override.clone(),
            history_support: match actual.history_support {
                ThreadHistorySupport::Paginated => 0,
                ThreadHistorySupport::LegacyOnly => 1,
            },
            thread_settings_update_supported: actual.thread_settings_update_supported,
            cloud_selections: sorted(actual.cloud_model_selections.iter().map(
                |(thread, value)| {
                    (
                        *thread,
                        CloudSelection {
                            thread: *thread,
                            model: value.model.clone(),
                            effort: value.effort.clone(),
                        },
                    )
                },
            ))
            .into_iter()
            .map(|(_, value)| value)
            .collect(),
            default_model: actual.default_model.clone(),
            available_models: actual
                .available_models
                .iter()
                .map(model::Data::capture)
                .collect(),
            prefetched_models: models,
            daybreak: refs.gate(&actual.daybreak_api_key_compat)?,
            managed_defaults: actual
                .managed_new_thread_defaults
                .as_ref()
                .map(|value| Defaults {
                    model: value.model.clone(),
                    effort: value.model_reasoning_effort.clone(),
                    service_tier: value.service_tier.clone(),
                }),
            external_agent_import: actual
                .external_agent_config_import_id
                .lock()
                .map_err(invalid)?
                .clone(),
            dynamic_mcp_present: actual.dynamic_tool_mcp.is_some(),
            worktree_builder_present: actual.worktree_source_config_builder.is_some(),
            retained_pending_present: actual.alab_retained_ui.is_some(),
            retained_tenure_present: actual.owned_local.is_some(),
        };
        data.validate_data()?;
        Ok(data)
    }
    pub(crate) fn validate_data(&self) -> io::Result<()> {
        if self.schema != 1
            || self.next_request_id < 1
            || self.thread_mode > 2
            || self.history_support > 1
            || serde_json::to_vec(self).map_err(invalid)?.len() > MAX_BYTES
        {
            return Err(invalid("native owning Session metadata schema/budget"));
        }
        if !ordered(self.automatic_names.iter().map(|(id, _)| *id))
            || !ordered(self.pagination.iter().map(|(id, _)| *id))
            || !ordered(self.unread_revisions.iter().map(|(id, _)| *id))
            || !ordered(self.task_tool_threads.iter().copied())
            || !ordered(self.cloud_selections.iter().map(|entry| entry.thread))
        {
            return Err(invalid(
                "native Session keyed DATA is duplicated or noncanonical",
            ));
        }
        for (_, page) in &self.pagination {
            let _native = page.prepare()?;
        }
        if let Some(path) = &self.task_capabilities_dir {
            let _native = AbsolutePathBuf::try_from(path.prepare()?).map_err(invalid)?;
        }
        if let Some(path) = &self.remote_cwd {
            let _native = path.prepare()?;
        }
        for model in &self.available_models {
            let _native = model.prepare()?;
        }
        if let Some(models) = &self.prefetched_models {
            models.validate_data()?;
        }
        Ok(())
    }
}
struct Pins {
    request: codex_app_server_client::OriginalRequestIdentity,
    dynamic: Option<Weak<DynamicToolMcpServer>>,
    builder: Option<Weak<()>>,
    pending: Option<Weak<alab_retained_ui::Pending>>,
    bytes: Vec<u8>,
}
pub(crate) struct Captured {
    pub(crate) data: Data,
    pins: Pins,
    models: alab_session_models::Ready,
}
impl Captured {
    /// The caller retains/joins the original prefetch cut while its SAME App/Tui
    /// pump stays live, then borrows ALL sessions into one enclosing alias graph.
    pub(crate) fn capture(
        actual: &AppServerSession,
        refs: &mut aliases::Capture,
        models: alab_session_models::Ready,
    ) -> io::Result<Self> {
        let data = Data::capture(actual, refs, models.current(actual)?)?;
        let bytes = serde_json::to_vec(&data).map_err(invalid)?;
        let pins = Pins {
            request: actual.request_handle().retained_original_resource(),
            dynamic: actual.dynamic_tool_mcp.as_ref().map(Arc::downgrade),
            builder: actual
                .worktree_source_config_builder
                .as_ref()
                .map(super::alab_builder_owner::Owned::identity),
            pending: actual.alab_retained_ui.as_ref().map(Arc::downgrade),
            bytes,
        };
        models.check()?;
        Ok(Self { data, pins, models })
    }
    /// SAME original resources only; this does not install any decoded metadata
    /// into a new client. C2 still requires the separate actual resource issuer.
    pub(crate) fn validate_original(
        &self,
        data: &Data,
        resources_data: &aliases::Data,
        resources: &aliases::OriginalResources,
        actual: &AppServerSession,
    ) -> io::Result<()> {
        data.validate_data()?;
        resources.validate_data(resources_data)?;
        self.models.check()?;
        if !self.pins.request.matches(&actual.request_handle()) {
            return Err(invalid("owning Session changed original command resource"));
        }
        let dynamic = match (&self.pins.dynamic, &actual.dynamic_tool_mcp) {
            (None, None) => true,
            (Some(old), Some(actual)) => Weak::ptr_eq(old, &Arc::downgrade(actual)),
            _ => false,
        };
        let pending = match (&self.pins.pending, &actual.alab_retained_ui) {
            (None, None) => true,
            (Some(old), Some(actual)) => Weak::ptr_eq(old, &Arc::downgrade(actual)),
            _ => false,
        };
        let builder = match (&self.pins.builder, &actual.worktree_source_config_builder) {
            (None, None) => true,
            (Some(old), Some(actual)) => Weak::ptr_eq(old, &actual.identity()),
            _ => false,
        };
        if !dynamic || !pending || !builder {
            return Err(invalid(
                "native owning Session changed original job/config resource",
            ));
        }
        let current = Data::capture(
            actual,
            &mut OriginalRefs(resources),
            self.models.current(actual)?,
        )?;
        if serde_json::to_vec(data).map_err(invalid)? != self.pins.bytes
            || serde_json::to_vec(&current).map_err(invalid)? != self.pins.bytes
        {
            return Err(invalid(
                "native Session metadata changed or differs from captured DATA",
            ));
        }
        self.models.check()?;
        resources.validate_data(resources_data)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn actual_session() -> AppServerSession {
        let (client, commands, events) = codex_app_server_client::CloudClient::channel(2);
        drop(commands);
        drop(events);
        AppServerSession::new(
            crate::backend_id::BackendId::new(),
            codex_app_server_client::AppServerClient::Cloud(client),
            ThreadParamsMode::Cloud,
        )
    }
    #[tokio::test]
    async fn original_metadata_checks_both_decoded_data_and_same_actual_channel_and_counter()
    -> io::Result<()> {
        let mut original = actual_session();
        original.next_request_id = 41;
        original.request_id_namespace = Some(Uuid::new_v4());
        original.attachment_subscribed = Some(false);
        original.attempted_revision = Some("native revision".into());
        let ready = original
            .alab_capture_original_model_jobs()
            .observe()
            .await?;
        let mut graph = aliases::Capture::new();
        let captured = Captured::capture(&original, &mut graph, ready)?;
        let (resources, pins) = graph.finish();
        captured.validate_original(&captured.data, &resources, &pins, &original)?;
        let mut changed = captured.data.clone();
        changed.next_request_id += 1;
        if captured
            .validate_original(&changed, &resources, &pins, &original)
            .is_ok()
        {
            return Err(invalid("edited metadata DATA replaced original counter"));
        }
        original.next_request_id += 1;
        if captured
            .validate_original(&captured.data, &resources, &pins, &original)
            .is_ok()
        {
            return Err(invalid("actual original counter edit was missed"));
        }
        original.next_request_id -= 1;
        let other = actual_session();
        if captured
            .validate_original(&captured.data, &resources, &pins, &other)
            .is_ok()
        {
            return Err(invalid(
                "another actual client was accepted from equal logical metadata",
            ));
        }
        Ok(())
    }
    #[tokio::test]
    async fn raw_native_path_and_required_nullable_model_state_are_preserved() -> io::Result<()> {
        let mut original = actual_session();
        let path = PathBuf::from(std::ffi::OsString::from_vec(
            b"/old/native-\xff-cwd".to_vec(),
        ));
        original.remote_cwd_override = Some(path.clone());
        let ready = original
            .alab_capture_original_model_jobs()
            .observe()
            .await?;
        let mut graph = aliases::Capture::new();
        let captured = Captured::capture(&original, &mut graph, ready)?;
        let (resources, pins) = graph.finish();
        let wire = serde_json::to_vec(&captured.data).map_err(invalid)?;
        let decoded: Data = serde_json::from_slice(&wire).map_err(invalid)?;
        captured.validate_original(&decoded, &resources, &pins, &original)?;
        if decoded
            .remote_cwd
            .as_ref()
            .ok_or_else(|| invalid("native cwd absent"))?
            .prepare()?
            != path
        {
            return Err(invalid("native non-UTF8 cwd changed"));
        }
        let mut missing = serde_json::to_value(&decoded).map_err(invalid)?;
        missing
            .as_object_mut()
            .ok_or_else(|| invalid("metadata DATA not object"))?
            .remove("prefetched_models");
        if Data::decode_raw(&serde_json::to_vec(&missing).map_err(invalid)?).is_ok() {
            return Err(invalid("missing original model cache defaulted to None"));
        }
        Ok(())
    }
}
#[cfg(test)]
mod raw_and_incarnation_tests {
    use super::*;
    fn actual_session() -> AppServerSession {
        let (client, commands, events) = codex_app_server_client::CloudClient::channel(2);
        drop(commands);
        drop(events);
        AppServerSession::new(
            crate::backend_id::BackendId::new(),
            codex_app_server_client::AppServerClient::Cloud(client),
            ThreadParamsMode::Cloud,
        )
    }
    #[tokio::test]
    async fn actual_builder_replacement_cannot_inherit_old_original_session_pin() -> io::Result<()>
    {
        let mut original = actual_session();
        original.worktree_source_config_builder =
            Some(super::super::alab_builder_owner::Owned::new(
                crate::legacy_core::config::ConfigBuilder::default(),
            ));
        let ready = original
            .alab_capture_original_model_jobs()
            .observe()
            .await?;
        let mut aliases = aliases::Capture::new();
        let captured = Captured::capture(&original, &mut aliases, ready)?;
        let (data, pins) = aliases.finish();
        captured.validate_original(&captured.data, &data, &pins, &original)?;
        original.worktree_source_config_builder = original.worktree_source_config_builder.clone();
        if captured
            .validate_original(&captured.data, &data, &pins, &original)
            .is_ok()
        {
            return Err(invalid(
                "new native builder allocation inherited the old incarnation",
            ));
        }
        Ok(())
    }
    fn actual_model() -> io::Result<ModelPreset> {
        let model = serde_json::json!({"id":"actual-native-model","model":"actual-native-model","displayName":"Actual Native Model","description":"Original native model data","hidden":false,"supportedReasoningEfforts":[{"reasoningEffort":"medium","description":"Medium"}],"defaultReasoningEffort":"medium","isDefault":true});
        let mut model =
            model_preset_from_api_model(serde_json::from_value(model).map_err(invalid)?);
        model.multi_agent_version = Some(codex_protocol::protocol::MultiAgentVersion::V2);
        Ok(model)
    }
    #[tokio::test]
    async fn direct_native_model_bytes_refuse_omitted_nullable_defaulted_and_unknown_fields()
    -> io::Result<()> {
        let mut original = actual_session();
        original.available_models = vec![actual_model()?];
        let ready = original
            .alab_capture_original_model_jobs()
            .observe()
            .await?;
        let mut aliases = aliases::Capture::new();
        let captured = Captured::capture(&original, &mut aliases, ready)?;
        let (data, pins) = aliases.finish();
        let bytes = serde_json::to_vec(&captured.data).map_err(invalid)?;
        let decoded = Data::decode_raw(&bytes)?;
        captured.validate_original(&decoded, &data, &pins, &original)?;
        if decoded.available_models[0].prepare()?.multi_agent_version
            != original.available_models[0].multi_agent_version
        {
            return Err(invalid("native serde-skipped model state was lost"));
        }
        for field in ["upgrade", "supports_personality"] {
            let mut wire: serde_json::Value = serde_json::from_slice(&bytes).map_err(invalid)?;
            wire["available_models"][0]["native"]
                .as_object_mut()
                .ok_or_else(|| invalid("native model DATA absent"))?
                .remove(field);
            if Data::decode_raw(&serde_json::to_vec(&wire).map_err(invalid)?).is_ok() {
                return Err(invalid("omitted native model field silently defaulted"));
            }
        }
        let mut wire: serde_json::Value = serde_json::from_slice(&bytes).map_err(invalid)?;
        wire["available_models"][0]["native"]["not_an_original_field"] = serde_json::json!(true);
        if Data::decode_raw(&serde_json::to_vec(&wire).map_err(invalid)?).is_ok() {
            return Err(invalid("unknown native model field was discarded"));
        }
        Ok(())
    }
}
