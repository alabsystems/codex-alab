// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use codex_app_server_protocol::ThreadItem;

fn turn(id: &str, item_ids: &[&str]) -> Turn {
    Turn {
        id: id.to_owned(),
        root_turn_id: None,
        items: item_ids
            .iter()
            .map(|id| ThreadItem::untimed(payload(id)))
            .collect(),
        items_view: TurnItemsView::Full,
        status: TurnStatus::Completed,
        error: None,
        started_at: None,
        completed_at: None,
        duration_ms: None,
    }
}

fn payload(id: &str) -> ThreadItemPayload {
    ThreadItemPayload::UserMessage {
        id: id.to_owned(),
        client_id: None,
        content: Vec::new(),
    }
}

fn item(turn_id: &str, id: &str) -> HistoryPageItem {
    HistoryPageItem {
        turn_id: turn_id.to_owned(),
        payload: payload(id),
    }
}

fn interrupted(id: &str) -> Turn {
    Turn {
        status: TurnStatus::Interrupted,
        ..turn(id, &[])
    }
}

fn assert_groups(items: &[HistoryPageItem], turns: &[Turn], expected: &[(usize, Option<&str>)]) {
    let groups = group_turn_items(items, turns);
    assert_eq!(
        groups
            .iter()
            .map(|(items, turn)| (items.len(), turn.map(|turn| turn.id.as_str())))
            .collect::<Vec<_>>(),
        expected,
    );
    assert_eq!(
        groups
            .iter()
            .flat_map(|(items, _)| items.iter())
            .collect::<Vec<_>>(),
        items.iter().collect::<Vec<_>>(),
        "every item must be rendered exactly once and in page order",
    );
}

#[test]
fn reversed_turn_endings_reproduce_the_76_to_38_crash() {
    let ids = (0..76).map(|i| format!("item-{i}")).collect::<Vec<_>>();
    let early = ids.iter().take(38).map(String::as_str).collect::<Vec<_>>();
    let late = ids.iter().skip(38).map(String::as_str).collect::<Vec<_>>();
    let turns = [turn("late", &late), turn("early", &early)];
    let items = early
        .iter()
        .map(|id| item("early", id))
        .chain(late.iter().map(|id| item("late", id)))
        .collect::<Vec<_>>();
    assert_groups(&items, &turns, &[(38, Some("early")), (38, Some("late"))]);
}

#[test]
fn reused_item_ids_do_not_complete_another_turn() {
    let turns = [turn("first", &["same"]), turn("second", &["same"])];
    assert_groups(
        &[item("first", "same"), item("second", "same")],
        &turns,
        &[(1, Some("first")), (1, Some("second"))],
    );
    assert_groups(&[item("second", "same")], &turns, &[(1, Some("second"))]);
}

#[test]
fn interleaved_turns_complete_in_item_order_with_their_empty_interruptions() {
    let turns = [
        turn("first", &["a", "d"]),
        interrupted("after first"),
        turn("second", &["b", "c"]),
        interrupted("after second"),
    ];
    assert_groups(
        &[
            item("first", "a"),
            item("second", "b"),
            item("second", "c"),
            item("first", "d"),
            item("unknown", "trailing"),
        ],
        &turns,
        &[
            (3, Some("second")),
            (0, Some("after second")),
            (1, Some("first")),
            (0, Some("after first")),
            (1, None),
        ],
    );
}

#[test]
fn interruptions_are_owned_only_by_the_page_with_their_anchor() {
    let mut unloaded = interrupted("not loaded");
    unloaded.items_view = TurnItemsView::NotLoaded;
    let turns = [
        interrupted("leading"),
        turn("first", &["a", "last"]),
        turn("empty completed", &[]),
        unloaded,
        interrupted("following"),
        interrupted("following again"),
        turn("second", &["a"]),
    ];
    assert_groups(
        &[item("first", "a")],
        &turns,
        &[(0, Some("leading")), (1, None)],
    );
    assert_groups(
        &[item("first", "last")],
        &turns,
        &[
            (1, Some("first")),
            (0, Some("following")),
            (0, Some("following again")),
        ],
    );
    assert_groups(&[item("second", "a")], &turns, &[(1, Some("second"))]);
    assert_groups(&[], &turns, &[]);
    assert_groups(&[], &[interrupted("no items")], &[]);
}

#[test]
fn metadata_permutations_and_partial_pages_preserve_exact_coverage() {
    let turns = [
        turn("a", &["shared", "last"]),
        turn("b", &["shared", "last"]),
        turn("c", &["shared", "last"]),
    ];
    let items = [
        item("a", "shared"),
        item("b", "shared"),
        item("c", "shared"),
        item("c", "last"),
        item("b", "last"),
        item("a", "last"),
    ];
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let metadata = order.map(|index| turns[index].clone());
        for size in 1..=items.len() {
            for page in items.windows(size) {
                let expected = page
                    .split_inclusive(|item| item.payload.id() == "last")
                    .map(|group| {
                        let last = group.last().expect("nonempty split");
                        (
                            group.len(),
                            (last.payload.id() == "last").then_some(last.turn_id.as_str()),
                        )
                    })
                    .collect::<Vec<_>>();
                assert_groups(page, &metadata, &expected);
            }
        }
    }
}
