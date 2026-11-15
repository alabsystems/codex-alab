//! Pagination boundaries preserve grouped item rendering and attach completion metadata once.

use crate::app_server_session::history_page::test_support::{group_ranges, page_items}; // ALAB(history): keep the original assertions with ALab-owned adapters.
use crate::app_server_session::history_page::HistoryPageItem; // ALAB(history): preserve scoped identities in pagination regressions.
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadItemPayload; // ALAB(history): the helper is now ALab-owned.
use codex_app_server_protocol::Turn; // ALAB(history): the helper is now ALab-owned.
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStatus;
use pretty_assertions::assert_eq;

fn payloads(turn_id: &str, items: &[codex_app_server_protocol::ThreadItem]) -> Vec<HistoryPageItem> { // ALAB(history): IDs are scoped by turn.
    page_items(turn_id, items) // ALAB(history): retain the page's owning turn.
}

fn turn(id: &str, status: TurnStatus, item_ids: &[&str]) -> Turn {
    Turn {
        id: id.to_string(),
        root_turn_id: None,
        items: item_ids
            .iter()
            .map(|id| ThreadItemPayload::UserMessage {
                id: id.to_string(),
                client_id: None,
                content: Vec::new(),
            })
            .map(ThreadItem::untimed)
            .collect(),
        items_view: TurnItemsView::Summary,
        status,
        error: None,
        started_at: None,
        completed_at: Some(1_700_000_000),
        duration_ms: Some(125_000),
    }
}

#[test]
fn split_turn_only_completes_on_the_page_with_its_last_item() {
    let turns = vec![turn(
        "turn",
        TurnStatus::Completed,
        &["first", "second", "last"],
    )];
    assert_eq!(
        group_ranges(&payloads(&turns[0].id, &turns[0].items[1..]), &turns), // ALAB(history): scoped page identities.
        vec![(0..2, Some(&turns[0]))],
    );
    assert_eq!(
        group_ranges(&payloads(&turns[0].id, &turns[0].items[..1]), &turns), // ALAB(history): scoped page identities.
        vec![(0..1, None)],
    );
}

#[test]
fn multiple_turns_keep_item_groups_and_completion_order() {
    let turns = vec![
        turn("first", TurnStatus::Completed, &["a", "b"]),
        turn("second", TurnStatus::Completed, &["c", "d"]),
    ];
    let items = turns
        .iter()
        .flat_map(|turn| payloads(&turn.id, &turn.items)) // ALAB(history): scoped page identities.
        .collect::<Vec<_>>();
    assert_eq!(
        group_ranges(&items, &turns), // ALAB(history): compare borrowed groups by their coverage.
        vec![(0..2, Some(&turns[0])), (2..4, Some(&turns[1])),],
    );
}

#[test]
fn itemless_interruptions_keep_their_position_on_the_owning_page() {
    let turns = [
        turn("empty first", TurnStatus::Interrupted, &[]),
        turn("failed", TurnStatus::Failed, &["a"]),
        turn("empty between", TurnStatus::Interrupted, &[]),
        turn("running", TurnStatus::InProgress, &["b"]),
    ];
    assert_eq!(
        group_ranges(&payloads(&turns[1].id, &turns[1].items), &turns), // ALAB(history): scoped page identities.
        vec![
            (0..0, Some(&turns[0])),
            (0..1, Some(&turns[1])),
            (1..1, Some(&turns[2]))
        ],
    );
    assert_eq!(
        group_ranges(&payloads(&turns[3].id, &turns[3].items), &turns), // ALAB(history): scoped page identities.
        vec![(0..1, Some(&turns[3]))],
    );
}
