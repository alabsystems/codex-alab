// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The real ordinary-retry handler, held inside its event notification across sleep.

use super::*;
use crate::realtime_history::RealtimeHistoryState;
use crate::session::step_context::StepContext;
use codex_alab_core::wake::Reading;
use codex_alab_core::wake::WakeState;
use codex_async_utils::OrCancelExt;
use codex_http_client::RetryAfter;
use std::fs;
use std::process::Command;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

const SECOND: Duration = Duration::from_secs(1);
const WAIT: Duration = Duration::from_secs(10);
const SLEEP: Duration = Duration::from_secs(7200);
const BASE: Duration = Duration::from_secs(5000);
const CHILD: &str = "ALAB_ORDINARY_RETRY_WAKE_CHILD";
const COMPLETE: &str = "ALAB_ORDINARY_RETRY_WAKE_OK";

fn maximum_delay(now: Instant) -> Duration {
    let duration = |nanos: u128| {
        Duration::new(
            u64::try_from(nanos / 1_000_000_000).expect("duration seconds fit"),
            u32::try_from(nanos % 1_000_000_000).expect("duration nanoseconds fit"),
        )
    };
    let mut lower = 0;
    let mut upper = Duration::MAX.as_nanos();
    while lower < upper {
        let candidate = lower + (upper - lower).div_ceil(2);
        if now.checked_add(duration(candidate)).is_some() {
            lower = candidate;
        } else {
            upper = candidate - 1;
        }
    }
    duration(lower)
}

struct Host {
    total: Duration,
    state: WakeState,
    reads: usize,
}

fn host_clock(total: Duration) -> (Arc<Mutex<Host>>, super::OrdinaryRetryClock) {
    let host = Arc::new(Mutex::new(Host {
        total,
        state: WakeState::default(),
        reads: 0,
    }));
    let reader = Arc::clone(&host);
    let read = Arc::new(move || {
        let mut host = reader
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        host.reads += 1;
        let asleep = host.total;
        Some(Reading {
            asleep,
            last: host.state.observe(Instant::now().into_std(), asleep),
        })
    });
    let read = super::OrdinaryRetryClock {
        read,
        selected: Arc::new(AtomicUsize::new(0)),
        entered: Arc::new(AtomicUsize::new(0)),
    };
    (host, read)
}

