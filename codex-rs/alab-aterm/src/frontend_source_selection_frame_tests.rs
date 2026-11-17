// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Real byte/SCM framing only; no Native selection, SDK or UI positive grant.
use super::*;
#[tokio::test]
async fn source_frames_keep_distinct_s1_s2_and_all_actual_fields() -> io::Result<()> {
    for tag in [b"S1", b"S2"] {
        let (receiver, mut sender) = pair()?;
        let bytes = wire(tag, &[OPERATION, FRONTIER, NONCE, ATTACHMENT]);
        let (received, written) = tokio::join!(
            bounded(receive(&receiver)),
            bounded(fragmented(&mut sender, &bytes))
        );
        written?;
        let packet = match (tag, received?) {
            (b"S1", Frame::SourceInhibit(packet)) | (b"S2", Frame::SourceSelection(packet)) => {
                packet
            }
            _ => panic!("wrong Source physical kind"),
        };
        assert_eq!(packet.parent, OPERATION);
        assert_eq!(packet.operation, FRONTIER);
        assert_eq!(packet.nonce, NONCE);
        assert_eq!(packet.attachment, ATTACHMENT);
    }
    Ok(())
}
#[tokio::test]
async fn every_source_head_version_and_body_field_refuses_and_closes_rights() -> io::Result<()> {
    for tag in [b"S1", b"S2"] {
        let bytes = wire(tag, &[OPERATION, FRONTIER, NONCE, ATTACHMENT]);
        for offset in [0, 1, 2, 34, 66, 98] {
            let (receiver, mut sender) = pair()?;
            let (descriptor, peer) = pair()?;
            if offset > 0 {
                sender.write_all(&bytes[..offset]).await?;
            }
            let mut receiving = Box::pin(receive(&receiver));
            if offset > 0 {
                assert!(
                    tokio::time::timeout(Duration::from_millis(10), &mut receiving)
                        .await
                        .is_err()
                );
            }
            rights(
                &sender,
                &bytes[offset..offset + 1],
                &[descriptor.as_raw_fd()],
            )
            .await?;
            drop(descriptor);
            assert!(bounded(receiving).await.is_err());
            assert_peer_closed(peer).await?;
        }
    }
    Ok(())
}
#[tokio::test]
async fn source_frames_fail_closed_at_all_truncations_and_noncanonical_fields() -> io::Result<()> {
    for tag in [b"S1", b"S2"] {
        let bytes = wire(tag, &[OPERATION, FRONTIER, NONCE, ATTACHMENT]);
        for end in [1, 2, 33, 34, 65, 66, 97, 98, 129] {
            let (receiver, mut sender) = pair()?;
            sender.write_all(&bytes[..end]).await?;
            sender.shutdown().await?;
            assert!(bounded(receive(&receiver)).await.is_err());
        }
        for offset in [2, 34, 66, 98] {
            let mut bad = bytes.clone();
            bad[offset] = b'G';
            let (receiver, mut sender) = pair()?;
            sender.write_all(&bad).await?;
            assert!(bounded(receive(&receiver)).await.is_err());
        }
    }
    for bad in [b"S0", b"S3"] {
        let (receiver, mut sender) = pair()?;
        sender.write_all(bad).await?;
        assert!(bounded(receive(&receiver)).await.is_err());
    }
    Ok(())
}
