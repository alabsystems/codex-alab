// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use codex_extension_api::ExtensionFuture;
use codex_extension_api::MessageLifecycleContributor;
use codex_extension_api::MessageLifecycleInput;
use codex_extension_api::TurnLifecycleContributor;
use codex_extension_api::TurnStopInput;
use codex_features::Feature;
use codex_history::RolloutItem;
use codex_protocol::ThreadId;
use codex_protocol::models::ResponseItem;
use codex_protocol::models::plaintext_agent_message_content;
use codex_protocol::protocol::ThreadHistoryMode;
use core_test_support::ThreadIdle;
use core_test_support::wait_for_event_match;
use std::collections::HashMap;
use std::sync::Mutex;
use tokio::sync::Semaphore;

const WORKERS: [(&str, &str); 2] = [
    ("cedar", "The cedar measurement was twenty-seven."),
    ("elm", "The elm measurement was forty-three."),
];

// Keep both real model turns at their stop callback until their caller is idle. With passive
// mail, Core can then accept their terminal publications without consuming them in a model turn.
struct FinishingChildren(Semaphore);

impl TurnLifecycleContributor for FinishingChildren {
    fn on_turn_stop<'a>(&'a self, input: TurnStopInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if input.thread_store.level_id() != input.session_store.level_id() {
                self.0.acquire().await.expect("child stop gate").forget();
            }
        })
    }
}

#[derive(Default)]
struct Deliveries(Mutex<HashMap<String, (ThreadId, Option<ThreadId>)>>);

impl MessageLifecycleContributor for Deliveries {
    fn on_publish(&self, input: &MessageLifecycleInput) {
        let mut messages = self.0.lock().expect("delivery observer");
        assert!(
            messages
                .insert(input.data.level_id().to_owned(), (input.sender, None))
                .is_none()
        );
    }

    fn on_consumed(&self, input: &MessageLifecycleInput, recipient: ThreadId) {
        let mut messages = self.0.lock().expect("delivery observer");
        let (sender, consumed) = messages
            .get_mut(input.data.level_id())
            .expect("same publication");
        assert_eq!(*sender, input.sender);
        assert!(
            consumed.replace(recipient).is_none(),
            "publication consumed twice"
        );
    }
}

impl Deliveries {
    fn assert_worker_delivery(&self, sender: ThreadId, recipient: Option<ThreadId>) {
        let messages = self.0.lock().expect("delivery observer");
        let matching: Vec<_> = messages
            .values()
            .filter(|(from, _)| *from == sender)
            .map(|(_, to)| *to)
            .collect();
        assert_eq!(matching, [recipient], "worker's sole terminal publication");
    }
}

async fn saved(thread: &codex_core::CodexThread) -> Vec<RolloutItem> {
    let path = thread.rollout_path().expect("persistent thread");
    let (items, id, errors) = codex_core::RolloutRecorder::load_rollout_items(&path)
        .await
        .expect("read closed rollout");
    assert_eq!(errors, 0, "malformed lines in {path:?}");
    assert_eq!(id, Some(thread.startup_metadata().thread_id));
    let Some(RolloutItem::SessionMeta(meta)) = items.first() else {
        panic!("missing original metadata: {path:?}")
    };
    assert_eq!(
        meta.meta.parent_thread_id,
        thread.session_source().parent_thread_id()
    );
    assert_eq!(
        meta.meta.source.get_agent_path(),
        thread.session_source().get_agent_path()
    );
    items
}

fn agent_messages(items: &[RolloutItem]) -> Vec<(&str, &str, String)> {
    items
        .iter()
        .filter_map(|item| {
            let RolloutItem::ResponseItem(item) = item else {
                return None;
            };
            let ResponseItem::AgentMessage {
                author,
                recipient,
                content,
                ..
            } = &item.item
            else {
                return None;
            };
            Some((
                author.as_str(),
                recipient.as_str(),
                plaintext_agent_message_content(content).expect("plaintext fixture message"),
            ))
        })
        .collect()
}

