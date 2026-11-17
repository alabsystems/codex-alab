// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use tokio::io::AsyncWriteExt as _;

#[tokio::test]
async fn only_actual_socket_activity_or_closure_withdraws_owner() {
    for send_byte in [false, true] {
        let (reader, mut writer) = tokio::net::UnixStream::pair().unwrap();
        let observed = observe(&reader);
        tokio::pin!(observed);
        tokio::select! {
            biased;
            () = &mut observed => panic!("live silent owner was withdrawn"),
            () = std::future::ready(()) => {}
        }
        if send_byte {
            writer.write_all(b"x").await.unwrap();
        } else {
            drop(writer);
        }
        tokio::time::timeout(std::time::Duration::from_secs(1), observed)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn withdrawal_is_sticky_for_later_shutdown_observers() {
    let (sender, receiver) = watch::channel(false);
    sender.send_replace(true);
    wait(receiver.clone()).await;
    wait(receiver).await;
}

#[tokio::test]
async fn failed_owner_monitor_also_withdraws() {
    let (sender, receiver) = watch::channel(false);
    drop(sender);
    wait(receiver).await;
}

#[tokio::test]
async fn source_writes_do_not_withdraw_the_native_owner() {
    use tokio::io::AsyncReadExt as _;
    let (source, mut native) = tokio::net::UnixStream::pair().unwrap();
    let observed = observe(&source);
    tokio::pin!(observed);
    source.writable().await.unwrap();
    assert_eq!(source.try_write(b"tool-io").unwrap(), 7);
    let mut received = [0; 7];
    native.read_exact(&mut received).await.unwrap();
    assert_eq!(&received, b"tool-io");
    tokio::select! {
        biased;
        () = &mut observed => panic!("outgoing IO withdrew the owner"),
        () = std::future::ready(()) => {}
    }
    drop(native);
    tokio::time::timeout(std::time::Duration::from_secs(1), observed)
        .await
        .unwrap();
}
