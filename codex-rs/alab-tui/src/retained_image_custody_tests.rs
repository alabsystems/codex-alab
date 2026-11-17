// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Real descriptors/link validation only. These controls mint no Entered, Native
//! command, public claim, reconstructed unlink owner or UI/Source completion.
use super::*;
use std::io::Seek;
const NONCE: [u8; 32] = [23; 32];
fn original() -> io::Result<Arc<OriginalFile>> {
    let mut file = NamedTempFile::new()?;
    file.write_all(b"same original PNG owner")?;
    file.flush()?;
    Ok(OriginalFile::retain_native(file))
}
#[test]
fn actual_mailbox_receives_same_inode_and_eof_without_taking_original_unlink_owner()
-> io::Result<()> {
    let source = original()?;
    let link = source.with_native(Link::capture)?;
    let socket = prepare_channel(std::slice::from_ref(&source), NONCE)?;
    let mut progress = Progress::default();
    let paths = receive_descriptors(
        &socket,
        &mut progress,
        &Bundle {
            nonce: NONCE,
            files: vec![link.clone()],
        },
    )?;
    assert_eq!(paths, vec![link.path()?]);
    assert_eq!(progress.raw.len(), 1);
    let fd = progress.raw[0]
        .as_ref()
        .ok_or_else(|| invalid("lower actual received fd missing"))?;
    let mut copied = File::from(fd.try_clone()?);
    copied.rewind()?;
    let mut bytes = Vec::new();
    copied.read_to_end(&mut bytes)?;
    assert_eq!(bytes, b"same original PNG owner");
    drop(progress);
    drop(socket); // no reconstruction; original unlink owner is live.
    source.with_native(|file| {
        assert!(file.path().exists());
        Ok(())
    })?;
    Ok(())
}
#[test]
fn actual_received_rights_are_retained_on_link_refusal_and_source_owner_stays_live()
-> io::Result<()> {
    let source = original()?;
    let mut link = source.with_native(Link::capture)?;
    let socket = prepare_channel(std::slice::from_ref(&source), NONCE)?;
    link.inode = link.inode.wrapping_add(1);
    let mut progress = Progress::default();
    assert!(
        receive_descriptors(
            &socket,
            &mut progress,
            &Bundle {
                nonce: NONCE,
                files: vec![link]
            }
        )
        .is_err()
    );
    assert_eq!(progress.raw.len(), 1);
    assert!(progress.raw[0].is_some());
    source.with_native(|file| {
        assert!(file.path().exists());
        Ok(())
    })?;
    Ok(())
}
#[test]
fn original_relative_named_link_is_normalized_only_from_its_actual_file() -> io::Result<()> {
    let directory = tempfile::tempdir()?;
    let (_, module) = module_path!()
        .split_once("::")
        .ok_or_else(|| invalid("relative-file child has no test module"))?;
    let result = std::process::Command::new(std::env::current_exe()?)
        .arg("--exact")
        .arg(format!("{module}::actual_relative_named_file_child"))
        .arg("--nocapture")
        .env("ALAB_TEST_IMAGE_RELATIVE_FILE_CHILD", "1")
        .current_dir(directory.path())
        .output()?;
    if !result.status.success()
        || !String::from_utf8_lossy(&result.stdout).contains("ALAB_IMAGE_RELATIVE_FILE_VERIFIED")
    {
        return Err(invalid(format!(
            "original relative-file child did not verify its native file: stdout={} stderr={}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        )));
    }
    Ok(())
}
#[test]
fn actual_relative_named_file_child() -> io::Result<()> {
    if std::env::var_os("ALAB_TEST_IMAGE_RELATIVE_FILE_CHILD").is_none() {
        return Ok(());
    }
    let file = NamedTempFile::new_in(".")?;
    let link = Link::capture(&file)?;
    assert!(link.path()?.is_absolute());
    assert_eq!(
        std::fs::symlink_metadata(link.path()?)?.ino(),
        file.as_file().metadata()?.ino()
    );
    println!("ALAB_IMAGE_RELATIVE_FILE_VERIFIED");
    Ok(())
}
#[test]
fn wrong_actual_nonce_denies_before_descriptor_or_owner_reconstruction() -> io::Result<()> {
    let source = original()?;
    let link = source.with_native(Link::capture)?;
    let socket = prepare_channel(std::slice::from_ref(&source), NONCE)?;
    let mut progress = Progress::default();
    assert!(
        receive_descriptors(
            &socket,
            &mut progress,
            &Bundle {
                nonce: [24; 32],
                files: vec![link]
            }
        )
        .is_err()
    );
    assert!(progress.raw.is_empty());
    assert!(progress.owners.is_none());
    assert!(progress.keepalive.is_some());
    source.with_native(|file| {
        assert!(file.path().exists());
        Ok(())
    })?;
    Ok(())
}
#[test]
fn locator_requires_explicit_no_files_and_closed_supported_version() -> io::Result<()> {
    let offer = Offer::prepare(&[])?;
    let raw = serde_json::to_value(offer.locator_data()?).map_err(invalid)?;
    assert_eq!(raw, serde_json::json!({"schema":1,"bundle":null}));
    for wrong in [
        serde_json::json!({"schema":1}),
        serde_json::json!({"schema":1,"bundle":null,"other":true}),
    ] {
        assert!(serde_json::from_value::<LocatorData>(wrong).is_err());
    }
    let wrong: LocatorData =
        serde_json::from_value(serde_json::json!({"schema":2,"bundle":null})).map_err(invalid)?;
    assert!(wrong.validate_data().is_err());
    for wrong in [
        serde_json::json!({"e1Version":2,"imageCustody":1,"clipboardCustody":1}),
        serde_json::json!({"e1Version":2,"imageCustody":1,"clipboardCustody":1,"preThreadPin":1,"other":true}),
    ] {
        assert!(serde_json::from_value::<Support>(wrong).is_err());
    }
    let wrong: Support = serde_json::from_value(
        serde_json::json!({"e1Version":2,"imageCustody":2,"clipboardCustody":1,"preThreadPin":1}),
    )
    .map_err(invalid)?;
    assert!(wrong.validate().is_err());
    Ok(())
}

#[test]
fn actual_scene_handoff_keeps_aliases_then_allows_only_real_last_owner_retirement() -> io::Result<()>
{
    let source = original()?;
    let path = source.with_native(|file| Ok(file.path().to_path_buf()))?;
    let mut table = FileTable::new(vec![source.clone()]);
    assert!(table.handoff(&[]).is_err());
    assert!(table.pending.is_some());
    let actual_scene = source.clone();
    let retired = table.handoff(std::slice::from_ref(&actual_scene))?;
    drop(retired);
    drop(source);
    assert!(Arc::ptr_eq(&table.file(FileRef(0))?, &actual_scene));
    assert!(path.exists());
    drop(actual_scene);
    assert!(!path.exists());
    assert!(table.file(FileRef(0)).is_err());
    assert!(table.pending.is_none());
    Ok(())
}

#[test]
fn actual_claim_lookahead_crosses_ancillary_marker_without_consuming_queued_rights()
-> io::Result<()> {
    let source = original()?;
    let link = source.with_native(Link::capture)?;
    let channel = prepare_channel(std::slice::from_ref(&source), NONCE)?;
    assert_eq!(peek_nonce(&channel)?, Some(NONCE));
    assert_eq!(peek_nonce(&channel)?, Some(NONCE));
    let mut state = Progress::default();
    let paths = receive_descriptors(
        &channel,
        &mut state,
        &Bundle {
            nonce: NONCE,
            files: vec![link.clone()],
        },
    )?;
    assert_eq!(paths, vec![link.path()?]);
    assert!(state.keepalive.is_some());
    assert_eq!(state.raw.len(), 1);
    assert!(state.raw[0].is_some());
    source.with_native(|file| {
        assert!(file.path().exists());
        Ok(())
    })?;
    Ok(())
}

#[test]
fn required_ui2_and_clipboard_grammar_rejects_image_only_and_old_target_support() -> io::Result<()>
{
    let valid: Support = serde_json::from_value(
        serde_json::json!({"e1Version":2,"imageCustody":1,"clipboardCustody":1,"preThreadPin":1}),
    )
    .map_err(invalid)?;
    valid.validate()?;
    for wrong in [
        serde_json::json!({"e1Version":1,"imageCustody":1,"preThreadPin":1}),
        serde_json::json!({"e1Version":2,"imageCustody":1,"preThreadPin":1}),
        serde_json::json!({"e1Version":1,"imageCustody":1,"clipboardCustody":1,"preThreadPin":1}),
        serde_json::json!({"e1Version":2,"imageCustody":1,"clipboardCustody":0,"preThreadPin":1}),
    ] {
        if let Ok(value) = serde_json::from_value::<Support>(wrong) {
            assert!(value.validate().is_err());
        }
    }
    let offer = Offer::prepare(&[])?;
    let mut frame = serde_json::json!({"version":2,"service":"actual-envelope-correlation-data","image_custody":offer.locator_data()?,"clipboard":{"schema":1}});
    let encode = |magic: &[u8], frame: &serde_json::Value| -> io::Result<Vec<u8>> {
        let mut bytes = magic.to_vec();
        bytes.extend(serde_json::to_vec(frame).map_err(invalid)?);
        Ok(bytes)
    };
    // Private grammar test only: the tiny opaque object is NOT ClipboardData
    // validation, a genuine Entry, a file claim or whole-App readiness.
    image_header(
        &encode(UI_MAGIC, &frame)?,
        "actual-envelope-correlation-data",
    )?;
    assert!(
        image_header(
            &encode(b"ALAB-RETAIN-UI1\n", &frame)?,
            "actual-envelope-correlation-data"
        )
        .is_err()
    );
    frame["version"] = serde_json::json!(1);
    assert!(
        image_header(
            &encode(UI_MAGIC, &frame)?,
            "actual-envelope-correlation-data"
        )
        .is_err()
    );
    frame["version"] = serde_json::json!(2);
    for value in [
        serde_json::Value::Null,
        serde_json::json!([]),
        serde_json::json!(false),
    ] {
        frame["clipboard"] = value;
        assert!(
            image_header(
                &encode(UI_MAGIC, &frame)?,
                "actual-envelope-correlation-data"
            )
            .is_err()
        );
    }
    frame
        .as_object_mut()
        .ok_or_else(|| invalid("private frame DATA missing"))?
        .remove("clipboard");
    assert!(
        image_header(
            &encode(UI_MAGIC, &frame)?,
            "actual-envelope-correlation-data"
        )
        .is_err()
    );
    Ok(())
}
