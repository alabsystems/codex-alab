// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Consumer billing reports actual debits for the displayed thread. Its scope
//! excludes separately billed descendant chats. No estimated token price enters this view.

use super::ChatWidget;
use crate::app_event::AppEvent;
use codex_alab_tui::interface::ChatCreditSummary;
use codex_alab_tui::interface::ChatCreditUpdate;
use codex_alab_tui::interface::ChatCreditUsage;
use codex_alab_tui::interface::UNREPORTED_CHAT_SPENDING;
use codex_backend_client::AnalyticsSession;
use codex_backend_client::RequestError;
use codex_backend_client::TaskCredits;
use codex_backend_client::TaskUsage;
use codex_backend_client::TaskUsageStatus;
use codex_backend_client::TaskUsageThread;
use codex_features::Feature;
use codex_protocol::ThreadId;
use codex_protocol::account::PlanType;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use tokio::sync::OnceCell;
use uuid::Uuid;

#[derive(Default)]
pub(super) struct State {
    session: Arc<OnceCell<AnalyticsSession>>,
    thread: Option<ThreadId>,
    last_attempt: Option<Instant>,
    request: Option<Uuid>,
    /// Last accepted summary and its own provider cutoff, which can be absent.
    last_read: Option<(ChatCreditSummary, Option<i64>)>,
    latest_cutoff: Option<chrono::DateTime<chrono::Utc>>,
    cached: bool,
}

impl State {
    fn start(&mut self, thread: ThreadId, now: Instant) -> Option<Uuid> {
        if self.thread != Some(thread) {
            self.thread = Some(thread);
            self.last_read = None;
            self.latest_cutoff = None;
            self.cached = false;
        } else if self
            .last_attempt
            .is_some_and(|started| now.duration_since(started) < Duration::from_secs(60))
        {
            return None;
        }
        self.last_attempt = Some(now);
        let request = Uuid::new_v4();
        self.request = Some(request);
        Some(request)
    }

    fn finish(&mut self, thread: ThreadId, request: Uuid, update: ChatCreditUpdate) -> bool {
        if self.thread != Some(thread) || self.request != Some(request) {
            return false;
        }
        self.request = None;
        match update {
            ChatCreditUpdate::Read {
                as_of: Some(cutoff),
                ..
            } if self.latest_cutoff.is_some_and(|latest| cutoff < latest) => {
                self.cached = true;
            }
            ChatCreditUpdate::Read { summary, as_of } => {
                // An undated response can update the view, but cannot supply
                // its display time or erase the latest known provider cutoff.
                self.latest_cutoff = as_of.or(self.latest_cutoff);
                self.last_read = Some((summary, as_of.map(|stamp| stamp.timestamp())));
                self.cached = false;
            }
            ChatCreditUpdate::Clear => {
                // A rejected login stays bound to its original user. Rediscover
                // the session on the next permitted poll.
                self.session = Arc::default();
                self.last_read = None;
                self.latest_cutoff = None;
                self.cached = false;
            }
            ChatCreditUpdate::TemporarilyUnavailable => self.cached = true,
        }
        true
    }

    fn displayed(&self, thread: ThreadId) -> Option<ChatCreditUsage> {
        if self.thread != Some(thread) {
            return None;
        }
        let (summary, as_of) = self.last_read.as_ref()?;
        Some(ChatCreditUsage {
            summary: summary.clone(),
            as_of: *as_of,
            retained: self.cached,
        })
    }
}

fn reported_usage(task: TaskUsage) -> ChatCreditSummary {
    let paid_source = matches!(task.usage_source.as_str(), "credits" | "plan_and_credits");
    let amount = task
        .amounts
        .balance_usage_credits
        .as_ref()
        .map(TaskCredits::as_str);
    let limit_comparison = match (
        task.amounts.five_hour_limit_percent,
        task.amounts.weekly_limit_percent,
    ) {
        (Some(short), Some(weekly)) => Some(format!("5h {short}% · weekly {weekly}%")),
        (Some(short), None) => Some(format!("5h {short}%")),
        (None, Some(weekly)) => Some(format!("weekly {weekly}%")),
        (None, None) => None,
    };
    let (mut label, credit_activity) = match amount {
        Some(value) if codex_alab_tui::interface::credit_amount_is_nonzero(value) => {
            if let Some(refund) = value.strip_prefix('-') {
                (format!("Net refund · {refund} credits for this chat"), true)
            } else {
                (format!("{value} credits used in this chat"), true)
            }
        }
        Some(_) if task.usage_source == "included_plan" => (
            "Covered by plan · 0 net credits used in this chat".into(),
            false,
        ),
        Some(_) => ("0 net credits used in this chat".into(), paid_source),
        None => {
            let label = match task.usage_source.as_str() {
                "included_plan" => "Plan used in this chat · credit amount unreported",
                "plan_and_credits" => {
                    "Plan and credits used in this chat · credit amount unreported"
                }
                "credits" => "Credits used in this chat · credit amount unreported",
                _ => UNREPORTED_CHAT_SPENDING,
            };
            (label.into(), paid_source)
        }
    };
    if amount.is_some() && task.usage_source == "plan_and_credits" {
        label.push_str(" · plan also used");
    }
    let text = if task.data_status == TaskUsageStatus::Partial {
        format!("Partial · {label}")
    } else {
        label
    };
    ChatCreditSummary {
        text,
        limit_comparison,
        credit_activity,
    }
}

