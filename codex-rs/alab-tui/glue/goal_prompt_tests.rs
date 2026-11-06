// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The goal box carries why the goal stopped (`ALAB(answer)`, CONTRACT §5 "goal box"): a real
//! `App` resuming a thread whose goal stopped opens "Resume paused goal?"
//! (`maybe_prompt_resume_paused_goal_after_resume`), and its hook tells codex-alab's shared
//! state why before the view opens, so the box the view reports carries it; the autopilot
//! leaves a blocked goal's box to a person by it.

use crate::app::tests::make_test_app_with_channels;
use codex_alab_tui::boxes;
use codex_alab_tui::boxes::BoxKind;
use codex_alab_tui::boxes::GoalStop;
use codex_app_server_protocol::ThreadGoalStatus;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn a_resume_over_a_stopped_goal_reports_the_goal_box_with_why_it_stopped()
-> color_eyre::eyre::Result<()> {
    for (status, stop) in [
        (ThreadGoalStatus::Paused, GoalStop::Paused),
        (ThreadGoalStatus::Blocked, GoalStop::Blocked),
        (ThreadGoalStatus::UsageLimited, GoalStop::UsageLimited),
    ] {
        let shared: &'static boxes::Shared = Box::leak(Box::new(boxes::Shared::new()));
        let _boxes = boxes::scoped(shared);
        let (mut app, _events, _ops) = make_test_app_with_channels().await;
        let mut server = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
        let started = server.start_thread(&app.config).await?;
        let thread_id = started.session.thread_id;
        app.enqueue_primary_thread_session(started.session, started.turns)
            .await?;
        server
            .thread_goal_set(
                thread_id,
                Some("Finish the stopped goal".to_owned()),
                Some(status),
                /*token_budget*/ None,
            )
            .await?;
        app.maybe_prompt_resume_paused_goal_after_resume(&mut server, thread_id)
            .await;
        app.chat_widget.pre_draw_tick();
        let open = shared
            .open_box()
            .unwrap_or_else(|| panic!("the goal box is reported over a {status:?} goal"));
        let BoxKind::Goal { stop: carried, .. } = open.kind else {
            panic!("a goal box: {open:?}");
        };
        assert_eq!(carried, Some(stop), "{status:?}");
        server.shutdown().await?;
    }
    Ok(())
}