#[expect(
    clippy::await_holding_invalid_type,
    reason = "the fixture holds the real event-delivery lock while its injected host clock sleeps"
)]
async fn ordinary_retry(case: &str) {
    let (mut session, mut turn) = crate::session::tests::make_session_and_context().await;
    session.realtime_history = Some(tokio::sync::Mutex::new(RealtimeHistoryState::default()));
    let connection = matches!(case, "connection-sleep" | "connection-inactive");
    let inactive = matches!(case, "inactive" | "connection-inactive");
    if connection {
        Arc::make_mut(&mut turn.config)
            .features
            .enable(Feature::UnboundedConnectionRetries)
            .expect("unbounded connection retry fixture");
        turn.session_source = codex_protocol::protocol::SessionSource::Exec;
    }
    let step = StepContext::for_test(Arc::new(turn));
    let old_sleep = case == "old-sleep";
    let (host, read) = host_clock(if old_sleep { BASE + SLEEP } else { BASE });
    let read_before = (read.read)().expect("a valid fresh baseline exists before notification");
    assert_eq!(
        read_before.asleep,
        if old_sleep { BASE + SLEEP } else { BASE }
    );
    let selected = Arc::clone(&read.selected);
    let entered = Arc::clone(&read.entered);
    step.turn.extension_data.insert(read);
    let pacing = Arc::new(Mutex::new(Vec::new()));
    let published = Arc::clone(&pacing);
    let (cut, cut_read) = tokio::sync::oneshot::channel();
    let cut = Mutex::new(Some(cut));
    step.turn
        .extension_data
        .insert(InFlightRetryObserver::new(move |deadline| {
            let mut published = published
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let before = published.last().copied().flatten();
            published.push(deadline);
            if let (Some(before), Some(after)) = (before, deadline)
                && after < before
                && let Some(cut) = cut
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take()
            {
                let _ = cut.send(after);
            }
        }));
    let mut client = session.services.model_client.new_session();
    let mut state = ResponsesStreamRetryState {
        retries: 1,
        connection_retry_delay: if case == "connection-sleep" {
            WAIT * 6
        } else {
            super::super::INITIAL_CONNECTION_RETRY_DELAY
        },
        ..Default::default()
    };
    tokio::time::pause();
    let start = Instant::now();
    // Exactly the receipt representation after a rejected sample: active, but no
    // accepted receipt baseline. Later callers must keep this advice Copy.
    let provider_wait = if case == "extreme" {
        maximum_delay(start) - Duration::from_millis(1)
    } else {
        WAIT
    };
    let advice = RetryAfter::from_delay_with_sleep_reading(provider_wait, None)
        .expect("representable provider deadline");
    let copied = advice;
    let original = advice.deadline();
    let error = if connection {
        CodexErr::ConnectionFailed(codex_protocol::error::ConnectionFailedError {
            source: codex_http_client::HttpError::Timeout,
        })
    } else if case == "no-advice" {
        CodexErr::InternalServerError
    } else {
        CodexErr::InternalServerError.with_retry_after(advice)
    };
    if !inactive {
        codex_alab_core::activate();
    }
    let cancel = CancellationToken::new();
    let delivery = session
        .realtime_history
        .as_ref()
        .expect("real event-delivery lock")
        .lock()
        .await;
    {
        let retry = super::super::handle_response_stream_error(
            &mut state,
            3,
            error,
            &mut client,
            &session,
            &step,
            ResponsesStreamRequest::Sampling,
        )
        .or_cancel(&cancel);
        tokio::pin!(retry);
        assert!(
            futures::poll!(&mut retry).is_pending(),
            "the actual retry handler must reach its held notification"
        );
        assert_eq!(
            selected.load(Ordering::SeqCst),
            1,
            "the real handler reached its synchronous selection hook before notification"
        );
        let selection_reads = host
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .reads;
        if case == "extreme" {
            tokio::time::advance(2 * SECOND).await;
            assert!(
                Instant::now().checked_add(provider_wait).is_none(),
                "the unused after-notification local sum would overflow"
            );
        }
        // The notification is pending while a real accepted reader can distinguish
        // sleep from awake time. No tokio time passes while the host sleeps.
        if matches!(
            case,
            "sleep" | "inactive" | "connection-sleep" | "connection-inactive"
        ) {
            host.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .total += SLEEP;
        }
        drop(delivery);
        assert!(futures::poll!(&mut retry).is_pending());
        if !inactive {
            let hang = std::time::Instant::now() + Duration::from_secs(60);
            while entered.load(Ordering::SeqCst) == 0 {
                assert!(
                    std::time::Instant::now() < hang,
                    "post-notification wait-entry hang detector"
                );
                tokio::task::yield_now().await;
                assert!(
                    futures::poll!(&mut retry).is_pending(),
                    "the selected wait cannot finish before entering its timer"
                );
            }
        }
        if matches!(case, "cancel" | "extreme") {
            cancel.cancel();
            assert!(matches!(retry.await, Err(CancelErr::Cancelled)));
            assert_eq!(
                Instant::now(),
                start
                    + if case == "extreme" {
                        2 * SECOND
                    } else {
                        Duration::ZERO
                    }
            );
        } else if matches!(case, "sleep" | "connection-sleep") {
            let narrowed = tokio::select! {
                result = &mut retry => panic!("the selected wait finished before publishing its measured cut: {result:?}"),
                narrowed = cut_read => narrowed.expect("the exact wait publishes its measured cut"),
            };
            assert!(
                futures::poll!(&mut retry).is_pending(),
                "the exact wait remains pending after publishing its measured cut"
            );
            assert!(
                host.lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .reads
                    > selection_reads,
                "a positive post-selection clock read precedes the cut assertion"
            );
            assert_eq!(
                pacing
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .last()
                    .copied(),
                Some(Some(narrowed)),
                "the real handler republishes its measured cut while the wait still holds"
            );
            let wake = host
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .state
                .last()
                .expect("the supplied clock observed the sleep");
            assert_eq!(
                narrowed,
                wake.at + codex_alab_core::wake::AFTER_WAKE,
                "the wait uses the accepted wake's network-settle boundary"
            );
            assert!(
                Instant::now().into_std() < narrowed,
                "the publication is observed before its wait can finish"
            );
            retry
                .await
                .expect("not cancelled")
                .expect("ordinary retry allowed");
            assert!(Instant::now().into_std() >= narrowed);
            assert_eq!(
                selection_reads, 2,
                "the successful wait captured the valid baseline before the host slept inside notification"
            );
        } else {
            assert!(
                futures::poll!(&mut retry).is_pending(),
                "a positive wait remains"
            );
            if case != "no-advice" {
                tokio::time::advance(3 * SECOND).await;
                assert!(
                    futures::poll!(&mut retry).is_pending(),
                    "no later sleep leaves the provider's full interval intact"
                );
            }
            retry
                .await
                .expect("not cancelled")
                .expect("ordinary retry allowed");
            if case == "no-advice" {
                assert!(
                    Instant::now() > start,
                    "headerless advice keeps positive local backoff"
                );
            } else if case == "connection-inactive" {
                assert!(
                    Instant::now() >= start + super::super::INITIAL_CONNECTION_RETRY_DELAY,
                    "inactive connection retry keeps its original local interval"
                );
            } else {
                assert!(
                    Instant::now() >= original,
                    "the original provider deadline stands"
                );
            }
        }
    }
    if case == "connection-sleep" {
        assert_eq!(
            state.connection_retry_delay,
            super::super::INITIAL_CONNECTION_RETRY_DELAY,
            "a cut-short60s connection wait resets doubling to5s"
        );
        assert_eq!(state.connection_retries, 1);
    }
    if case == "connection-inactive" {
        assert_eq!(
            state.connection_retry_delay,
            super::super::INITIAL_CONNECTION_RETRY_DELAY * 2,
            "inactive connection retry keeps ordinary doubling"
        );
    }
    assert_eq!(
        copied.deadline(),
        original,
        "the copied receipt is never mutated"
    );
    assert_eq!(advice, copied);
    if inactive {
        assert!(!codex_alab_core::active());
        assert!(
            pacing
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_empty(),
            "inactive retry leaves ALab pacing unpublished"
        );
        assert_eq!(
            host.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .reads,
            1,
            "the inactive consumer never samples the injected host clock"
        );
    } else {
        assert_eq!(
            pacing
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .last()
                .copied(),
            Some(None),
            "finishing or cancelling the exact wait clears its pacing publication"
        );
    }
}

