// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual native status-card/handle and independent mutable UI row aliases. No account/IO grant.
use super::super::thread_usage::alab_retained_usage as usage;
use super::*;
use crate::terminal_hyperlinks::alab_retained_text as native;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::ffi::OsStringExt;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct StatusRef(u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct SummaryRef(u32);
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Data {
    card: StatusRef,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Graph {
    cards: Vec<Card>,
    agents: Vec<String>,
    rates: Vec<RateState>,
    usage: usage::Graph,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Card {
    model_name: String,
    model_details: Vec<String>,
    directory: Vec<u8>,
    permissions: String,
    agents: SummaryRef,
    collaboration_mode: Option<String>,
    model_provider: Option<String>,
    remote: Option<Remote>,
    show_usage_link: bool,
    account: Option<Account>,
    thread_name: Option<String>,
    session_id: Option<String>,
    forked_from: Option<String>,
    tokens: Tokens,
    rates: u32,
    usage: usage::Data,
    plan: Option<PlanType>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Tokens {
    total: i64,
    input: i64,
    output: i64,
    context: Option<[i64; 3]>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Remote {
    address: String,
    version: String,
    local: bool,
    managed: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum Account {
    ChatGpt {
        email: Option<String>,
        plan: Option<String>,
        workspace: Option<String>,
    },
    ApiKey {},
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RateState {
    limits: Limits,
    refreshing: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum Limits {
    Available { rows: Vec<Row> },
    Stale { rows: Vec<Row> },
    Unavailable {},
    Missing {},
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Row {
    label: String,
    value: RateValue,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum RateValue {
    Window {
        percent_bits: u64,
        resets_at: Option<String>,
        details: Option<String>,
    },
    Text {
        text: String,
    },
}
impl Limits {
    fn capture(actual: &StatusRateLimitData) -> Self {
        let rows = |actual: &[StatusRateLimitRow]| {
            actual
                .iter()
                .map(|row| Row {
                    label: row.label.clone(),
                    value: match &row.value {
                        StatusRateLimitValue::Window {
                            percent_used,
                            resets_at,
                            details,
                        } => RateValue::Window {
                            percent_bits: percent_used.to_bits(),
                            resets_at: resets_at.clone(),
                            details: details.clone(),
                        },
                        StatusRateLimitValue::Text(text) => RateValue::Text { text: text.clone() },
                    },
                })
                .collect()
        };
        match actual {
            StatusRateLimitData::Available(values) => Self::Available { rows: rows(values) },
            StatusRateLimitData::Stale(values) => Self::Stale { rows: rows(values) },
            StatusRateLimitData::Unavailable => Self::Unavailable {},
            StatusRateLimitData::Missing => Self::Missing {},
        }
    }
    fn restore(self) -> StatusRateLimitData {
        let rows = |values: Vec<Row>| {
            values
                .into_iter()
                .map(|row| StatusRateLimitRow {
                    label: row.label,
                    value: match row.value {
                        RateValue::Window {
                            percent_bits,
                            resets_at,
                            details,
                        } => StatusRateLimitValue::Window {
                            percent_used: f64::from_bits(percent_bits),
                            resets_at,
                            details,
                        },
                        RateValue::Text { text } => StatusRateLimitValue::Text(text),
                    },
                })
                .collect()
        };
        match self {
            Self::Available { rows: values } => StatusRateLimitData::Available(rows(values)),
            Self::Stale { rows: values } => StatusRateLimitData::Stale(rows(values)),
            Self::Unavailable {} => StatusRateLimitData::Unavailable,
            Self::Missing {} => StatusRateLimitData::Missing,
        }
    }
}
#[derive(Default)]
pub(crate) struct Capture {
    cards: Vec<Arc<StatusHistoryCell>>,
    card_ids: HashMap<usize, u32>,
    agents: Vec<Arc<RwLock<String>>>,
    agent_ids: HashMap<usize, u32>,
    rates: Vec<Arc<RwLock<StatusRateLimitState>>>,
    rate_ids: HashMap<usize, u32>,
    usage: usage::Capture,
}
fn intern<T>(
    actual: &Arc<T>,
    held: &mut Vec<Arc<T>>,
    ids: &mut HashMap<usize, u32>,
) -> io::Result<u32> {
    let pointer = Arc::as_ptr(actual) as usize;
    if let Some(id) = ids.get(&pointer) {
        return Ok(*id);
    }
    if held.len() >= native::MAX_NODES {
        return Err(native::invalid("status native alias capacity"));
    }
    let id = u32::try_from(held.len()).map_err(native::invalid)?;
    held.push(actual.clone());
    ids.insert(pointer, id);
    Ok(id)
}
impl Capture {
    fn card(&mut self, actual: &Arc<StatusHistoryCell>) -> io::Result<StatusRef> {
        intern(actual, &mut self.cards, &mut self.card_ids).map(StatusRef)
    }
    pub(crate) fn handle(&mut self, actual: &StatusHistoryHandle) -> io::Result<StatusRef> {
        self.card(&actual.card)
    }
    pub(crate) fn summary(&mut self, actual: &Arc<RwLock<String>>) -> io::Result<SummaryRef> {
        intern(actual, &mut self.agents, &mut self.agent_ids).map(SummaryRef)
    }
    pub(crate) fn export(&mut self) -> io::Result<Graph> {
        let mut cards = Vec::with_capacity(self.cards.len());
        for index in 0..self.cards.len() {
            let actual = self.cards[index].clone();
            let agents = self.summary(&actual.agents_summary)?;
            let rates = intern(
                &actual.rate_limit_state,
                &mut self.rates,
                &mut self.rate_ids,
            )?;
            let usage = self.usage.reference(&actual.thread_usage)?;
            cards.push(Card {
                model_name: actual.model_name.clone(),
                model_details: actual.model_details.clone(),
                directory: actual.directory.as_os_str().as_bytes().to_vec(),
                permissions: actual.permissions.clone(),
                agents,
                collaboration_mode: actual.collaboration_mode.clone(),
                model_provider: actual.model_provider.clone(),
                remote: actual.remote_connection.as_ref().map(|actual| Remote {
                    address: actual.address.clone(),
                    version: actual.version.clone(),
                    local: actual.is_local_daemon,
                    managed: actual.is_managed_daemon,
                }),
                show_usage_link: actual.show_chatgpt_usage_link,
                account: actual.account.as_ref().map(|actual| match actual {
                    StatusAccountDisplay::ChatGpt {
                        email,
                        plan,
                        workspace,
                    } => Account::ChatGpt {
                        email: email.clone(),
                        plan: plan.clone(),
                        workspace: workspace.clone(),
                    },
                    StatusAccountDisplay::ApiKey => Account::ApiKey {},
                }),
                thread_name: actual.thread_name.clone(),
                session_id: actual.session_id.clone(),
                forked_from: actual.forked_from.clone(),
                tokens: Tokens {
                    total: actual.token_usage.total,
                    input: actual.token_usage.input,
                    output: actual.token_usage.output,
                    context: actual.token_usage.context_window.as_ref().map(|actual| {
                        [
                            actual.percent_remaining,
                            actual.tokens_in_context,
                            actual.window,
                        ]
                    }),
                },
                rates,
                usage,
                plan: actual.plan_type,
            });
        }
        let agents = self
            .agents
            .iter()
            .map(|actual| {
                actual
                    .read()
                    .map(|value| value.clone())
                    .map_err(|_| native::invalid("status agent summary capture lock poisoned"))
            })
            .collect::<io::Result<_>>()?;
        let rates = self
            .rates
            .iter()
            .map(|actual| {
                actual
                    .read()
                    .map(|actual| RateState {
                        limits: Limits::capture(&actual.rate_limits),
                        refreshing: actual.refreshing_rate_limits,
                    })
                    .map_err(|_| native::invalid("status rate capture lock poisoned"))
            })
            .collect::<io::Result<_>>()?;
        Ok(Graph {
            cards,
            agents,
            rates,
            usage: self.usage.export()?,
        })
    }
}
#[derive(Default)]
pub(crate) struct Restored {
    cards: Option<Vec<Arc<StatusHistoryCell>>>,
    agents: Vec<Arc<RwLock<String>>>,
}
impl Restored {
    pub(crate) fn install(&mut self, graph: Graph) -> io::Result<()> {
        if self.cards.is_some()
            || graph.cards.len() > native::MAX_NODES
            || graph.agents.len() > native::MAX_NODES
            || graph.rates.len() > native::MAX_NODES
        {
            return Err(native::invalid("status graph capacity/already installed"));
        }
        let agents = graph
            .agents
            .into_iter()
            .map(|value| Arc::new(RwLock::new(value)))
            .collect::<Vec<_>>();
        let rates = graph
            .rates
            .into_iter()
            .map(|value| {
                Arc::new(RwLock::new(StatusRateLimitState {
                    rate_limits: value.limits.restore(),
                    refreshing_rate_limits: value.refreshing,
                }))
            })
            .collect::<Vec<_>>();
        let mut usage = usage::Restored::default();
        usage.install(graph.usage)?;
        let cards = graph
            .cards
            .into_iter()
            .map(|card| {
                if card.directory.contains(&0) {
                    return Err(native::invalid("native status directory contains NUL"));
                }
                Ok(Arc::new(StatusHistoryCell {
                    model_name: card.model_name,
                    model_details: card.model_details,
                    directory: PathBuf::from(std::ffi::OsString::from_vec(card.directory)),
                    permissions: card.permissions,
                    agents_summary: agents
                        .get(card.agents.0 as usize)
                        .cloned()
                        .ok_or_else(|| native::invalid("missing status summary alias"))?,
                    collaboration_mode: card.collaboration_mode,
                    model_provider: card.model_provider,
                    remote_connection: card.remote.map(|actual| RemoteConnectionStatus {
                        address: actual.address,
                        version: actual.version,
                        is_local_daemon: actual.local,
                        is_managed_daemon: actual.managed,
                    }),
                    show_chatgpt_usage_link: card.show_usage_link,
                    account: card.account.map(|actual| match actual {
                        Account::ChatGpt {
                            email,
                            plan,
                            workspace,
                        } => StatusAccountDisplay::ChatGpt {
                            email,
                            plan,
                            workspace,
                        },
                        Account::ApiKey {} => StatusAccountDisplay::ApiKey,
                    }),
                    thread_name: card.thread_name,
                    session_id: card.session_id,
                    forked_from: card.forked_from,
                    token_usage: StatusTokenUsageData {
                        total: card.tokens.total,
                        input: card.tokens.input,
                        output: card.tokens.output,
                        context_window: card.tokens.context.map(
                            |[percent_remaining, tokens_in_context, window]| {
                                StatusContextWindowData {
                                    percent_remaining,
                                    tokens_in_context,
                                    window,
                                }
                            },
                        ),
                    },
                    rate_limit_state: rates
                        .get(card.rates as usize)
                        .cloned()
                        .ok_or_else(|| native::invalid("missing status rate alias"))?,
                    thread_usage: usage.usage(&card.usage)?,
                    plan_type: card.plan,
                }))
            })
            .collect::<io::Result<Vec<_>>>()?;
        self.agents = agents;
        self.cards = Some(cards);
        Ok(())
    }
    fn card(&self, reference: StatusRef) -> io::Result<Arc<StatusHistoryCell>> {
        self.cards
            .as_ref()
            .and_then(|cards| cards.get(reference.0 as usize))
            .cloned()
            .ok_or_else(|| native::invalid("missing original status card alias"))
    }
    pub(crate) fn handle(&self, reference: StatusRef) -> io::Result<StatusHistoryHandle> {
        self.card(reference)
            .map(|card| StatusHistoryHandle { card })
    }
    pub(crate) fn summary(&self, reference: SummaryRef) -> io::Result<Arc<RwLock<String>>> {
        self.agents
            .get(reference.0 as usize)
            .cloned()
            .ok_or_else(|| native::invalid("missing original status summary alias"))
    }
}
impl Data {
    pub(crate) fn capture(
        actual: &dyn HistoryCell,
        capture: &mut Capture,
    ) -> io::Result<Option<Self>> {
        actual
            .as_any()
            .downcast_ref::<Arc<StatusHistoryCell>>()
            .map(|actual| capture.card(actual).map(|card| Self { card }))
            .transpose()
    }
    pub(crate) fn restore(self, restored: &Restored) -> io::Result<Box<dyn HistoryCell>> {
        restored
            .card(self.card)
            .map(|actual| Box::new(actual) as Box<dyn HistoryCell>)
    }
}

#[cfg(test)]
mod controls {
    use super::*;
    use crate::history_cell::alab_retained_cells as cells;
    fn original_card() -> Arc<StatusHistoryCell> {
        Arc::new(StatusHistoryCell {
            model_name: "exact original model".into(),
            model_details: vec!["first original detail".into(), "second detail".into()],
            directory: PathBuf::from(std::ffi::OsString::from_vec(
                b"/actual/status-\xff".to_vec(),
            )),
            permissions: "exact original permissions".into(),
            agents_summary: Arc::new(RwLock::new("original agents".into())),
            collaboration_mode: Some("original mode".into()),
            model_provider: Some("original provider".into()),
            remote_connection: Some(RemoteConnectionStatus {
                address: "actual original address".into(),
                version: "original native version".into(),
                is_local_daemon: false,
                is_managed_daemon: true,
            }),
            show_chatgpt_usage_link: true,
            account: Some(StatusAccountDisplay::ChatGpt {
                email: Some("fixture@example.invalid".into()),
                plan: Some("original display plan".into()),
                workspace: Some("original display workspace".into()),
            }),
            thread_name: Some("actual thread name".into()),
            session_id: Some("original session display".into()),
            forked_from: Some("original parent display".into()),
            token_usage: StatusTokenUsageData {
                total: 991,
                input: 773,
                output: 218,
                context_window: Some(StatusContextWindowData {
                    percent_remaining: 61,
                    tokens_in_context: 123,
                    window: 991,
                }),
            },
            rate_limit_state: Arc::new(RwLock::new(StatusRateLimitState {
                rate_limits: StatusRateLimitData::Stale(vec![StatusRateLimitRow {
                    label: "original quota".into(),
                    value: StatusRateLimitValue::Window {
                        percent_used: -0.0,
                        resets_at: Some("original localized reset".into()),
                        details: Some("original quota details".into()),
                    },
                }]),
                refreshing_rate_limits: true,
            })),
            thread_usage: StatusThreadUsage::default(),
            plan_type: None,
        })
    }
    #[test]
    fn actual_status_handle_card_roots_and_summary_keep_native_aliases_and_updates()
    -> io::Result<()> {
        let card = original_card();
        let handle = StatusHistoryHandle { card: card.clone() };
        handle.set_thread_usage(Some(codex_app_server_protocol::ThreadUsage {
            thread_id: "actual display thread".into(),
            estimated_usage_credits_micros: 19,
            estimated_usage_usd_micros: Some(7),
            native_usage_usd_micros: Some(11),
            groups: vec![],
        }));
        let roots: Vec<Arc<dyn HistoryCell>> = vec![Arc::new(card.clone()), Arc::new(card.clone())];
        let mut graph = cells::Capture::default();
        let handle_ref = graph.status_handle(&handle)?;
        let summary_ref = graph.status_summary(&card.agents_summary)?;
        let keys = roots
            .iter()
            .map(|actual| graph.intern(actual))
            .collect::<io::Result<Vec<_>>>()?;
        let mut text = native::Capture::default();
        let bytes = graph.encode(&mut text, 1_048_576)?;
        let mut shared = native::Restored::new(text.export()?)?;
        let mut resources = cells::RestoreContext::default();
        let restored = cells::Restored::decode(&bytes, 1_048_576, &mut shared, &mut resources)?;
        let a = restored.cell(keys[0])?;
        let b = restored.cell(keys[1])?;
        let a = a
            .as_any()
            .downcast_ref::<Arc<StatusHistoryCell>>()
            .ok_or_else(|| native::invalid("actual native status wrapper changed"))?;
        let b = b
            .as_any()
            .downcast_ref::<Arc<StatusHistoryCell>>()
            .ok_or_else(|| native::invalid("actual native status wrapper changed"))?;
        let restored_handle = resources.status_handle(handle_ref)?;
        assert!(Arc::ptr_eq(a, b));
        assert!(Arc::ptr_eq(a, &restored_handle.card));
        assert!(!Arc::ptr_eq(a, &card));
        assert_eq!(a.model_name, card.model_name);
        assert_eq!(a.model_details, card.model_details);
        assert_eq!(a.directory, card.directory);
        assert_eq!(a.permissions, card.permissions);
        assert_eq!(a.remote_connection, card.remote_connection);
        assert_eq!(a.account, card.account);
        assert_eq!(a.thread_name, card.thread_name);
        assert_eq!(a.session_id, card.session_id);
        assert_eq!(a.forked_from, card.forked_from);
        for width in [0, 8, 24, 97] {
            assert_eq!(a.display_lines(width), card.display_lines(width));
        }
        assert_eq!(restored_handle.copy_text(), handle.copy_text());
        let summary = resources.status_summary(summary_ref)?;
        assert!(Arc::ptr_eq(&summary, &a.agents_summary));
        *summary
            .write()
            .map_err(|_| native::invalid("actual restored summary lock poisoned"))? =
            "actual newer summary".into();
        assert_eq!(
            *b.agents_summary
                .read()
                .map_err(|_| native::invalid("actual native summary lock poisoned"))?,
            "actual newer summary"
        );
        restored_handle.finish_rate_limit_refresh(&[], Local::now());
        let updated = b
            .rate_limit_state
            .read()
            .map_err(|_| native::invalid("actual restored rate lock poisoned"))?;
        // The original handle maps an absent snapshot to Missing and completes
        // refreshing; both restored history roots share this same live state.
        assert!(matches!(updated.rate_limits, StatusRateLimitData::Missing));
        assert!(!updated.refreshing_rate_limits);
        drop(updated);
        assert!(matches!(
            card.rate_limit_state
                .read()
                .map_err(|_| native::invalid("original rate lock poisoned"))?
                .rate_limits,
            StatusRateLimitData::Stale(_)
        ));
        Ok(())
    }
    #[test]
    fn equal_independent_status_cards_stay_distinct_and_missing_handle_never_defaults()
    -> io::Result<()> {
        let a = original_card();
        let b = original_card();
        let mut captured = Capture::default();
        let a_ref = captured.card(&a)?;
        let b_ref = captured.card(&b)?;
        assert_ne!(a_ref, b_ref);
        let graph = captured.export()?;
        let mut restored = Restored::default();
        restored.install(graph)?;
        let a = restored.handle(a_ref)?;
        let b = restored.handle(b_ref)?;
        assert!(!Arc::ptr_eq(&a.card, &b.card));
        assert!(!Arc::ptr_eq(&a.card.agents_summary, &b.card.agents_summary));
        assert!(!Arc::ptr_eq(
            &a.card.rate_limit_state,
            &b.card.rate_limit_state
        ));
        assert!(restored.handle(StatusRef(u32::MAX)).is_err());
        assert!(restored.summary(SummaryRef(u32::MAX)).is_err());
        let state = a
            .card
            .rate_limit_state
            .read()
            .map_err(|_| native::invalid("actual restored rate state poisoned"))?;
        let StatusRateLimitData::Stale(rows) = &state.rate_limits else {
            return Err(native::invalid("original stale state changed"));
        };
        let StatusRateLimitValue::Window { percent_used, .. } = &rows[0].value else {
            return Err(native::invalid("original window state changed"));
        };
        assert_eq!(percent_used.to_bits(), (-0.0_f64).to_bits());
        Ok(())
    }
}
