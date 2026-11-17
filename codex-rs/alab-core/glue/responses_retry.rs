// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Compiled inside core's Responses retry module so upstream needs only a thin hook.

use super::ResponsesStreamRequest;
use super::ResponsesStreamRetryState;
use crate::client::ModelClientSession;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use codex_alab_core::capacity::CapacityBackoff;
use codex_alab_core::capacity::CapacityReason;
use codex_alab_core::capacity::InFlightRetryObserver;
use codex_alab_core::capacity::LiveCapacityWait;
use codex_alab_core::capacity::SamplingRetryAdvice;
use codex_alab_core::capacity::permanent_client_status;
use codex_async_utils::CancelErr;
use codex_client::RetryOperation;
use codex_features::Feature;
use codex_protocol::error::CodexErr;
use codex_protocol::error::CodexErrorDetails;
use codex_protocol::protocol::CodexErrorInfo;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::StreamErrorEvent;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

#[path = "reset_notices.rs"]
mod reset_notices;

/// What follows an opaque rejection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OpaqueReplay {
    /// The same request again, routing token included.
    Unchanged,
    /// The request without the turn's routing state.
    WithoutRoutingState,
    /// A rejected WebSocket handshake. It carried no token, so there is none to drop.
    Handshake,
    /// The retry budget is spent: the turn ends with this rejection.
    None,
}

/// Where an opaque rejection arrived and what the request had already been through.
#[derive(Clone, Copy, Debug)]
struct OpaqueRejection {
    websocket: bool,
    /// Opaque rejections of this request over HTTPS, this one included.
    https_rejections: u32,
    retries: u64,
    max_retries: u64,
}

/// Hook at the top of core's retry handler. The request that starts next is this one's
/// retry, which the HTTPS fallback's return leaves on its transport.
///
/// Returns true when this request was the one that returned to the WebSocket and the
/// socket did not carry it. The session is then back on HTTPS and the caller replays the
/// request there with its retry budget unspent, as upstream's own fallback does: the
/// socket's failure, even a terminal one, is not this request's.
///
/// An opaque rejection names no fault in the request. Over HTTPS the request carries the
/// routing token the turn's first response issued, and its first replay is unchanged, as
/// upstream's turn-state contract asks of a retry. Rejected again, the replay drops the
/// token, as upstream does when auth ownership changes. The transport is kept: a
/// rejection recorded with its transport was an HTTPS reply in a session an earlier
/// outage had moved off WebSocket, so moving a session to HTTPS does not answer it.
pub(super) async fn request_failed(
    err: &CodexErr,
    retry_state: &mut ResponsesStreamRetryState,
    max_retries: u64,
    client_session: &mut ModelClientSession,
    turn_context: &TurnContext,
) -> bool {
    client_session.alab_request_failed(unwell(err));
    note_opaque_rejection(err, retry_state, max_retries, client_session);
    if !give_up_returned_socket(err, client_session, turn_context) {
        return false;
    }
    retry_state.retries = 0;
    if let Some(retry_after) = err.retry_after() {
        // Changing transport does not bypass the server's retry deadline.
        wait_after(retry_after, turn_context).await;
    }
    true
}

/// A failure in which the transport did not carry the request to an answer. A refused
/// upgrade is an HTTP status with the URL it was asked of; a status inside an error
/// frame has none and is the server's answer. An unclassified stream failure counts
/// whichever side produced it.
fn transport_failed(err: &CodexErr) -> bool {
    match err.details() {
        CodexErrorDetails::Stream(_)
        | CodexErrorDetails::ConnectionFailed(_)
        | CodexErrorDetails::RequestTimeout => true,
        CodexErrorDetails::UnexpectedStatus(response) => response.url.is_some(),
        _ => false,
    }
}

/// A failure that says the network or the backend is unwell, which postpones a return to
/// the WebSocket. A refusal, a filter or a capacity limit is the server working, and an
/// opaque rejection over HTTPS is no reason to stay there.
fn unwell(err: &CodexErr) -> bool {
    match err.details() {
        CodexErrorDetails::Stream(_) => !opaque(err),
        CodexErrorDetails::ConnectionFailed(_)
        | CodexErrorDetails::RequestTimeout
        | CodexErrorDetails::InternalServerError => true,
        CodexErrorDetails::UnexpectedStatus(response) => response.status.is_server_error(),
        _ => false,
    }
}

/// The request that returned a session to the WebSocket is a trial. If the socket did not
/// carry it, the session goes back to HTTPS now, without the routing token the socket may
/// have issued, so no later request meets the same socket.
fn give_up_returned_socket(
    err: &CodexErr,
    client_session: &mut ModelClientSession,
    turn_context: &TurnContext,
) -> bool {
    let gave_up = client_session.alab_returned_socket_on_trial()
        && transport_failed(err)
        && client_session.try_switch_fallback_transport(
            &turn_context.session_telemetry,
            turn_context.model_info(),
        );
    if gave_up {
        client_session.alab_discard_routing_state();
    }
    gave_up
}

