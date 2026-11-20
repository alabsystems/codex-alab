// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::session::input_queue::TurnInput as QueuedInput;
use codex_history::RolloutItem;
use codex_protocol::items::TurnItem;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::ReviewRequest;
use codex_protocol::protocol::ReviewTarget;
use codex_protocol::turn_input::SteerSubmission;
use codex_protocol::turn_input::TurnInput;
use codex_protocol::turn_input::TurnInputRequest;
use codex_protocol::turn_input::TurnInputSubmission;
use codex_protocol::user_input::UserInput;
use codex_rollout::RolloutRecorder;
use core_test_support::responses::mount_response_handler;
use core_test_support::responses::sse_completed;
use core_test_support::responses::sse_response;
use core_test_support::responses::start_mock_server;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::timeout;

const REVIEW: &str = "Inspect the original change before the tree handoff";
const NESTED: &str = "Perform the nested work before the tree handoff";
const QUEUED: &str = "Unconsumed follow-up for the original nested turn";
const CLIENT: &str = "original-nested-client";

fn text(value: &str) -> Vec<UserInput> {
    vec![UserInput::Text {
        text: value.into(),
        text_elements: vec![],
    }]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exec_review_tree_suspends_original_delegate_grandchild_and_queued_user_input() {
    timeout(Duration::from_secs(60), suspend_live_review_tree())
        .await
        .expect("source tree did not finish suspension before the deadline");
}

async fn suspend_live_review_tree() {
    let server = start_mock_server().await;
    let (sent, mut received) = mpsc::unbounded_channel();
    let requests = mount_response_handler(&server, move |request| {
        sent.send(request.message_input_texts("user")).unwrap();
        // Keep the transport opening until suspension. Once SSE headers arrive,
        // an ordinary steer can preempt the stream and enter history before capture.
        sse_response(sse_completed("withheld")).set_delay(Duration::from_secs(120))
    })
    .await;
    let (_home, config, manager, root) =
        fixture_with_options(false, None, Some(format!("{}/v1", server.uri()))).await;
    let root_id = root.exec_thread_id();
    let root_turn = root
        .submit(Op::Review {
            review_request: ReviewRequest {
                target: ReviewTarget::Custom {
                    instructions: REVIEW.into(),
                },
                user_facing_hint: None,
            },
        })
        .await
        .unwrap();
    let review_request = received.recv().await.unwrap();
    assert!(review_request.iter().any(|input| input.contains(REVIEW)));

    // The real review delegate is internal, so obtain its original handle from
    // the source inventory and release the seal before registering its child.
    let delegate = {
        let inventory = root.seal_exec_tree_starts().await.unwrap();
        inventory
            .loaded_threads(&root)
            .await
            .unwrap()
            .into_iter()
            .find(|thread| {
                matches!(
                    thread.session_source(),
                    SessionSource::Internal(InternalSessionSource::Review)
                )
            })
            .expect("original review delegate")
    };
    let delegate_id = delegate.exec_thread_id();
    let delegate_turn = delegate
        .session
        .active_turn
        .lock()
        .await
        .as_ref()
        .unwrap()
        .task
        .as_ref()
        .unwrap()
        .turn_context
        .sub_id
        .clone();
    let nested = child(&manager, &delegate, &config).await;
    let nested_id = nested.exec_thread_id();
    let TurnInputSubmission::Started {
        turn_id: nested_turn,
        ..
    } = nested
        .start_or_steer_turn(TurnInputRequest::user_input(text(NESTED)))
        .await
        .unwrap()
    else {
        panic!("nested turn did not start")
    };
    let nested_request = received.recv().await.unwrap();
    assert!(nested_request.iter().any(|input| input == NESTED));
    let queued = TurnInputRequest::new(TurnInput::UserInput {
        content: text(QUEUED),
        client_id: Some(CLIENT.into()),
    });
    assert_eq!(
        nested
            .steer_turn(queued, nested_turn.clone())
            .await
            .unwrap(),
        SteerSubmission::Steered {
            turn_id: nested_turn.clone()
        }
    );

    let source_input = {
        let active = nested.session.active_turn.lock().await;
        let active = active.as_ref().expect("original nested turn is live");
        assert_eq!(
            active.task.as_ref().unwrap().turn_context.sub_id,
            nested_turn
        );
        let state = active.turn_state.lock().await;
        state.pending_input.pending_items_for_test().to_vec()
    };
    let users: Vec<_> = source_input
        .iter()
        .filter_map(|item| match item {
            QueuedInput::UserInput {
                content, client_id, ..
            } => Some((content, client_id.as_deref())),
            _ => None,
        })
        .collect();
    assert_eq!(
        users,
        [(&text(QUEUED), Some(CLIENT))],
        "the live nested turn must own just the submitted user steer: {source_input:?}"
    );

    let invocation = ExecConfiguration::capture(&config, &ConfigOverrides::default()).unwrap();
    let plan = root
        .prepare_exec_tree(&root_turn, &invocation)
        .await
        .unwrap();
    let captured = plan.suspend_and_capture().await.unwrap();
    let saved = super::super::super::ExecTreeSnapshot::capture(&captured).unwrap();
    saved.validate(root_id, Some(&root_turn)).unwrap();
    assert!(!root.is_running() && !delegate.is_running() && !nested.is_running());
    assert_eq!(saved.threads().len(), 3);
    let node = |id| {
        saved
            .threads()
            .iter()
            .find(|node| node.thread() == id)
            .unwrap()
    };
    assert_eq!(node(root_id).parent(), None);
    assert!(node(root_id).recovery().unwrap().is_review());
    let review = node(root_id).review().unwrap();
    assert_eq!(
        (review.delegate(), review.delegate_turn()),
        (delegate_id, delegate_turn.as_str())
    );
    assert_eq!(node(delegate_id).parent(), Some(root_id));
    assert_eq!(node(delegate_id).recovery().unwrap().turn(), delegate_turn);
    assert_eq!(node(nested_id).parent(), Some(delegate_id));
    assert_eq!(node(nested_id).recovery().unwrap().turn(), nested_turn);

    let mut pending = saved.inputs().clone().prepare(root_id, &root_turn).unwrap();
    assert!(
        pending
            .take_turn_input_for_test(nested_id, "unknown-turn")
            .is_err()
    );
    for (thread, turn) in [(root_id, &root_turn), (delegate_id, &delegate_turn)] {
        assert!(
            pending
                .take_turn_input_for_test(thread, turn)
                .unwrap()
                .is_empty()
        );
    }
    let input = pending
        .take_turn_input_for_test(nested_id, &nested_turn)
        .unwrap();
    assert_eq!(
        input, source_input,
        "the suspended nested turn must retain its entire original input queue"
    );
    assert!(
        pending
            .take_turn_input_for_test(nested_id, &nested_turn)
            .is_err()
    );
    assert!(pending.is_empty());

    // Decoded, writer-closed history and the queue must agree on which user
    // input already reached the original turn. A pending steer cannot also
    // appear as a completed user item or as model input.
    let mut completed = Vec::new();
    let mut messages = Vec::new();
    for original in [&root, &delegate, &nested] {
        let thread = node(original.exec_thread_id());
        for entry in thread.history().unwrap().items {
            if let RolloutItem::EventMsg(EventMsg::ItemCompleted(event)) = entry
                && let TurnItem::UserMessage(item) = event.item
            {
                let body = item.message();
                if body == NESTED || body == QUEUED {
                    assert_eq!(event.thread_id, thread.thread());
                    completed.push((event.thread_id, event.turn_id, body));
                }
            }
        }
        // Recovery evidence records completed user items. Read the original
        // writer's full rollout independently for the model-facing messages.
        let path = original.rollout_path().expect("durable source member");
        let (items, owner, invalid) = RolloutRecorder::load_rollout_items(&path).await.unwrap();
        assert_eq!((owner, invalid), (Some(thread.thread()), 0));
        for entry in items {
            if let RolloutItem::ResponseItem(envelope) = entry
                && let ResponseItem::Message { role, content, .. } = envelope.item
                && role == "user"
            {
                for item in content {
                    if let ContentItem::InputText { text } = item
                        && (text == NESTED || text == QUEUED)
                    {
                        messages.push((thread.thread(), text));
                    }
                }
            }
        }
    }
    assert_eq!(completed, [(nested_id, nested_turn, NESTED.to_owned())]);
    assert_eq!(messages, [(nested_id, NESTED.to_owned())]);

    let requests = requests.requests();
    assert_eq!(requests.len(), 2);
    for (request, original) in requests.iter().zip([REVIEW, NESTED]) {
        let input = request.message_input_texts("user");
        assert_eq!(
            input.iter().filter(|text| text.contains(original)).count(),
            1
        );
        assert!(input.iter().all(|text| !text.contains(QUEUED)));
    }
}
