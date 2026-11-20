// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use codex_core::StartIfIdleSubmission;
use codex_core::TurnInput;
use codex_core::TurnInputRequest;
use codex_core::TurnInputSubmission;
use codex_core::config::Config;
use codex_extension_api::ExtensionFuture;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::NoopExtensionEventSink;
use codex_extension_api::ThreadIdleInput;
use codex_extension_api::ThreadLifecycleContributor;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::CodexErrorInfo;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::TurnCompleteEvent;
use codex_protocol::user_input::UserInput;
use codex_queue_extension::QueuedItemService;
use codex_rollout::RolloutItem;
use codex_rollout::RolloutRecorder;
use codex_thread_store::LocalQueueStore;
use core_test_support::responses;
use core_test_support::streaming_sse::StreamingSseChunk;
use core_test_support::streaming_sse::start_streaming_sse_server;
use core_test_support::test_codex::TestCodex;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event_match;
use tokio::sync::oneshot;
use tokio::sync::watch;
use tokio::time::timeout;

// Core publishes TurnComplete before the idle callback. Observe its return so a
// slow dispatcher cannot make the queue look preserved before it starts work.
struct ObservedQueue {
    service: OnceLock<QueuedItemService>,
    idle: watch::Sender<()>,
}

impl ThreadLifecycleContributor<Config> for ObservedQueue {
    fn on_thread_idle<'a>(&'a self, input: ThreadIdleInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if let Some(queue) = self.service.get() {
                <QueuedItemService as ThreadLifecycleContributor<Config>>::on_thread_idle(
                    queue, input,
                )
                .await;
                self.idle.send_replace(());
            }
        })
    }
}

fn input(text: &str) -> TurnInput {
    TurnInput::UserInput {
        content: vec![UserInput::Text {
            text: text.to_owned(),
            text_elements: Vec::new(),
        }],
        client_id: None,
    }
}

async fn next_completion(test: &TestCodex) -> TurnCompleteEvent {
    wait_for_event_match(test.codex.as_ref(), |event| match event {
        EventMsg::TurnComplete(turn) => Some(turn.clone()),
        _ => None,
    })
    .await
}

#[derive(Default)]
struct PauseFirstTurnStop {
    stopped: std::sync::atomic::AtomicBool,
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
}