fn note_opaque_rejection(
    err: &CodexErr,
    retry_state: &mut ResponsesStreamRetryState,
    max_retries: u64,
    client_session: &mut ModelClientSession,
) {
    if !(codex_alab_core::active() && opaque(err)) {
        return;
    }
    let websocket = client_session.alab_uses_websocket();
    let rejection = OpaqueRejection {
        websocket,
        https_rejections: if websocket {
            0
        } else {
            retry_state.capacity.note_opaque_https_rejection()
        },
        retries: retry_state.retries,
        max_retries,
    };
    let replay = opaque_replay(rejection);
    let held = client_session.alab_replays_turn_state();
    let turn_state = match replay {
        OpaqueReplay::Handshake => "not_sent",
        _ if !held => "absent",
        OpaqueReplay::WithoutRoutingState => {
            client_session.alab_discard_routing_state();
            "dropped"
        }
        OpaqueReplay::Unchanged | OpaqueReplay::None => "kept",
    };
    // Which replay clears a rejection is the evidence its cause lacks. No field is
    // request content.
    tracing::info!(
        transport = if websocket { "websocket" } else { "https" },
        replay = match replay {
            OpaqueReplay::Unchanged => "unchanged",
            OpaqueReplay::WithoutRoutingState => "without_routing_state",
            OpaqueReplay::Handshake => "handshake",
            OpaqueReplay::None => "none",
        },
        turn_state,
        https_rejections = rejection.https_rejections,
        retries = rejection.retries,
        "Opaque backend rejection"
    );
}

fn opaque(err: &CodexErr) -> bool {
    matches!(err.details(), CodexErrorDetails::Stream(message)
        if codex_alab_core::request_error::opaque_bad_request_stream(message))
}

fn opaque_replay(rejection: OpaqueRejection) -> OpaqueReplay {
    if rejection.websocket {
        // Core retries the handshake, or moves the turn to HTTPS once the budget is spent.
        OpaqueReplay::Handshake
    } else if rejection.retries >= rejection.max_retries {
        OpaqueReplay::None
    } else if rejection.https_rejections <= 1 {
        OpaqueReplay::Unchanged
    } else {
        OpaqueReplay::WithoutRoutingState
    }
}

#[cfg(test)]
#[test]
fn only_a_repeated_https_rejection_with_budget_left_drops_routing_state() {
    let https = |https_rejections, retries, max_retries| {
        opaque_replay(OpaqueRejection {
            websocket: false,
            https_rejections,
            retries,
            max_retries,
        })
    };
    assert_eq!(https(1, 0, 5), OpaqueReplay::Unchanged);
    // Another kind of failure was retried first: this is still the first opaque one.
    assert_eq!(https(1, 3, 5), OpaqueReplay::Unchanged);
    assert_eq!(https(2, 1, 5), OpaqueReplay::WithoutRoutingState);
    assert_eq!(https(5, 4, 5), OpaqueReplay::WithoutRoutingState);
    // Nothing is replayed once the budget is spent, so nothing is dropped.
    assert_eq!(https(6, 5, 5), OpaqueReplay::None);
    assert_eq!(https(2, 1, 1), OpaqueReplay::None);
    assert_eq!(https(1, 0, 0), OpaqueReplay::None);
    for (retries, max_retries) in [(0, 5), (5, 5), (0, 0)] {
        assert_eq!(
            opaque_replay(OpaqueRejection {
                websocket: true,
                https_rejections: 0,
                retries,
                max_retries,
            }),
            OpaqueReplay::Handshake
        );
    }
}

#[cfg(test)]
#[test]
fn a_server_answer_is_not_a_failed_transport() {
    let status = |code: u16, url: Option<&str>| {
        CodexErr::UnexpectedStatus(codex_protocol::error::UnexpectedResponseError {
            status: http::StatusCode::from_u16(code).unwrap_or(http::StatusCode::IM_A_TEAPOT),
            body: String::new(),
            user_message: None,
            url: url.map(str::to_owned),
            cf_ray: None,
            request_id: None,
            identity_authorization_error: None,
            identity_error_code: None,
        })
    };
    let opaque_rejection = CodexErr::Stream(format!(
        "{}{}",
        codex_alab_core::request_error::OPAQUE_BAD_REQUEST_PREFIX,
        r#"{"detail":"Bad Request"}"#,
    ));
    // (error, the transport failed, the network or backend is unwell)
    let cases = [
        (
            CodexErr::Stream("closed before response.completed".into()),
            true,
            true,
        ),
        (CodexErr::RequestTimeout, true, true),
        // A middlebox refuses the upgrade: a status with the URL it was asked of.
        (
            status(403, Some("wss://example.test/v1/responses")),
            true,
            false,
        ),
        (
            status(503, Some("https://example.test/v1/responses")),
            true,
            true,
        ),
        // The same statuses inside an error frame are the server's answer.
        (status(403, None), false, false),
        (status(503, None), false, true),
        // A rejected handshake is a failed return; over HTTPS it does not hold one back.
        (opaque_rejection, true, false),
        (CodexErr::InternalServerError, false, true),
        (
            CodexErr::new(CodexErrorDetails::ContentFilter),
            false,
            false,
        ),
        (CodexErr::ContextWindowExceeded, false, false),
        (
            CodexErr::new(CodexErrorDetails::RateLimitExceeded("slow down".into())),
            false,
            false,
        ),
        (
            CodexErr::InvalidRequest("unsupported parameter".into()),
            false,
            false,
        ),
        (CodexErr::ServerOverloaded, false, false),
    ];
    for (error, failed, is_unwell) in cases {
        assert_eq!(transport_failed(&error), failed, "{error:?}");
        assert_eq!(unwell(&error), is_unwell, "{error:?}");
    }
}