async fn mount_workers(server: &wiremock::MockServer) -> responses::ResponseMock {
    let mut spawn = vec![responses::ev_response_created("spawn")];
    for (name, _) in WORKERS {
        let args = serde_json::json!({"task_name": name, "message": format!("Measure the {name}."), "fork_turns": "none"});
        spawn.push(responses::ev_function_call_with_namespace(
            name,
            "collaboration",
            "spawn_agent",
            &args.to_string(),
        ));
    }
    spawn.push(responses::ev_completed("spawn"));
    responses::mount_response_handler(server, move |request| {
        let body = if request.body_json()["client_metadata"]["x-codex-parent-thread-id"].is_string()
        {
            let input = serde_json::to_string(&request.input()).expect("worker input");
            let mut matching = WORKERS
                .into_iter()
                .filter(|(name, _)| input.contains(&format!("Measure the {name}.")));
            let (name, answer) = matching.next().expect("worker received its task");
            assert!(
                matching.next().is_none(),
                "worker received two initial tasks"
            );
            responses::sse(vec![
                responses::ev_assistant_message(name, answer),
                responses::ev_completed(name),
            ])
        } else if request.inputs_of_type("function_call_output").is_empty() {
            responses::sse(spawn.clone())
        } else {
            responses::sse_completed("root-finished")
        };
        responses::sse_response(body)
    })
    .await
}