fn report_for_thread(
    response: codex_backend_client::TaskUsageResponse,
    thread: ThreadId,
) -> ChatCreditUpdate {
    let row = response
        .threads
        .into_iter()
        .find(|row| row.thread_id == thread.to_string());
    let summary = match row {
        Some(row) if row.data_status == TaskUsageStatus::Unavailable => {
            return ChatCreditUpdate::TemporarilyUnavailable;
        }
        Some(row) => reported_usage(row),
        None => ChatCreditSummary {
            text: UNREPORTED_CHAT_SPENDING.to_owned(),
            limit_comparison: None,
            credit_activity: false,
        },
    };
    let as_of = response
        .data_as_of
        .as_deref()
        .and_then(|stamp| chrono::DateTime::parse_from_rfc3339(stamp).ok())
        .map(|stamp| stamp.to_utc());
    ChatCreditUpdate::Read { summary, as_of }
}

fn temporary_failure(error: &RequestError) -> bool {
    if let Some(status) = error.status() {
        return status.is_server_error() || matches!(status.as_u16(), 408 | 429);
    }
    matches!(error, RequestError::Other(cause) if matches!(
        cause.downcast_ref::<codex_http_client::HttpError>(),
        Some(codex_http_client::HttpError::Request(_) | codex_http_client::HttpError::Timeout)
    ))
}

fn personal_plan(plan: Option<PlanType>) -> bool {
    plan.is_some_and(|plan| plan != PlanType::Unknown && !plan.is_workspace_account())
}

impl ChatWidget {
    pub(super) fn alab_check_thread_credits(
        &mut self,
        account: &str,
        snapshot: &codex_app_server_protocol::RateLimitSnapshot,
    ) {
        let Some(thread) = self.thread_id else { return };
        if !codex_alab_tui::interface::enabled()
            || !self.alab_local_account_server()
            || !personal_plan(self.alab_allowance.observed_plan)
        {
            self.alab_allowance.thread_credits = State::default();
            return;
        }
        let inspecting = self.alab_account_details_is_active()
            && self.should_prefetch_rate_limits()
            && self.has_codex_backend_auth;
        let tracked = &mut self.alab_allowance.thread_credits;
        let exhausted = [&snapshot.primary, &snapshot.secondary]
            .into_iter()
            .flatten()
            .any(|window| window.used_percent >= 100);
        if !inspecting
            && !exhausted
            && !snapshot
                .credits
                .as_ref()
                .is_some_and(|c| c.has_credits || c.unlimited)
            && (tracked.thread != Some(thread)
                || tracked
                    .last_read
                    .as_ref()
                    .is_none_or(|(summary, _)| !summary.credit_activity))
        {
            return;
        }
        let Some(request) = tracked.start(thread, Instant::now()) else {
            return;
        };
        let session = Arc::clone(&tracked.session);
        let config = self.config.clone();
        let account = account.to_owned();
        let events = self.app_event_tx.clone();
        tokio::spawn(async move {
            let query = async {
                let session = session
                    .get_or_try_init(|| async {
                        let opened = AnalyticsSession::from_config(
                            &config,
                            config.http_client_factory(),
                            config
                                .features
                                .enabled(Feature::RuntimeAgentInstances)
                                .then_some(config.runtime_auth_mode),
                        )
                        .await
                        .map_err(|_| ())?;
                        // Cache a session only when account discovery also confirms
                        // that the selected account can use consumer chat billing.
                        if opened.account().id == account
                            && personal_plan(opened.account().plan_type)
                        {
                            Ok(opened)
                        } else {
                            Err(())
                        }
                    })
                    .await
                    .ok()?;
                let response = session
                    .request(|client| async move {
                        let threads = [TaskUsageThread {
                            thread_id: thread.to_string(),
                            created_at: None,
                            descendant_thread_ids: Vec::new(),
                        }];
                        // Leave time inside the overall deadline for request() to
                        // validate the identity again, including after a timeout.
                        match tokio::time::timeout(
                            Duration::from_secs(12),
                            client.get_task_usage(&threads),
                        )
                        .await
                        {
                            Ok(Ok(response)) => Ok(report_for_thread(response, thread)),
                            Ok(Err(error)) if !temporary_failure(&error) => Err(error),
                            _ => Ok(ChatCreditUpdate::TemporarilyUnavailable),
                        }
                    })
                    .await;
                response.ok()
            };
            let usage = tokio::time::timeout(Duration::from_secs(15), query)
                .await
                .ok()
                .flatten()
                .unwrap_or(ChatCreditUpdate::Clear);
            events.send(AppEvent::AlabThreadCredits {
                request,
                thread_id: thread,
                usage,
            });
        });
    }

    pub(crate) fn alab_finish_thread_credits(
        &mut self,
        request: Uuid,
        thread: ThreadId,
        usage: ChatCreditUpdate,
    ) {
        if self
            .alab_allowance
            .thread_credits
            .finish(thread, request, usage)
            && self.thread_id == Some(thread)
        {
            self.alab_refresh_interface();
            self.request_redraw();
        }
    }

