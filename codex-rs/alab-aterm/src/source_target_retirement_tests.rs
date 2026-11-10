// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Private transport controls, not proof of native OPEN or provider authority.

use super::*;
use codex_alab_core::native_target::Destination;

#[tokio::test]
async fn target_open_retirement_does_not_infer_closed_from_disconnect() -> io::Result<()> {
    let fixture = Fixture::start_with(|_, _| {}, Some("DROP-OPEN-CLOSE")).await?;
    let target = target::context_at(&fixture.endpoint, &"e".repeat(32), BIRTH).await?;
    assert!(
        target
            .retire_open(&"a".repeat(32), &"b".repeat(32), None)
            .await
            .is_err()
    );
    let requests = fixture.requests.lock().unwrap().clone();
    assert_eq!(
        requests
            .iter()
            .filter(|line| line.contains(" member-target-open-close "))
            .count(),
        1
    );
    assert!(!requests.iter().any(|line| line.contains(" member-retire ")));
    drop(target);
    fixture.finish().await;
    Ok(())
}

#[tokio::test]
async fn target_open_retirement_does_not_accept_another_known_member() -> io::Result<()> {
    let fixture = Fixture::start(|action, frame| {
        if action == "member-target-open-close" {
            frame["member"] = json!("3".repeat(32));
        }
    })
    .await?;
    let target = target::context_at(&fixture.endpoint, &"e".repeat(32), BIRTH).await?;
    assert!(
        target
            .retire_open(&"a".repeat(32), &"b".repeat(32), Some(MEMBER))
            .await
            .is_err()
    );
    drop(target);
    fixture.finish().await;
    Ok(())
}

#[tokio::test]
async fn target_open_retirement_checks_exact_private_closed_reply() -> io::Result<()> {
    for member in [None, Some(MEMBER)] {
        let fixture = Fixture::start(|_, _| {}).await?;
        let target = target::context_at(&fixture.endpoint, &"e".repeat(32), BIRTH).await?;
        let receipt = target
            .retire_open(&"a".repeat(32), &"b".repeat(32), member)
            .await?;
        assert_eq!(receipt.assignment(), "e".repeat(32));
        assert_eq!(receipt.operation(), "a".repeat(32));
        assert_eq!(receipt.open_operation(), "b".repeat(32));
        assert_eq!(receipt.member(), member);
        assert_eq!(format!("{receipt:?}"), "NativeOpenRetirement(<private>)");
        let requests = fixture.requests.lock().unwrap().clone();
        let request = requests
            .iter()
            .find(|line| line.contains(" member-target-open-close "))
            .unwrap();
        assert!(request.contains(&format!(
            " assignment={} context={}",
            "e".repeat(32),
            "c".repeat(64)
        )));
        assert!(request.contains(&format!(" open_operation={}", "b".repeat(32))));
        drop(target);
        fixture.finish().await;
    }
    Ok(())
}

#[tokio::test]
async fn target_open_retirement_refuses_unclosed_or_mismatched_receipts() -> io::Result<()> {
    let mutants: [fn(&str, &mut Value); 6] = [
        |action, value| {
            if action == "member-target-open-close" {
                value["state"] = json!("waiting");
            }
        },
        |action, value| {
            if action == "member-target-open-close" {
                value["open_operation"] = json!("f".repeat(32));
            }
        },
        |action, value| {
            if action == "member-target-open-close" {
                value["assignment"] = json!("f".repeat(32));
            }
        },
        |action, value| {
            if action == "member-target-open-close" {
                value["context"] = json!("f".repeat(64));
            }
        },
        |action, value| {
            if action == "member-target-open-close" {
                value["member"] = json!(MEMBER);
            }
        },
        |action, value| {
            if action == "member-target-open-close" {
                value["session"]["epoch"] = json!("replacement-native-instance");
            }
        },
    ];
    for mutate in mutants {
        let fixture = Fixture::start(mutate).await?;
        let target = target::context_at(&fixture.endpoint, &"e".repeat(32), BIRTH).await?;
        assert!(
            target
                .retire_open(&"a".repeat(32), &"b".repeat(32), None)
                .await
                .is_err()
        );
        drop(target);
        fixture.finish().await;
    }
    Ok(())
}