#[cfg(test)]
#[test]
fn only_the_normalized_opaque_rejection_is_opaque() {
    assert!(opaque(&CodexErr::Stream(format!(
        "{}{}",
        codex_alab_core::request_error::OPAQUE_BAD_REQUEST_PREFIX,
        r#"{"detail":"Bad Request"}"#,
    ))));
    for error in [
        CodexErr::InvalidRequest(r#"{"detail":"Bad Request"}"#.into()),
        CodexErr::Stream("connection interrupted".into()),
        CodexErr::Stream(format!(
            "{}{}",
            codex_alab_core::request_error::OPAQUE_BAD_REQUEST_PREFIX,
            r#"{"detail":"Bad Request","code":"invalid_prompt"}"#,
        )),
    ] {
        assert!(!opaque(&error));
    }
}

/// Keep automatic transport recovery in the live retry-status channel.
pub(super) fn fallback_notice(event: EventMsg, err: &CodexErr) -> EventMsg {
    fallback_notice_when_active(codex_alab_core::active(), event, err)
}

fn fallback_notice_when_active(active: bool, event: EventMsg, err: &CodexErr) -> EventMsg {
    if !active {
        return event;
    }
    match event {
        EventMsg::Warning(warning) => {
            tracing::warn!("WebSocket request is continuing over HTTPS");
            EventMsg::StreamError(StreamErrorEvent {
                message: "Reconnecting over HTTPS".into(),
                codex_error_info: Some(CodexErrorInfo::ResponseStreamDisconnected {
                    http_status_code: err.http_status_code_value(),
                }),
                additional_details: Some(warning.message),
            })
        }
        event => event,
    }
}

/// Steering cannot erase a received refusal or an exhausted retry verdict. Capacity
/// keeps its selected wait too. An actual interrupt still cancels the caller's outer
/// future; ordinary retry waits retain steering's existing preemption.
pub(crate) fn preempt_for_retry(
    err: &CodexErr,
    preempt: &CancellationToken,
    state: &ResponsesStreamRetryState,
    max_retries: u64,
    turn: &TurnContext,
) -> CancellationToken {
    preempt_when_active(
        codex_alab_core::active(),
        err,
        preempt,
        state,
        max_retries,
        turn,
    )
}

fn preempt_when_active(
    active: bool,
    err: &CodexErr,
    preempt: &CancellationToken,
    state: &ResponsesStreamRetryState,
    max_retries: u64,
    turn: &TurnContext,
) -> CancellationToken {
    let unbounded_connection = unbounded_connection_retry(err, turn);
    if protects_retry(
        active,
        err,
        state.retries,
        max_retries,
        unbounded_connection,
    ) {
        CancellationToken::new()
    } else {
        preempt.clone()
    }
}

fn unbounded_connection_retry(err: &CodexErr, turn: &TurnContext) -> bool {
    turn.config
        .features
        .enabled(Feature::UnboundedConnectionRetries)
        && matches!(err.details(), CodexErrorDetails::ConnectionFailed(_))
        && !turn.session_source.is_internal()
        && !turn.provider.info().is_amazon_bedrock()
}

fn protects_retry(
    active: bool,
    err: &CodexErr,
    retries: u64,
    max_retries: u64,
    unbounded_connection: bool,
) -> bool {
    active
        && (err
            .http_status_code_value()
            .is_some_and(permanent_client_status)
            || capacity_reason(err).is_some()
            || err.retry_delay(1).is_none()
            || (retries >= max_retries && !unbounded_connection))
}

/// A handler's completed error owns the outcome even if steering arrived meanwhile.
/// Cancellation of an ordinary retry wait still hands its queued input back normally.
pub(crate) fn follow_up_after_retry(
    retry: &Result<Result<(), CodexErr>, CancelErr>,
    preempt: &CancellationToken,
    draining_for_mailbox: bool,
) -> bool {
    follow_up_when_active(
        codex_alab_core::active(),
        retry,
        preempt.is_cancelled(),
        draining_for_mailbox,
    )
}

fn follow_up_when_active(
    active: bool,
    retry: &Result<Result<(), CodexErr>, CancelErr>,
    preempted: bool,
    draining_for_mailbox: bool,
) -> bool {
    if active && matches!(retry, Ok(Err(_))) {
        return false;
    }
    preempted || (draining_for_mailbox && matches!(retry, Ok(Ok(()))))
}

pub(crate) fn should_retry(err: &CodexErr, turn_context: &TurnContext) -> bool {
    if !codex_alab_core::active() {
        return false;
    }
    remember(err, turn_context);
    should_retry_when_active(true, err) || quota_retry::eligible(err, turn_context)
}

/// Interactive quota walls retain their recovery policy; eligible exec walls
/// must reach this module instead of returning before the retry handler runs.
pub(crate) fn quota_is_terminal(err: &CodexErr, turn: &TurnContext) -> bool {
    !quota_retry::eligible(err, turn)
}

fn capacity_reason(err: &CodexErr) -> Option<CapacityReason> {
    match err.details() {
        CodexErrorDetails::ServerOverloaded => Some(CapacityReason::ServerOverloaded),
        CodexErrorDetails::PolicyCapacityUnavailable => {
            Some(CapacityReason::PolicyCapacityUnavailable)
        }
        _ => None,
    }
}

fn should_retry_when_active(active: bool, err: &CodexErr) -> bool {
    active
        && !err
            .http_status_code_value()
            .is_some_and(permanent_client_status)
        && capacity_reason(err).is_some()
}

/// Native sampling returns these failures before the common response handler.
/// Other failures are observed once by that handler, including remote compaction.
pub(crate) fn remember_terminal(err: &CodexErr, turn: &TurnContext) {
    if matches!(
        err.details(),
        CodexErrorDetails::ContextWindowExceeded
            | CodexErrorDetails::UsageLimitReached(_)
            | CodexErrorDetails::SessionBudgetExceeded
            | CodexErrorDetails::InferenceHookDenied { .. }
            | CodexErrorDetails::Fatal(_)
    ) {
        remember(err, turn);
    }
}

/// Every Responses request lane shares its turn's advice, including compaction before
/// sampling starts. A later failure without advice replaces an earlier deadline.
pub(crate) fn remember(err: &CodexErr, turn_context: &TurnContext) {
    if !codex_alab_core::active() {
        return;
    }
    // Overwrite absent advice too: a later failure in this turn must not inherit an
    // earlier response's deadline. The turn store cannot leak into another turn.
    let reading = codex_alab_core::wake::read();
    let advice = SamplingRetryAdvice::from_deadline_with_reading(
        err.retry_after()
            .map(codex_http_client::RetryAfter::sleep_deadline),
        reading,
    );
    let retry_at = advice.deadline_at(reading, Instant::now().into_std());
    turn_context.extension_data.insert(advice);
    if let Some(observer) = turn_context.extension_data.get::<InFlightRetryObserver>() {
        observer.observe(retry_at);
    }
}

/// A transport wrapper must not make a permanent client refusal retryable in core.
pub(super) fn should_stop(err: &CodexErr) -> bool {
    codex_alab_core::active()
        && err
            .http_status_code_value()
            .is_some_and(permanent_client_status)
}

/// Legacy compaction predates the shared retry handler. Keep its context-shrinking
/// recovery, but do not replay refusals or errors core already calls terminal.
///
/// The two rules the shared handler applies to an HTTPS fallback's return are applied
/// here: what follows is this request's retry, and a returned socket that did not carry
/// its first request is given up.
pub(crate) fn compact_should_stop(
    err: &CodexErr,
    turn_context: &TurnContext,
    client_session: &mut ModelClientSession,
) -> bool {
    if !codex_alab_core::active() {
        return false;
    }
    remember(err, turn_context);
    client_session.alab_request_failed(unwell(err));
    if give_up_returned_socket(err, client_session, turn_context) {
        // The socket's failure is not this request's: the loop's retry arm replays it.
        return false;
    }
    should_stop(err)
        || (!matches!(err.details(), CodexErrorDetails::ContextWindowExceeded)
            && err.retry_delay(1).is_none())
}

/// Select before reporting the wait: reporting must not restart provider advice or
/// erase the local positive backoff. An inactive consumer keeps upstream's sleep.
pub(crate) fn compact_retry_at(
    err: &CodexErr,
    delay: std::time::Duration,
    turn: &TurnContext,
) -> Option<OrdinaryRetryWait> {
    codex_alab_core::active().then(|| retry_wait(err.retry_after(), delay, true, turn))
}

/// A deadline is ALab's (active): the host's sleep counts toward it
/// ([`codex_alab_core::wake::sleep_until`]).
pub(crate) async fn compact_wait(retry_at: Instant, selected: Option<OrdinaryRetryWait>) {
    match selected {
        Some(wait) => {
            ordinary_wait_until(retry_at, wait).await;
        }
        None => tokio::time::sleep_until(retry_at).await,
    }
}

/// Active retry waits count host sleep; an inactive consumer keeps upstream's timer.
pub(super) async fn wait_until(retry_at: Instant) {
    if codex_alab_core::active() {
        codex_alab_core::wake::sleep_until(retry_at).await;
    } else {
        tokio::time::sleep_until(retry_at).await;
    }
}

/// A fallback must retain provider advice across a rejected selection sample too.
pub(super) async fn wait_after(advice: codex_http_client::RetryAfter, turn: &TurnContext) {
    if codex_alab_core::active() {
        let selected = ordinary_retry_wait(Some(advice), std::time::Duration::ZERO, turn);
        ordinary_wait_until(Instant::now(), selected).await;
    } else {
        tokio::time::sleep_until(advice.deadline()).await;
    }
}

/// Provider advice does not need an unused local sum that may overflow after reporting.
pub(super) fn reported_deadline(
    advice: Option<codex_http_client::RetryAfter>,
    now: Instant,
    delay: std::time::Duration,
) -> Instant {
    if codex_alab_core::active() {
        advice.map_or_else(|| now + delay, codex_http_client::RetryAfter::deadline)
    } else {
        advice
            .map(codex_http_client::RetryAfter::deadline)
            .unwrap_or(now + delay)
    }
}

/// A request-local wait that can survive an asynchronous retry notification.
pub(crate) struct OrdinaryRetryWait {
    wait: Option<codex_alab_core::wake::ArmedWait>,
    persisted: Option<codex_alab_core::capacity::InFlightRetryGuard>,
    #[cfg(test)]
    read: Option<ArcWakeRead>,
    #[cfg(test)]
    entered: Option<std::sync::Arc<std::sync::atomic::AtomicUsize>>,
}

#[cfg(test)]
type ArcWakeRead = std::sync::Arc<dyn Fn() -> Option<codex_alab_core::wake::Reading> + Send + Sync>;

#[cfg(test)]
struct OrdinaryRetryClock {
    read: ArcWakeRead,
    selected: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    entered: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

/// Select alongside the request, before notification can await event delivery.
pub(super) fn ordinary_retry_wait(
    advice: Option<codex_http_client::RetryAfter>,
    delay: std::time::Duration,
    turn: &TurnContext,
) -> OrdinaryRetryWait {
    retry_wait(advice, delay, false, turn)
}

fn retry_wait(
    advice: Option<codex_http_client::RetryAfter>,
    delay: std::time::Duration,
    local_minimum: bool,
    turn: &TurnContext,
) -> OrdinaryRetryWait {
    let active = codex_alab_core::active();
    #[cfg(test)]
    let (read, entered) =
        turn.extension_data
            .get::<OrdinaryRetryClock>()
            .map_or((None, None), |clock| {
                clock
                    .selected
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if active {
                    (
                        Some(std::sync::Arc::clone(&clock.read)),
                        Some(std::sync::Arc::clone(&clock.entered)),
                    )
                } else {
                    (None, None)
                }
            });
    let wait = active.then(|| {
        let minimum = local_minimum.then(|| Instant::now() + delay);
        #[cfg(test)]
        if let Some(read) = &read {
            return advice.map_or_else(
                || {
                    codex_alab_core::wake::ArmedWait::capture_with_clock(
                        minimum.unwrap_or_else(|| Instant::now() + delay),
                        true,
                        || read(),
                    )
                },
                |advice| advice.arm_sleep_wait_with_clock(minimum, || read()),
            );
        }
        advice.map_or_else(
            || {
                codex_alab_core::wake::ArmedWait::new(
                    minimum.unwrap_or_else(|| Instant::now() + delay),
                )
            },
            |advice| advice.arm_sleep_wait(minimum),
        )
    });
    let persisted = wait.and_then(|wait| {
        turn.extension_data
            .get::<InFlightRetryObserver>()
            .map(|observer| observer.waiting(wait.deadline().into_std()))
    });
    OrdinaryRetryWait {
        wait,
        persisted,
        #[cfg(test)]
        read,
        #[cfg(test)]
        entered,
    }
}

/// An inactive consumer retains upstream's deadline calculated after reporting.
pub(super) async fn ordinary_wait_until(retry_at: Instant, selected: OrdinaryRetryWait) -> bool {
    let OrdinaryRetryWait {
        wait,
        persisted,
        #[cfg(test)]
        read,
        #[cfg(test)]
        entered,
    } = selected;
    #[cfg(test)]
    if let Some(entered) = entered {
        entered.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
    let changed = |deadline: Instant| {
        if let Some(guard) = &persisted {
            let _ = guard.update(deadline.into_std());
        }
    };
    #[cfg(test)]
    if let Some(read) = read {
        let wait = wait.unwrap_or_else(|| {
            codex_alab_core::wake::ArmedWait::capture_with_clock(retry_at, true, || read())
        });
        return wait
            .wait_with_clock_observed(codex_alab_core::wake::POLL, || read(), changed)
            .await;
    }
    match wait {
        Some(wait) => wait.wait_observed(changed).await,
        None => {
            wait_until(retry_at).await;
            false
        }
    }
}

/// CORE'S WAIT FOR THE NETWORK (`unbounded_connection_retries`: 5 s doubling to 60 s inside the
/// turn) as a wait of real time: the host's sleep counts toward it, so a wait a sleep outlasted
/// ends [`codex_alab_core::wake::AFTER_WAKE`] after the wake instead of once the awake clock has
/// made up its remainder, and the doubling starts over (the delay returned doubles to core's
/// first): the network the wake brings is a new one, and a request the wake's own network was
/// not yet up for retries within seconds, never after the minute the dark wakes climbed to.
/// Inactive, upstream's sleep and delay.
pub(super) async fn connection_wait(
    delay: std::time::Duration,
    selected: OrdinaryRetryWait,
) -> std::time::Duration {
    if !codex_alab_core::active() {
        tokio::time::sleep(delay).await;
        return delay;
    }
    if ordinary_wait_until(Instant::now() + delay, selected).await {
        super::INITIAL_CONNECTION_RETRY_DELAY / 2
    } else {
        delay
    }
}

/// Legacy compaction has its own request loop and capacity budget. It uses sampling
/// requests, but retains its compaction input and does not shrink history on overload.
pub(crate) async fn compact_capacity_wait(
    backoff: &mut CapacityBackoff,
    err: CodexErr,
    sess: &Session,
    turn_context: &TurnContext,
) {
    wait(
        backoff,
        err,
        sess,
        turn_context,
        ResponsesStreamRequest::Sampling,
    )
    .await;
}

/// Callers retain their existing task cancellation guards. Returning to the request
/// loop retains its original input, model and task; no new turn is sent.
pub(super) async fn wait(
    backoff: &mut CapacityBackoff,
    err: CodexErr,
    sess: &Session,
    turn_context: &TurnContext,
    request: ResponsesStreamRequest,
) {
    if quota_retry::eligible(&err, turn_context) {
        quota_retry::wait(backoff, err, sess, turn_context).await;
        return;
    }
    let reason = capacity_reason(&err).unwrap_or(CapacityReason::ServerOverloaded);
    let now = Instant::now();
    let local_deadline = now + backoff.next_delay(rand::random());
    // Retry-After is an absolute deadline. Reporting the wait must not restart it, and
    // absent, expired or zero advice must never bypass our positive local backoff.
    let armed_wait = err.retry_after().map_or_else(
        || codex_alab_core::wake::ArmedWait::new(local_deadline),
        |advice| advice.arm_sleep_wait(Some(local_deadline)),
    );
    let retry_at = armed_wait.deadline();
    let delay = retry_at.saturating_duration_since(now);
    let persisted_wait = turn_context
        .extension_data
        .get::<InFlightRetryObserver>()
        .map(|observer| observer.waiting(retry_at.into_std()));
    let live_capacity_wait = turn_context
        .extension_data
        .get_or_init(LiveCapacityWait::default);
    let live_capacity_wait = live_capacity_wait.begin_with_reason(retry_at.into_std(), reason);
    tracing::warn!(
        turn_id = %turn_context.sub_id,
        attempts = backoff.attempts(),
        ?reason,
        ?delay,
        "selected model is at capacity; waiting to retry the active task"
    );
    sess.send_event(
        turn_context,
        EventMsg::StreamError(StreamErrorEvent {
            message: reason.notice().into(),
            codex_error_info: Some(err.to_codex_protocol_error()),
            additional_details: Some(err.to_string()),
        }),
    )
    .await;
    let operation = match request {
        ResponsesStreamRequest::Sampling => RetryOperation::Sampling,
        ResponsesStreamRequest::RemoteCompactionV2 => RetryOperation::RemoteCompactionV2,
    };
    codex_client::record_retry!(backoff.attempts(), delay, operation);
    // The host's sleep counts toward the wait (Retry-After is real time): a wait a sleep
    // outlasted ends soon after the wake, never once the awake clock has made it up.
    armed_wait
        .wait_observed(|deadline| {
            let deadline = deadline.into_std();
            if let Some(guard) = &persisted_wait {
                let _ = guard.update(deadline);
            }
            let _ = live_capacity_wait.update(deadline);
        })
        .await;
}

#[cfg(test)]
#[path = "test_child_process.rs"]
mod test_child_process;

#[path = "responses_quota_retry.rs"]
mod quota_retry;

#[cfg(test)]
#[path = "responses_retry_preempt_tests.rs"]
mod preempt_tests;

#[cfg(test)]
#[path = "responses_retry_wake_tests.rs"]
mod wake_tests;

#[cfg(test)]
#[path = "responses_transport_status_tests.rs"]
mod transport_status_tests;

// Keep the actual model client's Codex-backend provider binding (client.rs).
fn weekly_provider(turn: &TurnContext) -> bool {
    let provider = turn.provider.info();
    provider.is_openai()
        && provider.requires_openai_auth
        && provider.env_key.is_none()
        && provider.experimental_bearer_token.is_none()
        && provider.auth.is_none()
        && provider.aws.is_none()
}

/// Compaction does not pass its task token through its request API. Capture the
/// matching foreground task before awaiting, so interrupt cancels redemption
/// during the task runner's graceful shutdown window too.
async fn compaction_task_token(sess: &Session, turn: &TurnContext) -> Option<CancellationToken> {
    let active = sess.active_turn.lock().await;
    active
        .as_ref()?
        .task
        .as_ref()
        .filter(|task| task.turn_context.sub_id == turn.sub_id)
        .map(|task| task.cancellation_token.clone())
}

pub(crate) async fn bind_compaction_weekly(
    sess: &Session,
    turn: &TurnContext,
) -> Result<(), CodexErr> {
    use codex_async_utils::OrCancelExt;
    if let Some(cancellation) = compaction_task_token(sess, turn).await {
        bind_weekly(sess, turn).or_cancel(&cancellation).await?;
    }
    Ok(())
}

pub(crate) async fn preflight_compaction_weekly(
    sess: &Session,
    turn: &TurnContext,
) -> Result<(), CodexErr> {
    use codex_async_utils::OrCancelExt;
    if let Some(cancellation) = compaction_task_token(sess, turn).await {
        preflight_weekly(sess, turn)
            .or_cancel(&cancellation)
            .await?;
    }
    Ok(())
}

pub(crate) async fn recover_compaction_weekly(
    err: &CodexErr,
    sess: &Session,
    turn: &TurnContext,
) -> Result<bool, CodexErr> {
    use codex_async_utils::OrCancelExt;
    let Some(cancellation) = compaction_task_token(sess, turn).await else {
        return Ok(false);
    };
    Ok(recover_weekly(err, sess, turn)
        .or_cancel(&cancellation)
        .await?)
}

/// Whether this session's thread is one of the TUI's hidden helpers: a rename
/// suggestion or automatic title (`thread_title`) or a recap (`system`), each
/// a single structured turn on an ephemeral thread the person never sees.
///
/// This mirrors upstream's own test, which is written inline where the TUI
/// keeps such a thread out of its routing (the `ThreadStarted` test in
/// `handle_server_notification_event`, `tui/src/app/app_server_events.rs`)
/// and, for the title alone, as `title_request` in
/// `core/src/session/session.rs`. Every other `Feature` thread is a
/// conversation: an attached side chat is `tui_side_chat`, and automations
/// name their own.
async fn hidden_helper_thread(sess: &Session) -> bool {
    use codex_protocol::protocol::ThreadSource;
    let thread = sess.thread_config_snapshot().await;
    thread.ephemeral
        && matches!(
            thread.thread_source.as_ref(),
            Some(ThreadSource::Feature(feature)) if matches!(feature.as_str(), "system" | "thread_title")
        )
}

/// A helper has no visible conversation for its reset decisions. An exec run
/// owns its request even when its thread uses a helper's source name.
async fn reset_request_visible(sess: &Session, turn: &TurnContext) -> bool {
    turn.extension_data
        .get::<codex_alab_core::quota::HeadlessExec>()
        .is_some()
        || !hidden_helper_thread(sess).await
}

/// Redeem before deciding whether the original quota wall is terminal.
pub(crate) async fn recover_weekly(err: &CodexErr, sess: &Session, turn: &TurnContext) -> bool {
    use codex_protocol::protocol::SessionSource;
    // An exec host keeps its request across the wall: no handoff is asked for
    // its turn, and its usage error does not end it.
    let exec = turn
        .extension_data
        .get::<codex_alab_core::quota::HeadlessExec>()
        .is_some();
    if !codex_alab_core::active()
        || !weekly_provider(turn)
        || turn.session_source.is_non_root_agent()
        || !(matches!(turn.session_source, SessionSource::Cli) || exec)
    {
        return false;
    }
    let CodexErrorDetails::UsageLimitReached(details) = err.details() else {
        return false;
    };
    // A hidden helper's turn is nobody's conversation: no handoff is ever ready
    // for its thread, so recovery would poll until the wall's reset and the
    // session would say busy for as long. Its usage error ends it instead.
    // Only an interactive turn is such a helper: an exec turn of the same
    // thread identity (`codex exec --ephemeral --thread-source system`) is the
    // run's own request, and returning here would leave it waiting out the wall
    // with no further look for an earned reset.
    if !reset_request_visible(sess, turn).await {
        return false;
    }
    if !codex_alab_quota_recovery::ordinary_weekly_wall(details) {
        reset_notice(
            sess,
            turn,
            "Reset not used: this limit is outside the ordinary Codex allowance, so an earned reset cannot clear it.".into(),
        )
        .await;
        return false;
    }
    let Some(state) = turn
        .extension_data
        .get::<codex_alab_quota_recovery::State>()
    else {
        reset_notice(
            sess,
            turn,
            "Reset not used: the current request has no authenticated reset-recovery state; availability and expiry could not be verified.".into(),
        )
        .await;
        return false;
    };
    // Recovery can keep polling until an earned reset arrives. Keep the exact
    // authenticated request observable during that wait, before the later quota
    // retry path is reached; cancellation or restoration drops this lease.
    let _quota_observation = crate::session::alab_exec_recovery_options::quota_observation::publish(
        sess,
        turn,
        codex_alab_quota_recovery::WallKind::Ordinary,
    );
    // An earned reset comes first, then another account. A host that can move
    // this conversation once the turn ends installs the handoff in the turn
    // store; an exec host keeps its request across the wall instead.
    let handoff = (!exec)
        .then(|| {
            turn.extension_data
                .get::<codex_alab_core::quota::WallHandoff>()
        })
        .flatten();
    with_reset_notices(
        sess,
        turn,
        &state,
        codex_alab_quota_recovery::recover_weekly(
            &sess.services.auth_manager,
            &turn.config.codex_home,
            &turn.config.chatgpt_base_url,
            turn.config.http_client_factory(),
            &turn.sub_id,
            &state,
            codex_alab_quota_recovery::Wall {
                reset_at: details.resets_at.as_ref().map(chrono::DateTime::timestamp),
                handoff: handoff.as_deref(),
            },
        ),
    )
    .await
}

async fn with_reset_notices<T>(
    sess: &Session,
    turn: &TurnContext,
    state: &codex_alab_quota_recovery::State,
    operation: impl std::future::Future<Output = T>,
) -> T {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    state.set_notice_sender(sender);
    reset_notices::forward(receiver, operation, |message| {
        reset_notice(sess, turn, message)
    })
    .await
}

async fn reset_notice(sess: &Session, turn: &TurnContext, message: String) {
    sess.send_event(
        turn,
        EventMsg::Warning(codex_protocol::protocol::WarningEvent {
            message: format!("[codex-alab reset] {message}"),
        }),
    )
    .await;
}

pub(crate) async fn bind_weekly(sess: &Session, turn: &TurnContext) {
    use codex_alab_core::renewal::AuthRequestScope;
    use codex_alab_core::renewal::RequestAuth;
    // Revoke inherited permission before any refresh can await or fail.
    turn.extension_data.insert_if(
        AuthRequestScope::new(turn.sub_id.clone(), None, 0, 0, || false),
        |_| true,
    );
    if codex_alab_core::active() && weekly_provider(turn) {
        let manager = std::sync::Arc::clone(&sess.services.auth_manager);
        if let Some(auth) = manager.auth().await
            && let Some(revision) = manager.auth_change_state_for(&auth)
        {
            let kind = if auth.auth_mode() == codex_protocol::auth::AuthMode::Chatgpt {
                auth.get_account_id()
                    .zip(auth.get_chatgpt_user_id())
                    .map(|(account, user)| RequestAuth::ChatGpt { account, user })
            } else if auth.is_api_key_auth() {
                Some(RequestAuth::OpenAiApiKey)
            } else {
                None
            };
            let api_key = matches!(kind, Some(RequestAuth::OpenAiApiKey));
            turn.extension_data.insert_if(
                AuthRequestScope::new(
                    turn.sub_id.clone(),
                    kind,
                    revision.owner_generation,
                    revision.generation,
                    move || {
                        manager
                            .auth_cached()
                            .and_then(|auth| manager.auth_change_state_for(&auth))
                            .is_some_and(|now| {
                                now.owner_generation == revision.owner_generation
                                    && (!api_key || now.generation == revision.generation)
                            })
                    },
                ),
                |_| true,
            );
        }
    }
    if codex_alab_core::active()
        && let Some(scope) = turn.extension_data.get::<AuthRequestScope>()
    {
        scope.observe_file(&codex_alab_core::home::auth_file(&turn.config.codex_home));
        scope.register(std::sync::Arc::clone(&sess.services.auth_manager));
    }
    // The quota retry sees no session: an exec member's turn carries its tree's walls.
    crate::session::alab_exec_recovery_options::quota_observation::bind(sess, turn);
    if codex_alab_core::active()
        && weekly_provider(turn)
        && !turn.session_source.is_non_root_agent()
        && turn
            .extension_data
            .get::<codex_alab_quota_recovery::State>()
            .is_none()
    {
        let state = codex_alab_quota_recovery::State::bind(&sess.services.auth_manager).await;
        turn.extension_data.insert_if(state, |old| old.is_none());
    }
}

/// Check quota at the send boundary, including attempts after retry waits.
/// Authentication and custody were bound before request preparation.
pub(crate) async fn preflight_weekly(sess: &Session, turn: &TurnContext) {
    if codex_alab_core::active()
        && weekly_provider(turn)
        && !turn.session_source.is_non_root_agent()
        && (matches!(
            turn.session_source,
            codex_protocol::protocol::SessionSource::Cli
        ) || turn
            .extension_data
            .get::<codex_alab_core::quota::HeadlessExec>()
            .is_some())
        && reset_request_visible(sess, turn).await
        && let Some(state) = turn
            .extension_data
            .get::<codex_alab_quota_recovery::State>()
    {
        // What the last response reported can end the reuse of this turn's
        // clear usage reading early; it never excuses a read by itself.
        let observed =
            crate::session::alab_exec_recovery_options::quota_observation::response_limits(sess)
                .await;
        with_reset_notices(
            sess,
            turn,
            &state,
            codex_alab_quota_recovery::preflight(
                &sess.services.auth_manager,
                &turn.config.codex_home,
                &turn.config.chatgpt_base_url,
                turn.config.http_client_factory(),
                &turn.sub_id,
                &state,
                observed.as_ref(),
            ),
        )
        .await;
    }
}
