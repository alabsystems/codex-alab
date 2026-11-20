// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::ShellBindings;
use uuid::Uuid;

#[test]
fn bare_terminal_number_loses_authority_on_reissue_or_history_import() {
    let number = 2_318;
    let first = Uuid::from_u128(1);
    let second = Uuid::from_u128(2);
    let mut bindings = ShellBindings::default();
    assert!(!bindings.accepts(number, None, Some(first)));
    bindings.record_issue(number);
    assert!(bindings.accepts(number, None, Some(first)));
    assert!(bindings.accepts(number, Some(first), Some(first)));
    assert!(!bindings.accepts(number, Some(second), Some(first)));
    assert!(!bindings.accepts(number, None, None));
    bindings.record_issue(number);
    assert!(!bindings.accepts(number, None, Some(second)));
    assert!(!bindings.accepts(number, Some(first), Some(second)));
    assert!(bindings.accepts(number, Some(second), Some(second)));
    bindings.record_issue(number + 1);
    assert!(bindings.accepts(number + 1, None, Some(first)));
    bindings.import_history();
    assert!(!bindings.accepts(number + 1, None, Some(first)));
    assert!(bindings.accepts(number + 1, Some(first), Some(first)));
    assert!(!bindings.accepts(number + 1, Some(first), None));
}

#[test]
fn live_command_snapshot_refreshes_the_writer_even_when_the_number_and_status_match() {
    use crate::context::world_state::CommandSession;
    use crate::context::world_state::CommandSessionStatus;
    use crate::context::world_state::CommandSessionsState;
    use crate::context::world_state::PreviousSectionState;
    use crate::context::world_state::WorldStateSection;
    use codex_protocol::models::ContentItem;

    let number = 2_318;
    let previous_writer = Uuid::from_u128(1);
    let current_writer = Uuid::from_u128(2);
    let snapshot = |invocation_id| {
        CommandSessionsState::new(std::iter::once(CommandSession {
            session_id: number,
            invocation_id,
            status: CommandSessionStatus::Running,
            in_use: false,
        }))
    };
    let vanilla = snapshot(None);
    let vanilla_wire = serde_json::to_value(&vanilla).expect("serialize vanilla snapshot");
    assert!(
        vanilla_wire["sessions"]["2318"]
            .get("invocation_id")
            .is_none()
    );
    assert_eq!(
        serde_json::from_value::<CommandSessionsState>(vanilla_wire)
            .expect("restore older snapshot"),
        vanilla,
    );

    let previous = snapshot(Some(previous_writer));
    let current = snapshot(Some(current_writer));
    for baseline in [
        PreviousSectionState::Absent,
        PreviousSectionState::Known(&previous),
    ] {
        let (updated, mut updates) = current.render_diff(baseline);
        assert_eq!(updated.as_ref(), Some(&current));
        assert_eq!(updates.len(), 1);
        let fragment = updates
            .pop()
            .expect("a changed command has a snapshot update")
            .content
            .into_fragment()
            .expect("command sessions update as a fragment");
        let ContentItem::InputText { text } = fragment.body() else {
            panic!("expected command-session text");
        };
        assert!(text.contains(&format!(
            "{number}: running; invocation_id={current_writer}"
        )));
        assert!(!text.contains(&previous_writer.to_string()));
    }
    let (_, unchanged) = current.render_diff(PreviousSectionState::Known(&current));
    assert!(unchanged.is_empty());
}