impl codex_extension_api::TurnLifecycleContributor for PauseFirstTurnStop {
    fn on_turn_stop<'a>(
        &'a self,
        _input: codex_extension_api::TurnStopInput<'a>,
    ) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if !self.stopped.swap(true, std::sync::atomic::Ordering::SeqCst) {
                self.entered.notify_one();
                self.release.notified().await;
            }
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn relay_cannot_replace_the_finishing_turn_before_its_finding_is_published() -> Result<()> {
    use codex_core::NotSubmittedReason;
    use codex_core::SteerSubmission;
    use codex_protocol::turn_input::TurnStartOptions;
    use core_test_support::ThreadIdle;

    codex_alab_core::activate();
    let server = responses::start_mock_server().await;
    let model = responses::mount_sse_sequence(
        &server,
        vec![
            responses::sse_failed(
                "refused",
                "misalignment_policy_violation",
                "Review the cleanup.",
            ),
            responses::sse_completed("correction"),
        ],
    )
    .await;
    let pause = Arc::new(PauseFirstTurnStop::default());
    let mut extensions = ExtensionRegistryBuilder::new();
    extensions.turn_lifecycle_contributor(pause.clone());
    extensions.thread_lifecycle_contributor(Arc::new(ThreadIdle));
    let test = test_codex()
        .with_extensions(Arc::new(extensions.build()))
        .build_with_auto_env(&server)
        .await?;

    let started = test
        .codex
        .start_or_steer_turn(TurnInputRequest::new(input("Inspect disk use.")))
        .await?;
    let TurnInputSubmission::Started { turn_id, .. } = started else {
        anyhow::bail!("initial user turn did not start");
    };
    // A delegated send_input inherits a user trigger and carries its parent turn.
    let relay = || {
        TurnInputRequest::new(input("Relay the cleanup request.")).on_start(TurnStartOptions {
            turn_trigger: Some("user".into()),
            parent_turn_id: Some(turn_id.clone()),
            ..Default::default()
        })
    };
    // The active task was taken, but its native finishing reservation is still owned.
    let bound = Duration::from_secs(60);
    timeout(bound, pause.entered.notified()).await?;
    let stale_request = || TurnInputRequest::new(input("Inspect the previous turn."));
    let during = timeout(bound, test.codex.start_or_steer_turn(relay())).await;
    let strict_during = timeout(
        bound,
        test.codex.steer_turn(stale_request(), turn_id.clone()),
    )
    .await;
    pause.release.notify_one();
    assert_eq!(
        during??,
        TurnInputSubmission::NotSubmitted {
            reason: NotSubmittedReason::NotIdle
        }
    );
    assert_eq!(
        strict_during??,
        SteerSubmission::NotSubmitted {
            reason: NotSubmittedReason::NotIdle
        }
    );

    let refused = next_completion(&test).await;
    assert_eq!(refused.turn_id, turn_id);
    assert_eq!(
        refused.error.and_then(|e| e.codex_error_info),
        Some(CodexErrorInfo::MisalignmentPolicyViolation)
    );
    ThreadIdle::wait(&test.codex).await;
    // Enter was pressed before the TUI saw the finding. A missing-turn answer
    // would make the client promote this same steer to an ordinary turn/start.
    for expected in [turn_id.as_str(), "older-turn"] {
        assert_eq!(
            test.codex
                .steer_turn(stale_request(), expected.to_owned())
                .await?,
            SteerSubmission::NotSubmitted {
                reason: NotSubmittedReason::MisalignmentReview {
                    turn_id: turn_id.clone()
                }
            }
        );
    }
    let unrelated = test_codex().build_with_auto_env(&server).await?;
    assert_eq!(
        unrelated
            .codex
            .steer_turn(stale_request(), turn_id.clone())
            .await?,
        SteerSubmission::NotSubmitted {
            reason: NotSubmittedReason::NoActiveTurn
        }
    );
    let after = test.codex.start_or_steer_turn(relay()).await?;
    assert_eq!(
        after,
        TurnInputSubmission::NotSubmitted {
            reason: NotSubmittedReason::MisalignmentReview { turn_id }
        }
    );
    assert_eq!(model.requests().len(), 1);

    let correction = "Report free space without changing files.";
    let started = test
        .codex
        .start_or_steer_turn(TurnInputRequest::new(input(correction)))
        .await?;
    let TurnInputSubmission::Started { turn_id, .. } = started else {
        anyhow::bail!("explicit correction did not start");
    };
    let completed = next_completion(&test).await;
    assert_eq!(completed.turn_id, turn_id);
    assert_eq!(completed.error, None);
    ThreadIdle::wait(&test.codex).await;
    assert_eq!(
        test.codex
            .steer_turn(stale_request(), refused.turn_id)
            .await?,
        SteerSubmission::NotSubmitted {
            reason: NotSubmittedReason::NoActiveTurn
        }
    );
    let requests = model.requests();
    assert_eq!(requests.len(), 2);
    assert!(
        requests[1]
            .message_input_texts("user")
            .iter()
            .any(|text| text == correction)
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn observed_steer_fallback_cannot_answer_a_finding_after_a_missing_reply() -> Result<()> {
    use codex_core::NotSubmittedReason;
    use codex_core::SteerSubmission;
    use codex_protocol::turn_input::TurnStartOptions;
    use core_test_support::ThreadIdle;

    fn request(text: &str, trigger: &str) -> TurnInputRequest {
        TurnInputRequest::new(input(text)).on_start(TurnStartOptions {
            turn_trigger: Some(trigger.to_owned()),
            ..Default::default()
        })
    }

    async fn run(test: &TestCodex, request: TurnInputRequest) -> Result<TurnCompleteEvent> {
        let TurnInputSubmission::Started { turn_id, .. } =
            test.codex.start_or_steer_turn(request).await?
        else {
            anyhow::bail!("expected the request to start a new turn");
        };
        let completed = next_completion(test).await;
        assert_eq!(completed.turn_id, turn_id);
        ThreadIdle::wait(&test.codex).await;
        Ok(completed)
    }

    codex_alab_core::activate();
    let server = responses::start_mock_server().await;
    let model = responses::mount_sse_sequence(
        &server,
        vec![
            responses::sse_completed("original"),
            responses::sse_failed(
                "refused",
                "misalignment_policy_violation",
                "Review the cleanup.",
            ),
            responses::sse_completed("corrected"),
            responses::sse_completed("fallback"),
        ],
    )
    .await;
    let mut extensions = ExtensionRegistryBuilder::new();
    extensions.thread_lifecycle_contributor(Arc::new(ThreadIdle));
    let test = test_codex()
        .with_extensions(Arc::new(extensions.build()))
        .build_with_auto_env(&server)
        .await?;
    let original = run(&test, request("Measure generated files.", "user")).await?;
    assert_eq!(original.error, None);
    let stale = "Remove the generated files.";
    assert_eq!(
        test.codex
            .steer_turn(request(stale, "user"), original.turn_id)
            .await?,
        SteerSubmission::NotSubmitted {
            reason: NotSubmittedReason::NoActiveTurn
        }
    );
    // Another writer finishes with a finding between the client's two RPCs.
    let finding = run(&test, request("Inspect the cleanup.", "user")).await?;
    assert_eq!(
        finding.error.and_then(|error| error.codex_error_info),
        Some(CodexErrorInfo::MisalignmentPolicyViolation)
    );
    assert_eq!(
        test.codex
            .start_or_steer_turn(request(stale, "user_steer_fallback"))
            .await?,
        TurnInputSubmission::NotSubmitted {
            reason: NotSubmittedReason::MisalignmentReview {
                turn_id: finding.turn_id
            },
        }
    );
    assert_eq!(model.requests().len(), 2);
    assert!(test.codex.alab_queue_review_pending().await);
    let corrected = run(
        &test,
        request("Report disk use without deleting files.", "user"),
    )
    .await?;
    assert_eq!(corrected.error, None);
    assert!(!test.codex.alab_queue_review_pending().await);

    let accepted = run(
        &test,
        request("List the file sizes.", "user_steer_fallback"),
    )
    .await?;
    assert_eq!(accepted.error, None);
    let requests = model.requests();
    assert_eq!(requests.len(), 4);
    let metadata: serde_json::Value = serde_json::from_str(
        &requests[3]
            .header("x-codex-turn-metadata")
            .context("missing provider turn metadata")?,
    )?;
    assert_eq!(metadata["turn_id"], accepted.turn_id);
    assert_eq!(metadata["turn_trigger"], "user");
    assert!(requests.iter().all(|r| {
        !r.message_input_texts("user")
            .iter()
            .any(|text| text == stale)
    }));
    test.codex.flush_rollout().await?;
    let path = test.codex.rollout_path().context("local rollout")?;
    let history = RolloutRecorder::get_rollout_history(&path).await?;
    let source = history
        .get_rollout_items()
        .iter()
        .find_map(|item| match item {
            RolloutItem::EventMsg(EventMsg::TurnStarted(start))
                if start.turn_id == accepted.turn_id =>
            {
                start.turn_attribution.as_ref()
            }
            _ => None,
        })
        .context("missing accepted turn attribution")?;
    assert_eq!(source.turn_trigger.as_deref(), Some("user"));
    assert_eq!(source.parent_turn_id, None);
    assert_eq!(source.initiating_agent_path, None);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn typed_provider_refusal_holds_queued_input_until_explicit_start() -> Result<()> {
    codex_alab_core::activate();
    let (release_refusal, refusal_gate) = oneshot::channel();
    let (server, _streams) = start_streaming_sse_server(vec![
        vec![StreamingSseChunk {
            gate: Some(refusal_gate),
            body: responses::sse_failed(
                "refused",
                "misalignment_policy_violation",
                "Review the cleanup.",
            ),
        }],
        vec![StreamingSseChunk {
            gate: None,
            body: responses::sse_failed("invalid", "invalid_prompt", "Invalid request."),
        }],
        vec![StreamingSseChunk {
            gate: None,
            body: responses::sse_completed("finished"),
        }],
    ])
    .await;
    let (idle, mut idle_observed) = watch::channel(());
    let observer = Arc::new(ObservedQueue {
        service: OnceLock::new(),
        idle,
    });
    let mut extensions = ExtensionRegistryBuilder::new();
    extensions.thread_lifecycle_contributor(observer.clone());
    let test = test_codex()
        .with_extensions(Arc::new(extensions.build()))
        .build_with_streaming_server(&server)
        .await?;
    let thread_id = test.session_configured.thread_id;
    let store = test.codex.state_db().context("queue store unavailable")?;
    let queue = observer.service.get_or_init(|| {
        QueuedItemService::new(
            Arc::new(LocalQueueStore::new(store)),
            Arc::downgrade(&test.thread_manager),
            Arc::new(NoopExtensionEventSink),
        )
    });
    let started = test
        .codex
        .start_or_steer_turn(TurnInputRequest::new(input("Inspect disk use.")))
        .await?;
    let TurnInputSubmission::Started { turn_id, .. } = started else {
        anyhow::bail!("initial user turn did not start");
    };
    let bound = Duration::from_secs(60);
    timeout(bound, server.wait_for_request_count(1)).await?;
    let correction = "List generated files.";
    let queued = queue.enqueue(thread_id, input(correction)).await?;
    release_refusal
        .send(())
        .expect("provider still awaits the queued item");
    let refused = next_completion(&test).await;
    assert_eq!(refused.turn_id, turn_id);
    assert_eq!(
        refused.error.and_then(|e| e.codex_error_info),
        Some(CodexErrorInfo::MisalignmentPolicyViolation)
    );
    timeout(bound, idle_observed.changed()).await??;
    assert_eq!(queue.list(thread_id).await?, vec![queued.clone()]);
    assert_eq!(server.requests().await.len(), 1);

    // Enqueue wakes an already idle thread, just as an external change does.
    let later = queue
        .enqueue(thread_id, input("Report free space."))
        .await?;
    assert_eq!(queue.list(thread_id).await?, vec![queued.clone(), later]);
    assert_eq!(server.requests().await.len(), 1);
    idle_observed.borrow_and_update();

    let started = queue
        .start(test.codex.as_ref(), Some(queued.id), /*trace*/ None)
        .await?;
    let StartIfIdleSubmission::Started { turn_id, .. } = started else {
        anyhow::bail!("explicit start did not submit the selected queued item");
    };
    let invalid = next_completion(&test).await;
    assert_eq!(invalid.turn_id, turn_id);
    assert_eq!(
        invalid.error.and_then(|e| e.codex_error_info),
        Some(CodexErrorInfo::InvalidPrompt)
    );
    let drained = next_completion(&test).await;
    assert_ne!(drained.turn_id, turn_id);
    assert_eq!(drained.error, None);
    timeout(bound, idle_observed.changed()).await??;
    assert!(queue.list(thread_id).await?.is_empty());
    assert_eq!(server.requests().await.len(), 3);
    server.shutdown().await;

    test.codex.flush_rollout().await?;
    let path = test.codex.rollout_path().context("local queue rollout")?;
    let history = RolloutRecorder::get_rollout_history(&path).await?;
    let items = history.get_rollout_items();
    let mut corrections = 0;
    for (index, item) in items.iter().enumerate() {
        let RolloutItem::ResponseItem(envelope) = item else {
            continue;
        };
        let metadata = serde_json::to_value(&envelope.metadata)?;
        let Some(reviewed) = metadata.pointer("/alab_input_review_v1/corrected") else {
            continue;
        };
        corrections += 1;
        assert_eq!(reviewed.as_str(), Some(refused.turn_id.as_str()));
        assert_eq!(envelope.item.turn_id(), Some(turn_id.as_str()));
        let ResponseItem::Message { role, content, .. } = &envelope.item else {
            anyhow::bail!("review receipt on a non-message item");
        };
        assert_eq!(role, "user");
        assert!(matches!(
            content.as_slice(),
            [ContentItem::InputText { text }] if text == correction
        ));
        assert_eq!(
            codex_core::pending_misalignment_review(&items[..index]).map(|turn| &turn.turn_id),
            Some(&refused.turn_id)
        );
    }
    assert_eq!(
        corrections, 1,
        "only the explicitly selected prompt acknowledges review"
    );
    assert!(codex_core::pending_misalignment_review(items).is_none());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn trigger_turn_mail_waits_for_review_and_reaches_the_explicit_queue_turn() -> Result<()> {
    use codex_protocol::AgentPath;
    use codex_protocol::protocol::InterAgentCommunication;

    codex_alab_core::activate();
    let server = responses::start_mock_server().await;
    let model = responses::mount_sse_sequence(
        &server,
        vec![
            responses::sse_failed(
                "refused",
                "misalignment_policy_violation",
                "Review the cleanup.",
            ),
            responses::sse_completed("correction-input"),
            responses::sse_completed("delivered-mail"),
        ],
    )
    .await;
    let (idle, mut idle_observed) = watch::channel(());
    let observer = Arc::new(ObservedQueue {
        service: OnceLock::new(),
        idle,
    });
    let mut extensions = ExtensionRegistryBuilder::new();
    extensions.thread_lifecycle_contributor(observer.clone());
    let test = test_codex()
        .with_extensions(Arc::new(extensions.build()))
        .build_with_auto_env(&server)
        .await?;
    let thread_id = test.session_configured.thread_id;
    let store = test.codex.state_db().context("queue store unavailable")?;
    let queue = observer.service.get_or_init(|| {
        QueuedItemService::new(
            Arc::new(LocalQueueStore::new(store)),
            Arc::downgrade(&test.thread_manager),
            Arc::new(NoopExtensionEventSink),
        )
    });
    let initial = test
        .codex
        .start_or_steer_turn(TurnInputRequest::new(input("Inspect disk use.")))
        .await?;
    let TurnInputSubmission::Started {
        turn_id: initial_id,
        ..
    } = initial
    else {
        anyhow::bail!("initial user turn did not start");
    };
    let refused = next_completion(&test).await;
    assert_eq!(refused.turn_id, initial_id);
    assert_eq!(
        refused.error.and_then(|e| e.codex_error_info),
        Some(CodexErrorInfo::MisalignmentPolicyViolation)
    );
    timeout(Duration::from_secs(60), idle_observed.changed()).await??;

    let update = "Worker measured the generated files.";
    let mail = InterAgentCommunication::new(
        AgentPath::try_from("/root/worker").expect("worker path"),
        AgentPath::root(),
        Vec::new(),
        update.into(),
        /*trigger_turn*/ true,
    );
    // Accepted returns after the real mailbox scheduler tried to start an idle turn.
    let accepted = test
        .codex
        .start_or_steer_turn(TurnInputRequest::new(TurnInput::InterAgentCommunication(
            mail,
        )))
        .await?;
    assert_eq!(accepted, TurnInputSubmission::Accepted);
    assert!(
        !test.codex.has_active_turn().await,
        "mail started a turn during review"
    );
    assert_eq!(model.requests().len(), 1);

    let correction = "Report free space without changing files.";
    let selected = queue.enqueue(thread_id, input(correction)).await?;
    assert_eq!(queue.list(thread_id).await?, vec![selected.clone()]);
    assert_eq!(model.requests().len(), 1);
    let started = queue
        .start(test.codex.as_ref(), Some(selected.id), /*trace*/ None)
        .await?;
    let StartIfIdleSubmission::Started { turn_id, .. } = started else {
        anyhow::bail!("pending agent mail blocked an explicit queue start");
    };
    let completed = next_completion(&test).await;
    let requests = model.requests();
    let received = server
        .received_requests()
        .await
        .context("request recording unavailable")?;
    let paths: Vec<_> = received
        .iter()
        .map(|r| (r.method.as_str(), r.url.path()))
        .collect();
    let kinds: Vec<Vec<_>> = requests
        .iter()
        .map(|r| {
            r.input()
                .iter()
                .map(|item| {
                    let kind = item["type"].as_str().unwrap_or("unknown");
                    let role = item["role"].as_str().unwrap_or("");
                    format!("{kind}/{role}")
                })
                .collect()
        })
        .collect();
    let post_count = paths
        .iter()
        .filter(|entry| **entry == ("POST", "/v1/responses"))
        .count();
    assert_eq!(
        (post_count, requests.len()),
        (3, 3),
        "HTTP paths {paths:?}, input kinds {kinds:?}, completion {:?}",
        completed.error
    );
    assert_eq!(completed.turn_id, turn_id);
    assert_ne!(turn_id, initial_id);
    assert_eq!(
        requests[0].body_json()["client_metadata"]["turn_id"],
        initial_id
    );
    // The explicit queue item is sampled first. Pending worker mail is consumed
    // on the next model step of that same turn.
    for request in &requests[1..] {
        assert_eq!(request.body_json()["client_metadata"]["turn_id"], turn_id);
    }
    assert_eq!(
        requests[1]
            .message_input_texts("user")
            .iter()
            .filter(|text| text.as_str() == correction)
            .count(),
        1,
        "input kinds {kinds:?}"
    );
    assert!(
        requests[1].inputs_of_type("agent_message").is_empty(),
        "input kinds {kinds:?}"
    );
    let mail = requests[2].inputs_of_type("agent_message");
    assert_eq!(mail.len(), 1, "input kinds {kinds:?}");
    assert!(
        mail[0]["content"]
            .as_array()
            .context("agent message content")?
            .iter()
            .any(|part| part["text"]
                .as_str()
                .is_some_and(|text| text.contains(update))),
        "selected turn lost the pending worker message"
    );
    assert_eq!(
        completed.error, None,
        "HTTP paths {paths:?}, input kinds {kinds:?}"
    );
    Ok(())
}
