// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::tasks::SessionTask;
use crate::tasks::SessionTaskResult;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::sync::Notify;

struct CountTask(Arc<AtomicUsize>, Arc<Notify>);
impl SessionTask for CountTask {
    fn kind(&self) -> TaskKind {
        TaskKind::Regular
    }
    fn span_name(&self) -> &'static str {
        "exec_launch_test"
    }
    async fn run(
        self: Arc<Self>,
        _session: Arc<Session>,
        _context: Arc<TurnContext>,
        _input: Vec<crate::session::input_queue::TurnInput>,
        _cancelled: CancellationToken,
    ) -> SessionTaskResult {
        self.0.fetch_add(1, Ordering::SeqCst);
        self.1.notify_one();
        Ok(None)
    }
}

#[tokio::test]
async fn fresh_idle_task_body_waits_even_after_original_members_are_ready() {
    let (root, root_turn) = fixture().await;
    let (idle, idle_turn) = fixture().await;
    let gate = barrier(&[(&root, &root_turn)]);
    idle.services.thread_extension_data.insert(IdleLaunch {
        barrier: gate.clone(),
        session: Arc::downgrade(&idle),
    });
    root_turn
        .extension_data
        .insert(gate.permit(root.thread_id).unwrap());
    root.spawn_task(
        root_turn,
        Vec::new(),
        CountTask(Arc::new(AtomicUsize::new(0)), Arc::new(Notify::new())),
    )
    .await;
    gate.wait_ready().await.unwrap();
    let ran = Arc::new(AtomicUsize::new(0));
    let task: Arc<dyn crate::tasks::AnySessionTask> =
        Arc::new(CountTask(ran.clone(), Arc::new(Notify::new())));
    let body = task.run_with_exec_launch(idle, idle_turn, Vec::new(), CancellationToken::new());
    tokio::pin!(body);
    assert!(futures::poll!(body.as_mut()).is_pending());
    assert_eq!(ran.load(Ordering::SeqCst), 0);
    gate.release().unwrap();
    body.await.unwrap();
    assert_eq!(ran.load(Ordering::SeqCst), 1);
}

async fn fixture() -> (Arc<Session>, Arc<TurnContext>) {
    let (session, context) = crate::session::tests::make_session_and_context().await;
    (Arc::new(session), Arc::new(context))
}

fn barrier(threads: &[(&Arc<Session>, &Arc<TurnContext>)]) -> ExecLaunchBarrier {
    ExecLaunchBarrier::from_members(
        threads
            .iter()
            .map(|(session, context)| {
                (
                    session.thread_id,
                    Participant {
                        kind: TaskKind::Regular,
                        turn: context.sub_id.clone(),
                        session: Arc::downgrade(session),
                    },
                )
            })
            .collect(),
    )
    .unwrap()
}

async fn registered(gate: &ExecLaunchBarrier, expected: usize) {
    let mut changes = gate.0.status.subscribe();
    tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            if gate.0.arrived.lock().unwrap().len() >= expected {
                return;
            }
            changes.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn no_task_body_runs_until_every_member_is_registered() {
    let (root, root_turn) = fixture().await;
    let (child, child_turn) = fixture().await;
    let gate = barrier(&[(&root, &root_turn), (&child, &child_turn)]);
    root_turn
        .extension_data
        .insert(gate.permit(root.thread_id).unwrap());
    child_turn
        .extension_data
        .insert(gate.permit(child.thread_id).unwrap());
    let ran = Arc::new(AtomicUsize::new(0));
    let changed = Arc::new(Notify::new());
    assert!(gate.release().is_err());
    root.spawn_task(
        root_turn,
        Vec::new(),
        CountTask(ran.clone(), changed.clone()),
    )
    .await;
    registered(&gate, 1).await;
    assert_eq!(ran.load(Ordering::SeqCst), 0);
    child
        .spawn_task(
            child_turn,
            Vec::new(),
            CountTask(ran.clone(), changed.clone()),
        )
        .await;
    tokio::time::timeout(Duration::from_secs(60), gate.wait_ready())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ran.load(Ordering::SeqCst), 0, "readiness is not release");
    gate.release().unwrap();
    gate.release().unwrap();
    tokio::time::timeout(Duration::from_secs(60), async {
        while ran.load(Ordering::SeqCst) != 2 {
            changed.notified().await;
        }
    })
    .await
    .unwrap();
    assert!(!gate.abort("too late".into()));
}

#[tokio::test]
async fn aborting_pending_or_ready_launch_never_runs_a_task_body() {
    for all_registered in [false, true] {
        let (root, root_turn) = fixture().await;
        let (child, child_turn) = fixture().await;
        let gate = barrier(&[(&root, &root_turn), (&child, &child_turn)]);
        root_turn
            .extension_data
            .insert(gate.permit(root.thread_id).unwrap());
        let ran = Arc::new(AtomicUsize::new(0));
        root.spawn_task(
            root_turn,
            Vec::new(),
            CountTask(ran.clone(), Arc::new(Notify::new())),
        )
        .await;
        registered(&gate, 1).await;
        if all_registered {
            child_turn
                .extension_data
                .insert(gate.permit(child.thread_id).unwrap());
            child
                .spawn_task(
                    child_turn,
                    Vec::new(),
                    CountTask(ran.clone(), Arc::new(Notify::new())),
                )
                .await;
            gate.wait_ready().await.unwrap();
        }
        let done = root
            .active_turn
            .lock()
            .await
            .as_ref()
            .unwrap()
            .task
            .as_ref()
            .unwrap()
            .done
            .clone();
        let finished = done.notified();
        tokio::pin!(finished);
        finished.as_mut().enable();
        assert!(gate.abort("native admission refused".into()));
        tokio::time::timeout(Duration::from_secs(60), finished)
            .await
            .unwrap();
        assert!(gate.wait_ready().await.is_err());
        assert_eq!(ran.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn a_permit_cannot_launch_another_runtime() {
    let (root, root_turn) = fixture().await;
    let (mut other, mut other_turn) = fixture().await;
    Arc::get_mut(&mut other).unwrap().thread_id = root.thread_id;
    Arc::get_mut(&mut other_turn).unwrap().sub_id = root_turn.sub_id.clone();
    let gate = barrier(&[(&root, &root_turn)]);
    other_turn
        .extension_data
        .insert(gate.permit(root.thread_id).unwrap());
    let ran = Arc::new(AtomicUsize::new(0));
    other
        .spawn_task(
            other_turn,
            Vec::new(),
            CountTask(ran.clone(), Arc::new(Notify::new())),
        )
        .await;
    assert!(
        tokio::time::timeout(Duration::from_secs(60), gate.wait_ready())
            .await
            .unwrap()
            .is_err()
    );
    assert_eq!(ran.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn review_launch_rejects_a_registered_regular_task() {
    let (session, context) = fixture().await;
    let mut gate = barrier(&[(&session, &context)]);
    Arc::get_mut(&mut gate.0)
        .unwrap()
        .members
        .get_mut(&session.thread_id)
        .unwrap()
        .kind = TaskKind::Review;
    context
        .extension_data
        .insert(gate.permit(session.thread_id).unwrap());
    let ran = Arc::new(AtomicUsize::new(0));
    session
        .spawn_task(
            context,
            Vec::new(),
            CountTask(Arc::clone(&ran), Arc::new(Notify::new())),
        )
        .await;
    assert!(
        tokio::time::timeout(Duration::from_secs(5), gate.wait_ready())
            .await
            .unwrap()
            .is_err()
    );
    assert_eq!(ran.load(Ordering::SeqCst), 0);
}