#[test]
fn tree_suspension_persists_each_pending_child_result_and_retires_the_queued_goal() {
    big_stack(|| async {
        codex_alab_core::activate();
        let server = responses::start_mock_server().await;
        let finishes = Arc::new(FinishingChildren(Semaphore::new(0)));
        let deliveries = Arc::new(Deliveries::default());
        let late = Arc::new(late_goal::LateGoal::with_event_sink(None));
        let mut registry = ExtensionRegistryBuilder::<Config>::new();
        registry.turn_lifecycle_contributor(finishes.clone());
        registry.thread_lifecycle_contributor(Arc::new(ThreadIdle));
        registry.message_lifecycle_contributor(deliveries.clone());
        late.register(&mut registry);
        let test = test_codex()
            .with_model("gpt-5.6-sol")
            .with_history_mode(ThreadHistoryMode::Paginated)
            .with_extensions(Arc::new(registry.build()))
            .with_config(|cfg| {
                cfg.multi_agent_v2.max_concurrent_threads_per_session = 3;
                cfg.features.enable(Feature::Collab).expect("collaboration");
                cfg.features
                    .enable(Feature::MultiAgentV2)
                    .expect("V2 agent results");
                cfg.features.disable(Feature::WakeOnAgentMessages);
            })
            .build(&server)
            .await
            .expect("local tree with a goal runtime");
        let fixture = Fixture::bind(test, &late).await;
        let root = &fixture.test.codex;
        let manager = &fixture.test.thread_manager;
        let root_id = root.startup_metadata().thread_id;
        let mut created = manager.subscribe_thread_created();

        let requests = mount_workers(&server).await;
        let start = root
            .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                text: "Ask both workers for their measurements.".into(),
                text_elements: Vec::new(),
            }]))
            .await
            .expect("start original root turn");
        let TurnInputSubmission::Started {
            turn_id: root_turn, ..
        } = start
        else {
            panic!("root turn did not start: {start:?}");
        };
        ThreadIdle::wait(root).await;
        let mut children = Vec::new();
        for _ in WORKERS {
            let id = timeout(DEADLINE, created.recv())
                .await
                .expect("child created")
                .expect("thread creation");
            let worker = manager
                .get_thread(id)
                .await
                .expect("registered reporting child");
            assert_eq!(worker.session_source().parent_thread_id(), Some(root_id));
            let path = worker
                .session_source()
                .get_agent_path()
                .expect("canonical worker path");
            let turn = wait_for_event_match(&worker, |event| match event {
                EventMsg::TurnStarted(ev) => Some(ev.turn_id.clone()),
                _ => None,
            })
            .await;
            children.push((id, path, turn, worker));
        }
        assert_ne!(children[0].0, children[1].0);
        assert_ne!(children[0].1, children[1].1);
        assert_ne!(children[0].2, children[1].2);
        finishes.0.add_permits(children.len());
        for (_, _, _, worker) in &children {
            ThreadIdle::wait(worker).await;
        }

        // Child idle follows native result submission. A later acknowledgement on the root's
        // FIFO places both results in its passive mailbox before we arm automatic goal input.
        root.update_thread_settings(ThreadSettingsOverrides::default())
            .await
            .expect("acknowledge both accepted child results");
        fixture.set_status(ThreadGoalStatus::Active, true).await;
        fixture.queue_automatic_goal().await;
        for (id, _, _, _) in &children {
            deliveries.assert_worker_delivery(*id, None);
        }
        let before = requests.requests();
        assert_eq!(before.len(), 4, "two root calls and one per worker");
        for (id, _, _, _) in &children {
            assert_eq!(
                before
                    .iter()
                    .filter(|r| r.header("thread-id") == Some(id.to_string()))
                    .count(),
                1,
                "worker model request"
            );
        }
        assert_eq!(
            before
                .iter()
                .filter(|r| r.header("thread-id") == Some(root_id.to_string()))
                .count(),
            2,
            "root model requests"
        );

        let tree = timeout(DEADLINE, manager.suspend_agent_tree(root))
            .await
            .expect("drain tree")
            .expect("all terminal results written")
            .expect("durable local tree");
        assert_eq!(
            tree,
            codex_core::SuspendedAgentTree {
                root_turn_id: None,
                wake_root_after: None,
                child_turns: HashMap::new()
            }
        );
        fixture.assert_queued_goal_retired().await;
        root.wait_until_terminated().await;
        let parent = saved(root).await;
        let root_turns: Vec<_> = parent
            .iter()
            .filter_map(|item| match item {
                RolloutItem::EventMsg(EventMsg::TurnStarted(turn)) => Some(&turn.turn_id),
                _ => None,
            })
            .collect();
        assert_eq!(root_turns, [&root_turn], "no automatic turn admitted");
        let results = agent_messages(&parent);
        assert_eq!(results.len(), WORKERS.len(), "one durable result per child");
        for (id, path, turn, worker) in children {
            worker.wait_until_terminated().await;
            deliveries.assert_worker_delivery(id, Some(root_id));
            let (_, answer) = WORKERS
                .iter()
                .find(|(name, _)| path.as_str() == format!("/root/{name}"))
                .expect("named worker");
            let result: Vec<_> = results
                .iter()
                .filter(|(from, _, _)| *from == path.as_str())
                .collect();
            let [(_, to, text)] = result[..] else {
                panic!("expected exactly one result from {path}: {result:?}")
            };
            assert_eq!(*to, "/root");
            assert!(text.contains(*answer), "{text}");
            let child = saved(&worker).await;
            let completed: Vec<_> = child
                .iter()
                .filter_map(|item| match item {
                    RolloutItem::EventMsg(EventMsg::TurnComplete(event)) => {
                        Some((&event.turn_id, event.last_agent_message.as_deref()))
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(
                completed,
                [(&turn, Some(*answer))],
                "original worker's own terminal result"
            );
            for (_, other) in WORKERS.iter().filter(|(_, other)| other != answer) {
                assert!(
                    !text.contains(*other),
                    "sibling result attributed to {path}"
                );
                assert!(
                    !serde_json::to_string(&child)
                        .expect("saved child")
                        .contains(*other),
                    "sibling answer leaked into {path}"
                );
            }
        }
        assert_eq!(
            requests.requests().len(),
            before.len(),
            "queued goal sampled after terminal drain"
        );
    });
}