fn child_name() -> String {
    let (_, module) = module_path!().split_once("::").expect("test crate prefix");
    format!("{module}::ordinary_retry_wake_child")
}

fn isolated(case: &str) -> std::io::Result<()> {
    assert!(!codex_alab_core::active());
    let private = tempfile::Builder::new()
        .prefix("alab-ordinary-retry-wake-")
        .tempdir()?;
    let home = private.path().join("home");
    let codex_home = private.path().join("codex");
    let tmp = private.path().join("tmp");
    for directory in [&home, &codex_home, &tmp] {
        fs::create_dir_all(directory)?;
    }
    let path = std::env::var_os("PATH")
        .ok_or_else(|| std::io::Error::other("contained PATH is required"))?;
    let name = child_name();
    let mut child = Command::new(std::env::current_exe()?);
    child
        .args(["--exact", &name, "--nocapture", "--test-threads=1"])
        .env_clear()
        .env(CHILD, case)
        .env("HOME", home)
        .env("CODEX_HOME", codex_home)
        .env("TMPDIR", tmp)
        .env("PATH", path)
        .env("RUST_MIN_STACK", "16777216")
        .stdin(Stdio::null());
    for key in [
        "DYLD_LIBRARY_PATH",
        "LD_LIBRARY_PATH",
        "CARGO_BIN_EXE_test_stdio_server",
        "CARGO_BIN_EXE_codex-code-mode-host",
        "CARGO_BIN_EXE_codex_code_mode_host",
    ] {
        if let Some(value) = std::env::var_os(key) {
            child.env(key, value);
        }
    }
    let output = child.output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{case}: {stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains(&format!("{COMPLETE}:{case}")), "{stdout}");
    assert!(stdout.contains("1 passed; 0 failed"), "{stdout}");
    assert!(!codex_alab_core::active());
    Ok(())
}

#[test]
fn ordinary_retry_recovers_sleep_during_notification_after_a_rejected_receipt_sample()
-> std::io::Result<()> {
    isolated("sleep")
}

#[test]
fn ordinary_retry_keeps_provider_deadline_without_sleep() -> std::io::Result<()> {
    isolated("no-sleep")
}

#[test]
fn ordinary_retry_does_not_credit_sleep_before_its_fresh_selection() -> std::io::Result<()> {
    isolated("old-sleep")
}

#[test]
fn ordinary_retry_keeps_positive_headerless_local_backoff() -> std::io::Result<()> {
    isolated("no-advice")
}

#[test]
fn ordinary_retry_selected_before_notification_can_still_be_cancelled() -> std::io::Result<()> {
    isolated("cancel")
}

#[test]
fn inactive_ordinary_retry_preserves_its_provider_wait_without_sampling() -> std::io::Result<()> {
    isolated("inactive")
}

#[test]
fn connection_retry_counts_sleep_inside_notification_and_resets_doubling() -> std::io::Result<()> {
    isolated("connection-sleep")
}

#[test]
fn inactive_connection_retry_keeps_local_wait_and_doubling_without_sampling() -> std::io::Result<()>
{
    isolated("connection-inactive")
}

#[test]
fn extreme_valid_provider_advice_does_not_evaluate_an_overflowing_unused_fallback()
-> std::io::Result<()> {
    isolated("extreme")
}

#[test]
fn ordinary_retry_wake_child() {
    let Ok(case) = std::env::var(CHILD) else {
        return;
    };
    assert_eq!(
        std::env::args().skip(1).collect::<Vec<_>>(),
        ["--exact", &child_name(), "--nocapture", "--test-threads=1"]
    );
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("private fixture runtime")
        .block_on(async {
            tokio::time::timeout(Duration::from_secs(60), ordinary_retry(&case))
                .await
                .expect("ordinary retry wake hang detector")
        });
    #[expect(
        clippy::print_stdout,
        reason = "parent requires exact private child completion marker"
    )]
    {
        println!("{COMPLETE}:{case}");
    }
}