    pub(in crate::chatwidget) fn alab_thread_credit_notice(&self) -> Option<&'static str> {
        let thread = self.thread_id?;
        let state = &self.alab_allowance.thread_credits;
        if state.thread != Some(thread) || state.last_read.is_some() {
            return None;
        }
        if state.request.is_some() {
            return Some("Checking chat spending…");
        }
        Some(if state.cached {
            "Chat spending temporarily unavailable"
        } else {
            "Chat spending unavailable"
        })
    }

    pub(in crate::chatwidget) fn alab_thread_credit_usage(&self) -> Option<ChatCreditUsage> {
        self.alab_allowance
            .thread_credits
            .displayed(self.thread_id?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reported(source: &str, amount: Option<&str>, status: &str) -> ChatCreditSummary {
        let task = serde_json::from_value(serde_json::json!({
            "thread_id":"chat", "usage_source":source, "data_status":status,
            "balance_usage_credits":amount, "groups":[]
        }))
        .unwrap();
        reported_usage(task)
    }

    #[test]
    fn backend_reporting_time_stays_independent_of_poll_time() {
        let thread = ThreadId::new();
        for (stamp, expected) in [
            (Some("2026-11-01T11:00:00-08:00"), Some(1793559600)),
            (Some("unavailable"), None),
            (None, None),
        ] {
            let response = serde_json::from_value(serde_json::json!({
                "data_as_of":stamp, "threads":[{
                    "thread_id":thread.to_string(), "usage_source":"credits", "data_status":"partial",
                    "balance_usage_credits":"12.72643", "weekly_limit_percent":1.2, "groups":[]
                }]
            })).unwrap();
            let mut state = State::default();
            let request = state.start(thread, Instant::now()).unwrap();
            state.finish(thread, request, report_for_thread(response, thread));
            let report = state.displayed(thread).unwrap();
            assert_eq!(report.as_of, expected);
            assert_eq!(
                report.summary.limit_comparison.as_deref(),
                Some("weekly 1.2%")
            );
            assert_eq!(
                report.summary.text,
                "Partial · 12.72643 credits used in this chat"
            );
        }
    }

    #[test]
    fn only_backend_debits_or_explicit_credit_use_identify_billing() {
        for (source, amount, status, text, activity) in [
            (
                "accounting",
                Some("2.5"),
                "available",
                "2.5 credits used in this chat",
                true,
            ),
            (
                "plan_and_credits",
                Some("12.72643"),
                "partial",
                "Partial · 12.72643 credits used in this chat · plan also used",
                true,
            ),
            (
                "plan_and_credits",
                Some("0E-10"),
                "available",
                "0 net credits used in this chat · plan also used",
                true,
            ),
            (
                "plan_and_credits",
                Some("-1.25"),
                "available",
                "Net refund · 1.25 credits for this chat · plan also used",
                true,
            ),
            (
                "plan_and_credits",
                None,
                "partial",
                "Partial · Plan and credits used in this chat · credit amount unreported",
                true,
            ),
            (
                "included_plan",
                None,
                "available",
                "Plan used in this chat · credit amount unreported",
                false,
            ),
            (
                "credits",
                None,
                "available",
                "Credits used in this chat · credit amount unreported",
                true,
            ),
            (
                "accounting",
                Some("-0.000003"),
                "available",
                "Net refund · 0.000003 credits for this chat",
                true,
            ),
            (
                "credits",
                Some("0E-10"),
                "available",
                "0 net credits used in this chat",
                true,
            ),
            (
                "accounting",
                Some("1e-400"),
                "available",
                "1e-400 credits used in this chat",
                true,
            ),
            (
                "accounting",
                Some("-0E+5"),
                "partial",
                "Partial · 0 net credits used in this chat",
                false,
            ),
            (
                "included_plan",
                Some("0E-10"),
                "available",
                "Covered by plan · 0 net credits used in this chat",
                false,
            ),
        ] {
            let summary = reported(source, amount, status);
            assert_eq!(summary.text, text);
            assert_eq!(summary.credit_activity, activity);
        }
        let partial = reported("unknown", None, "partial");
        assert_eq!(partial.text, "Partial · Chat spending unreported");
        assert!(!partial.credit_activity);
    }

    #[test]
    fn limit_comparisons_do_not_imply_a_credit_charge() {
        for (short, weekly, expected) in [
            (Some(0.0), None, "5h 0%"),
            (None, Some(0.004), "weekly 0.004%"),
        ] {
            let row = serde_json::from_value(serde_json::json!({
                "thread_id":"chat", "data_status":"partial", "usage_source":"unknown",
                "five_hour_limit_percent":short, "weekly_limit_percent":weekly, "groups":[]
            }))
            .unwrap();
            let summary = reported_usage(row);
            assert_eq!(summary.text, "Partial · Chat spending unreported");
            assert_eq!(summary.limit_comparison.as_deref(), Some(expected));
            assert!(!summary.credit_activity);
        }
    }

    fn configure_billing(
        chat: &mut ChatWidget,
        home: &std::path::Path,
        server: &wiremock::MockServer,
    ) {
        chat.config.codex_home = home.to_path_buf().try_into().unwrap();
        chat.config.chatgpt_base_url = format!("{}/backend-api", server.uri());
        chat.config.cli_auth_credentials_store_mode = codex_login::AuthCredentialsStoreMode::File;
        crate::chatwidget::tests::set_chatgpt_auth(chat);
        chat.requires_openai_auth = true;
    }

    fn sign_in(home: &std::path::Path, account: &str, user: &str) {
        app_test_support::write_chatgpt_auth(
            home,
            app_test_support::ChatGptAuthFixture::new(format!("fixture-{user}"))
                .account_id(account)
                .chatgpt_account_id(account)
                .chatgpt_user_id(user)
                .plan_type("plus"),
            codex_login::AuthCredentialsStoreMode::File,
        )
        .unwrap();
    }

    async fn apply_billing_event(
        chat: &mut ChatWidget,
        events: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    ) {
        let pending = chat
            .alab_allowance
            .thread_credits
            .request
            .expect("pending billing request");
        tokio::time::timeout(Duration::from_secs(5), async {
            while let Some(event) = events.recv().await {
                if let AppEvent::AlabThreadCredits {
                    request,
                    thread_id,
                    usage,
                } = event
                {
                    assert_eq!((request, Some(thread_id)), (pending, chat.thread_id));
                    chat.alab_finish_thread_credits(request, thread_id, usage);
                    return;
                }
            }
            panic!("account event stream closed");
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn inspector_reads_chat_billing_with_plan_allowance_and_no_spendable_credits() {
        use crate::chatwidget::tests::make_chatwidget_manual_with_sender;
        use crate::chatwidget::tests::render_bottom_popup;
        use codex_alab_tui::interface;
        use serde_json::json;
        use wiremock::Mock;
        use wiremock::MockServer;
        use wiremock::ResponseTemplate;
        use wiremock::matchers::body_json;
        use wiremock::matchers::header;
        use wiremock::matchers::method;
        use wiremock::matchers::path;

        let _scope = interface::scoped(Box::leak(Box::new(interface::State::new())));
        interface::configure(true);
        let (mut chat, _, mut events, _) = make_chatwidget_manual_with_sender().await;
        let home = tempfile::tempdir().unwrap();
        let server = MockServer::start().await;
        let thread = ThreadId::new();
        let account = "inspected-account";
        sign_in(home.path(), account, "user");
        configure_billing(&mut chat, home.path(), &server);
        chat.thread_id = Some(thread);
        Mock::given(method("POST")).and(path("/backend-api/wham/usage/thread_usage/query_v2"))
            .and(header("chatgpt-account-id", account))
            .and(body_json(json!({"threads":[{"thread_id":thread.to_string(),"created_at":null,"descendant_thread_ids":[]}]})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data_as_of":"2026-11-01T19:00:00Z", "threads":[{
                    "thread_id":thread.to_string(),"usage_source":"included_plan","data_status":"available",
                    "balance_usage_credits":"0","weekly_limit_percent":125,"five_hour_limit_percent":0.125,"groups":[]
                }]
            }))).expect(1).mount(&server).await;

        let limits = serde_json::from_value(json!({
            "accountId":account, "rateLimits":{"limitId":"codex","planType":"plus",
                "primary":{"usedPercent":20,"windowDurationMins":300},
                "credits":{"hasCredits":false,"unlimited":false,"balance":"0"}}
        }))
        .unwrap();
        chat.alab_account_usage_read_at(&limits, 1);
        assert!(chat.alab_allowance.thread_credits.request.is_none());
        let mut sparse = limits.clone();
        sparse.rate_limits.plan_type = None;
        chat.alab_open_account_details();
        // The quota read already identifies a personal plan. Account discovery
        // can disagree, then confirm that plan on the next allowed refresh.
        for plan in ["business", "plus"] {
            let _discovery = Mock::given(method("GET"))
                .and(path("/backend-api/wham/accounts/check"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "accounts":[{"id":account,"plan_type":plan}], "account_ordering":[account]
                })))
                .expect(1)
                .mount_as_scoped(&server)
                .await;
            chat.alab_account_usage_read_at(&sparse, 2);
            let pending = chat
                .alab_allowance
                .thread_credits
                .request
                .expect("inspector requested billing");
            chat.alab_account_usage_read_at(&sparse, 3);
            assert_eq!(chat.alab_allowance.thread_credits.request, Some(pending));
            // The native account-read handler publishes the snapshot before consuming
            // the billing result. The open inspector should reflect that request.
            chat.on_rate_limit_snapshot(Some(sparse.rate_limits.clone()));
            let shown = render_bottom_popup(&chat, 85);
            assert!(shown.contains("Checking chat spending…"), "{shown}");
            assert!(!shown.contains("Chat spending unreported"), "{shown}");

            apply_billing_event(&mut chat, &mut events).await;

            if plan == "business" {
                assert!(chat.alab_thread_credit_usage().is_none());
                let shown = render_bottom_popup(&chat, 85);
                assert!(shown.contains("Chat spending unavailable"), "{shown}");
                assert!(!shown.contains("Checking chat spending"), "{shown}");
                assert!(!shown.contains(UNREPORTED_CHAT_SPENDING), "{shown}");
                assert!(chat.alab_allowance.thread_credits.session.get().is_none());
                chat.alab_allowance.thread_credits.last_attempt = None;
            }
        }
        let shown = render_bottom_popup(&chat, 85);
        assert!(shown.contains("Covered by plan · 0 net credits"), "{shown}");
        assert!(!shown.contains("Chat spending unavailable"), "{shown}");
        assert!(!shown.contains("Checking chat spending…"), "{shown}");
        assert!(
            shown.contains("Chat usage vs. current limits · 5h 0.125% · weekly 125%"),
            "{shown}"
        );

        chat.alab_close_account_details();
        chat.alab_allowance.thread_credits.last_attempt = None;
        chat.alab_account_usage_read_at(&limits, 4);
        assert!(chat.alab_allowance.thread_credits.request.is_none());
    }

    #[tokio::test]
    async fn a_first_billing_outage_and_a_completed_empty_report_have_distinct_states() {
        use crate::chatwidget::tests::make_chatwidget_manual_with_sender;
        use crate::chatwidget::tests::render_bottom_popup;
        use codex_alab_tui::interface;

        let _scope = interface::scoped(Box::leak(Box::new(interface::State::new())));
        interface::configure(true);
        let (mut chat, _, _, _) = make_chatwidget_manual_with_sender().await;
        chat.requires_openai_auth = true;
        chat.update_account_state(
            Some(crate::status::StatusAccountDisplay::ChatGpt {
                email: Some("person@example.test".into()),
                plan: None,
                workspace: None,
            }),
            Some(PlanType::Plus),
            true,
            true,
        );
        let thread = ThreadId::new();
        chat.thread_id = Some(thread);
        chat.alab_open_account_details();
        let now = Instant::now();
        let request = chat
            .alab_allowance
            .thread_credits
            .start(thread, now)
            .unwrap();
        chat.alab_finish_thread_credits(request, thread, ChatCreditUpdate::TemporarilyUnavailable);
        let shown = render_bottom_popup(&chat, 85);
        assert!(
            shown.contains("Chat spending temporarily unavailable"),
            "{shown}"
        );
        assert!(chat.alab_thread_credit_usage().is_none());
        chat.thread_id = Some(ThreadId::new());
        assert!(chat.alab_thread_credit_notice().is_none());
        chat.thread_id = Some(thread);

        let request = chat
            .alab_allowance
            .thread_credits
            .start(thread, now + Duration::from_secs(60))
            .unwrap();
        assert_eq!(
            chat.alab_thread_credit_notice(),
            Some("Checking chat spending…")
        );
        chat.alab_finish_thread_credits(
            request,
            thread,
            report_for_thread(
                serde_json::from_value(serde_json::json!({
                    "data_as_of":"2026-11-01T19:00:00Z", "threads":[]
                }))
                .unwrap(),
                thread,
            ),
        );
        assert!(chat.alab_thread_credit_notice().is_none());
        let shown = render_bottom_popup(&chat, 85);
        assert!(shown.contains("Chat spending unreported"), "{shown}");
        assert!(shown.contains("Chat usage reported through"), "{shown}");
        assert!(!shown.contains("0 net credits"), "{shown}");
        assert!(!shown.contains("temporarily unavailable"), "{shown}");
        assert!(!shown.contains("Checking chat spending"), "{shown}");
    }

    #[tokio::test]
    async fn finishing_a_billing_read_after_switching_chats_settles_only_the_requesting_chat() {
        use crate::chatwidget::tests::make_chatwidget_manual_with_sender;

        let (mut chat, _, _, _) = make_chatwidget_manual_with_sender().await;
        let original = ThreadId::new();
        let other = ThreadId::new();
        chat.thread_id = Some(original);
        let request = chat
            .alab_allowance
            .thread_credits
            .start(original, Instant::now())
            .unwrap();
        chat.thread_id = Some(other);
        chat.alab_finish_thread_credits(
            request,
            original,
            ChatCreditUpdate::Read {
                summary: reported("credits", Some("2.5"), "available"),
                as_of: None,
            },
        );
        assert!(chat.alab_thread_credit_usage().is_none());
        assert!(chat.alab_thread_credit_notice().is_none());
        chat.thread_id = Some(original);
        assert!(chat.alab_allowance.thread_credits.request.is_none());
        assert_eq!(
            chat.alab_thread_credit_usage().unwrap().summary.text,
            "2.5 credits used in this chat"
        );
    }

    #[tokio::test]
    async fn billing_rediscovers_a_changed_login_on_the_next_permitted_refresh() {
        use crate::chatwidget::tests::make_chatwidget_manual_with_sender;
        use codex_alab_tui::interface;
        use serde_json::json;
        use wiremock::Mock;
        use wiremock::MockServer;
        use wiremock::ResponseTemplate;
        use wiremock::matchers::header;
        use wiremock::matchers::method;
        use wiremock::matchers::path;

        let _scope = interface::scoped(Box::leak(Box::new(interface::State::new())));
        interface::configure(true);
        let (mut chat, _, mut events, _) = make_chatwidget_manual_with_sender().await;
        let home = tempfile::tempdir().unwrap();
        let server = MockServer::start().await;
        let thread = ThreadId::new();
        let account = "shared-account-id";
        sign_in(home.path(), account, "first");
        configure_billing(&mut chat, home.path(), &server);
        chat.thread_id = Some(thread);
        Mock::given(method("GET"))
            .and(path("/backend-api/wham/accounts/check"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "accounts":[{"id":account,"plan_type":"plus"}], "account_ordering":[account]
            })))
            .expect(2)
            .mount(&server)
            .await;
        for (user, debit) in [("first", "2.5"), ("second", "1.25")] {
            Mock::given(method("POST"))
                .and(path("/backend-api/wham/usage/thread_usage/query_v2"))
                .and(header("authorization", format!("Bearer fixture-{user}")))
                .and(header("chatgpt-account-id", account))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "threads":[{"thread_id":thread.to_string(),"usage_source":"credits",
                        "data_status":"available","balance_usage_credits":debit,"groups":[]}]
                })))
                .expect(1)
                .mount(&server)
                .await;
        }
        let limits = serde_json::from_value(json!({
            "accountId":account, "rateLimits":{"limitId":"codex","planType":"plus",
                "credits":{"hasCredits":true,"unlimited":false,"balance":"10"}}
        }))
        .unwrap();
        chat.alab_account_usage_read_at(&limits, 1);
        apply_billing_event(&mut chat, &mut events).await;
        assert_eq!(
            chat.alab_thread_credit_usage().unwrap().summary.text,
            "2.5 credits used in this chat"
        );

        sign_in(home.path(), account, "second");
        chat.alab_allowance.thread_credits.last_attempt = None;
        chat.alab_account_usage_read_at(&limits, 2);
        apply_billing_event(&mut chat, &mut events).await;
        assert!(chat.alab_thread_credit_usage().is_none());
        assert_eq!(
            chat.alab_thread_credit_notice(),
            Some("Chat spending unavailable")
        );
        chat.alab_account_usage_read_at(&limits, 3);
        assert!(chat.alab_allowance.thread_credits.request.is_none());

        chat.alab_allowance.thread_credits.last_attempt = None;
        chat.alab_account_usage_read_at(&limits, 4);
        apply_billing_event(&mut chat, &mut events).await;
        assert_eq!(
            chat.alab_thread_credit_usage().unwrap().summary.text,
            "1.25 credits used in this chat"
        );
    }

    #[tokio::test]
    async fn a_confirmed_charge_survives_an_omitted_plan_only_for_the_same_personal_account() {
        use crate::chatwidget::tests::make_chatwidget_manual_with_sender;
        use crate::chatwidget::tests::set_chatgpt_auth;
        use codex_alab_tui::interface;
        use serde_json::json;

        let _scope = interface::scoped(Box::leak(Box::new(interface::State::new())));
        interface::configure(true);
        let (mut chat, _, _, _) = make_chatwidget_manual_with_sender().await;
        set_chatgpt_auth(&mut chat);
        let thread = ThreadId::new();
        chat.thread_id = Some(thread);
        let limits = |account: &str, plan: Option<PlanType>| {
            serde_json::from_value(json!({
                "accountId":account, "rateLimits":{"limitId":"codex","planType":plan}
            }))
            .unwrap()
        };
        let charge = || ChatCreditUpdate::Read {
            summary: reported("credits", Some("2.5"), "available"),
            as_of: chrono::DateTime::from_timestamp(42, 0),
        };
        for next in [
            limits("personal", Some(PlanType::Unknown)),
            limits("personal", Some(PlanType::Business)),
            limits("other", None),
        ] {
            chat.alab_account_usage_read_at(&limits("personal", Some(PlanType::Plus)), 1);
            let state = &mut chat.alab_allowance.thread_credits;
            let request = state.start(thread, Instant::now()).unwrap();
            assert!(state.finish(thread, request, charge()));
            let report = chat.alab_thread_credit_usage().unwrap();
            chat.alab_account_usage_read_at(&limits("personal", None), 2);
            assert_eq!(chat.alab_thread_credit_usage(), Some(report));

            // Leave a request in flight while the account or plan changes.
            let state = &mut chat.alab_allowance.thread_credits;
            state.last_attempt = None;
            let pending = state.start(thread, Instant::now()).unwrap();
            chat.alab_account_usage_read_at(&next, 3);
            chat.alab_finish_thread_credits(pending, thread, charge());
            assert!(chat.alab_thread_credit_usage().is_none());
            assert!(chat.alab_allowance.thread_credits.request.is_none());
        }
    }

    #[tokio::test]
    async fn plan_usage_stays_in_the_inspector_while_the_footer_shows_the_balance() {
        use crate::chatwidget::tests::make_chatwidget_manual_with_sender;
        use crate::chatwidget::tests::render_bottom_popup;
        use codex_alab_tui::banner::Depth;
        use codex_alab_tui::banner::Theme;
        use codex_alab_tui::interface;
        use ratatui::buffer::Buffer;
        use ratatui::buffer::Cell;
        use ratatui::layout::Rect;

        let _scope = interface::scoped(Box::leak(Box::new(interface::State::new())));
        interface::configure(true);
        let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
        chat.alab_open_account_details();
        let thread = ThreadId::new();
        let now = Instant::now();
        chat.thread_id = Some(thread);
        let response = serde_json::from_value(serde_json::json!({
            "data_as_of":"2026-11-01T19:00:00Z", "threads":[{
                "thread_id":thread.to_string(), "usage_source":"included_plan", "data_status":"available",
                "balance_usage_credits":"0E-10", "groups":[]
            }]
        })).unwrap();
        let unquantified = serde_json::from_value(serde_json::json!({
            "data_as_of":"2026-11-01T19:05:00Z", "threads":[{
                "thread_id":thread.to_string(), "usage_source":"included_plan", "data_status":"partial",
                "groups":[]
            }]
        })).unwrap();
        for (minute, update, expected) in [
            (
                0,
                report_for_thread(response, thread),
                "Covered by plan · 0 net credits",
            ),
            (
                1,
                ChatCreditUpdate::TemporarilyUnavailable,
                "Last report · Covered by plan · 0 net credits",
            ),
            (
                2,
                report_for_thread(unquantified, thread),
                "Partial · Plan used in this chat · credit amount unreported",
            ),
            (3, ChatCreditUpdate::Clear, "Chat spending unavailable"),
        ] {
            let state = &mut chat.alab_allowance.thread_credits;
            let request = state
                .start(thread, now + Duration::from_secs(minute * 60))
                .unwrap();
            state.finish(thread, request, update);
            interface::set_status(interface::InterfaceStatus {
                account: "person@example.test".into(),
                credit_history: Some(interface::CreditHistory::PersonalChats),
                credit_balance: Some(interface::CreditBalance::Current("25 credits left".into())),
                thread_credit_usage: chat.alab_thread_credit_usage(),
                thread_credit_notice: chat.alab_thread_credit_notice(),
                ..Default::default()
            });
            let inspector = render_bottom_popup(&chat, 85);
            assert!(inspector.contains(expected), "{inspector}");
            assert_eq!(
                inspector.contains("Chat usage reported through"),
                minute < 3,
                "{inspector}"
            );
            let area = Rect::new(0, 0, 30, 1);
            let mut buf = Buffer::empty(area);
            interface::render_status_current(area, &mut buf, Depth::Mono, Theme::default());
            let footer: String = buf.content.iter().map(Cell::symbol).collect();
            assert!(footer.contains("25 credits left"), "{footer}");
            assert!(!footer.contains("Covered by plan"), "{footer}");
        }
    }

    #[test]
    fn narrow_footer_labels_partial_charges_and_refunds_before_the_amount() {
        use codex_alab_tui::banner::Depth;
        use codex_alab_tui::banner::Theme;
        use codex_alab_tui::interface;
        use ratatui::buffer::Buffer;
        use ratatui::buffer::Cell;
        use ratatui::layout::Rect;

        let _scope = interface::scoped(Box::leak(Box::new(interface::State::new())));
        interface::configure(true);
        let thread = ThreadId::new();
        let mut state = State::default();
        let now = Instant::now();
        let initial = state.start(thread, now).unwrap();
        let update = ChatCreditUpdate::Read {
            summary: reported("plan_and_credits", Some("12.72643"), "partial"),
            as_of: chrono::DateTime::from_timestamp(1793559600, 0),
        };
        state.finish(thread, initial, update);
        let fresh = state.displayed(thread);
        let refresh = state.start(thread, now + Duration::from_secs(60)).unwrap();
        state.finish(thread, refresh, ChatCreditUpdate::TemporarilyUnavailable);
        let cached = state.displayed(thread);
        let correction = state.start(thread, now + Duration::from_secs(120)).unwrap();
        state.finish(
            thread,
            correction,
            ChatCreditUpdate::Read {
                summary: reported("credits", Some("-1e-40"), "available"),
                as_of: None,
            },
        );
        let balance = Some(interface::CreditBalance::Current("25 credits left".into()));
        for (width, report, balance, expected) in [
            (24, fresh, balance.clone(), "Partial · 12.72643"),
            (38, cached.clone(), None, "Last report · Partial · 12.72643"),
            (24, cached.clone(), balance.clone(), "25 credits left"),
            (
                120,
                cached.clone(),
                balance.clone(),
                "25 credits left · Last report · Partial · 12.72643",
            ),
            (
                38,
                cached.clone(),
                Some(interface::CreditBalance::LastReported(
                    "Last check · 25 credits".into(),
                )),
                "Last report · Partial · 12.72643",
            ),
            (
                38,
                cached,
                Some(interface::CreditBalance::LastReported(
                    "Stale balance · 25 credits".into(),
                )),
                "Last report · Partial · 12.72643",
            ),
            (24, state.displayed(thread), balance, "Net refund · 1e-40"),
        ] {
            interface::set_status(interface::InterfaceStatus {
                account: "person@example.test".into(),
                credit_balance: balance,
                thread_credit_usage: report,
                ..Default::default()
            });
            let area = Rect::new(0, 0, width, 1);
            let mut buf = Buffer::empty(area);
            interface::render_status_current(area, &mut buf, Depth::Mono, Theme::default());
            let text: String = buf.content.iter().map(Cell::symbol).collect();
            assert!(text.contains(expected), "{text}");
        }
    }

    #[test]
    fn only_temporary_service_errors_can_keep_a_report() {
        for (code, temporary) in [
            (408, true),
            (429, true),
            (503, true),
            (401, false),
            (403, false),
            (404, false),
        ] {
            let error = RequestError::UnexpectedStatus {
                method: "POST".into(),
                url: String::new(),
                status: http::StatusCode::from_u16(code).unwrap(),
                content_type: String::new(),
                body: String::new(),
                identity_error_code: None,
            };
            assert_eq!(temporary_failure(&error), temporary, "{code}");
        }
        assert!(temporary_failure(&RequestError::from(
            codex_http_client::HttpError::Timeout
        )));
        assert!(!temporary_failure(&RequestError::Other(anyhow::anyhow!(
            "account changed"
        ))));
    }

    #[test]
    fn temporary_outage_keeps_the_last_report_until_the_service_answers() {
        let thread = ThreadId::new();
        let now = Instant::now();
        let read = || ChatCreditUpdate::Read {
            summary: reported("credits", Some("2.5"), "available"),
            as_of: chrono::DateTime::from_timestamp(42, 0),
        };
        let mut state = State::default();
        let initial = state.start(thread, now).unwrap();
        state.finish(thread, initial, read());
        let report = state.displayed(thread).unwrap();
        let unavailable = serde_json::from_value(serde_json::json!({
            "data_as_of":"2027-01-01T00:00:00Z", "threads":[{
                "thread_id":thread.to_string(), "usage_source":"credits",
                "data_status":"unavailable", "balance_usage_credits":"20", "groups":[]
            }]
        }))
        .unwrap();
        for (minute, update) in [
            (1, ChatCreditUpdate::TemporarilyUnavailable),
            (2, report_for_thread(unavailable, thread)),
        ] {
            let request = state
                .start(thread, now + Duration::from_secs(60 * minute))
                .unwrap();
            state.finish(thread, request, update);
            let cached = state.displayed(thread).unwrap();
            assert_eq!(
                cached.label(),
                "Last report · 2.5 credits used in this chat"
            );
            assert_eq!(cached.as_of, report.as_of);
            assert!(state.displayed(ThreadId::new()).is_none());
        }
        let recovered = state.start(thread, now + Duration::from_secs(180)).unwrap();
        state.finish(thread, recovered, read());
        assert_eq!(state.displayed(thread), Some(report));
        let cleared = state.start(thread, now + Duration::from_secs(240)).unwrap();
        state.finish(thread, cleared, ChatCreditUpdate::Clear);
        assert!(state.displayed(thread).is_none());
    }

    #[test]
    fn provider_cutoff_orders_debits_refunds_and_empty_reports_within_a_chat() {
        let mut state = State::default();
        let mut next_poll = Instant::now();
        let mut poll = |thread: ThreadId, stamp: Option<&str>, amount: Option<&str>| {
            let rows: Vec<_> = amount.into_iter().map(|amount| serde_json::json!({
                "thread_id":thread.to_string(), "usage_source":"accounting", "data_status":"available",
                "balance_usage_credits":amount, "groups":[]
            })).collect();
            let response = serde_json::from_value(serde_json::json!({
                "data_as_of":stamp, "threads":rows
            }))
            .unwrap();
            let request = state.start(thread, next_poll).unwrap();
            next_poll += Duration::from_secs(60);
            assert!(state.finish(thread, request, report_for_thread(response, thread)));
            state.displayed(thread)
        };
        let thread = ThreadId::new();
        let older = Some("2026-11-01T19:00:00.500Z");
        let current = Some("2026-11-01T19:00:00.501Z");
        let newer = Some("2026-11-01T19:00:00.502Z");
        let first = poll(thread, current, Some("8")).unwrap();
        for stale in [Some("3"), None] {
            let retained = poll(thread, older, stale).unwrap();
            assert_eq!(
                retained.label(),
                "Last report · 8 credits used in this chat"
            );
            assert_eq!(retained.as_of, first.as_of);
        }
        // An equal cutoff can correct the earlier amount; a newer one can report a refund.
        assert_eq!(
            poll(thread, Some("2026-11-01T11:00:00.501-08:00"), Some("7"))
                .unwrap()
                .summary
                .text,
            "7 credits used in this chat"
        );
        assert_eq!(
            poll(thread, newer, Some("-2")).unwrap().summary.text,
            "Net refund · 2 credits for this chat"
        );
        let empty = poll(thread, newer, None).unwrap();
        assert_eq!(empty.summary.text, "Chat spending unreported");
        assert!(!empty.summary.credit_activity);
        assert_eq!(empty.as_of, Some(1793559600));
        let cached = poll(thread, current, Some("8")).unwrap();
        assert_eq!(cached.label(), "Last report · Chat spending unreported");
        assert_eq!(cached.as_of, empty.as_of);
        let undated_empty = poll(thread, None, None).unwrap();
        assert_eq!(undated_empty.as_of, None);
        assert_eq!(undated_empty.summary.text, "Chat spending unreported");

        let undated = poll(thread, None, Some("4")).unwrap();
        assert_eq!(undated.as_of, None);
        let retained = poll(thread, older, Some("10")).unwrap();
        assert_eq!(
            retained.label(),
            "Last report · 4 credits used in this chat"
        );
        assert_eq!(retained.as_of, None);
        assert_eq!(
            poll(ThreadId::new(), older, Some("1.5"))
                .unwrap()
                .summary
                .text,
            "1.5 credits used in this chat"
        );
    }

    #[test]
    fn accepted_read_is_bounded_and_belongs_to_current_thread_and_account() {
        let a = ThreadId::new();
        let b = ThreadId::new();
        let now = Instant::now();
        let mut state = State::default();
        let usage = || ChatCreditUpdate::Read {
            summary: reported("credits", Some("2.5"), "available"),
            as_of: None,
        };
        let old = state.start(a, now).unwrap();
        assert!(state.start(a, now + Duration::from_secs(59)).is_none());
        let new = state.start(a, now + Duration::from_secs(60)).unwrap();
        assert!(!state.finish(a, old, usage()));
        assert!(state.finish(a, new, usage()));
        let other = state.start(b, now + Duration::from_secs(60)).unwrap();
        assert!(state.last_read.is_none());
        assert!(!state.finish(a, new, usage()));
        let mut next_account = State::default();
        next_account.start(b, now);
        assert!(!next_account.finish(b, other, usage()));
    }
}
