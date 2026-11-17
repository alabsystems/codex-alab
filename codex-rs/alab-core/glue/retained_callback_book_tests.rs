// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::*;
#[test]
fn retired_intervals_remain_bounded_behind_an_owned_hole() -> Result<(), &'static str> {
    let mut book = Book::default();
    book.insert(1, "original".into(), "unknown owner");
    for id in 2..10002 {
        let next = book.admit(2)?;
        assert_eq!(next, id);
        book.insert(id, id.to_string(), "normal terminal");
        assert_eq!(book.retire(id, &id.to_string()), Some("normal terminal"));
        assert!(book.retired.contains(id));
        assert!(book.retired.ranges.len() <= 1);
    }
    assert!(!book.retired.contains(1));
    assert_eq!(book.active.get(&1), Some(&"unknown owner"));
    assert_eq!(book.retire(1, "original"), Some("unknown owner"));
    assert_eq!(book.retired.floor, 10001);
    assert!(book.retired.ranges.is_empty());
    assert_eq!(book.admit(2)?, 10002);
    Ok(())
}
#[test]
fn capacity_refusal_does_not_forget_unknown_or_reissue_ordinal() {
    let mut book = Book::default();
    book.insert(1, "one".into(), "unknown");
    book.insert(2, "two".into(), "pending");
    assert!(book.admit(2).is_err());
    assert_eq!(book.next, 2);
    assert_eq!(book.active.len(), 2);
    assert!(!book.retired.contains(1));
    assert_eq!(book.retire(2, "two"), Some("pending"));
    assert_eq!(book.retire(2, "two"), None);
    assert_eq!(book.admit(2), Ok(3));
    assert_eq!(book.active.get(&1), Some(&"unknown"));
}
#[test]
fn out_of_order_retirement_never_declares_an_active_gap_retired() {
    let mut retired = Retired::default();
    for id in [5, 3, 7, 6, 2, 4] {
        retired.insert(id);
        retired.insert(id);
        assert!(!retired.contains(1));
        assert!(!retired.contains(8));
    }
    assert_eq!(retired.ranges.get(&2), Some(&7));
    retired.insert(1);
    assert_eq!(retired.floor, 7);
    assert!(retired.ranges.is_empty());
}

#[tokio::test]
async fn retirement_after_subscribe_before_poll_is_not_lost() -> Result<(), &'static str> {
    let mut book = Book::default();
    book.insert(1, "one".into(), ());
    let mut observed = book.subscribe();
    assert_eq!(book.retire(1, "one"), Some(()));
    tokio::time::timeout(std::time::Duration::from_secs(60), observed.changed())
        .await
        .map_err(|_| "retirement notice was lost")?
        .map_err(|_| "retirement sender lost")?;
    assert!(book.retired.contains(1));
    Ok(())
}
