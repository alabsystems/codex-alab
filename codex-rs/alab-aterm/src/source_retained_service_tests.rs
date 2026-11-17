// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Startup-data codec checks. No fixture here issues native SDK/controller rights.
use super::*;
use std::os::unix::ffi::OsStringExt as _;
use tokio::io::AsyncReadExt as _;

#[test]
fn encoder_matches_the_native_decoder_raw_value_vector() {
    // Accepted by aterm's codex_retained_settings_tests.rs at native main
    // 2c528861b8593f3510e28dd9f4bf4d77da9600bc. Compare the complete physical
    // frame, not a round trip through another copy of the Source encoder.
    let native = b"ALABSET1\0\0\0\x17\0\0\0\x01\0\0\0\x01x\0\0\0\x01\0\0\0\x01K\0\0\0\x01\xff";
    let encoded = StartupSettings::new(
        &["x".into()],
        &[("K".into(), OsString::from_vec(vec![0xff]))],
    )
    .unwrap();
    assert_eq!(encoded.frame, native);
}

#[test]
fn encoder_matches_the_native_decoder_non_utf8_and_empty_argument_vector() {
    let native = b"ALABSET1\0\0\0\x1c\0\0\0\x02\0\0\0\x0c/workspace/\xff\0\0\0\0\0\0\0\0";
    let encoded = StartupSettings::new(
        &[OsString::from_vec(b"/workspace/\xff".to_vec()), "".into()],
        &[],
    )
    .unwrap();
    assert_eq!(encoded.frame, native);
}

#[test]
fn startup_arguments_are_literal_data_with_no_flag_interpretation() {
    let encoded = StartupSettings::new(
        &["--config".into(), "model='--alab-retained-service'".into()],
        &[(OsString::from_vec(vec![b'K', 0xff]), "a=b".into())],
    )
    .unwrap();
    let expected = b"ALABSET1\0\0\0D\0\0\0\x02\0\0\0\x08--config\0\0\0\x1fmodel='--alab-retained-service'\0\0\0\x01\0\0\0\x02K\xff\0\0\0\x03a=b";
    assert_eq!(encoded.frame, expected);
}

#[test]
fn invalid_settings_never_silently_drop_or_replace_selected_values() {
    for (arguments, environment) in [
        (vec![OsString::from_vec(b"a\0b".to_vec())], vec![]),
        (vec![], vec![("".into(), "value".into())]),
        (vec![], vec![("K=V".into(), "value".into())]),
        (vec![], vec![("K\0V".into(), "value".into())]),
        (vec![], vec![("K".into(), "a\0b".into())]),
        (
            vec![],
            vec![("K".into(), "first".into()), ("K".into(), "second".into())],
        ),
    ] {
        assert_eq!(
            StartupSettings::new(&arguments, &environment)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }
}

#[test]
fn native_body_limit_includes_count_and_length_prefixes() {
    let exact = OsString::from_vec(vec![b'x'; SETTINGS_BODY_LIMIT - 12]);
    let encoded = StartupSettings::new(&[exact], &[]).unwrap();
    assert_eq!(encoded.frame.len(), SETTINGS_BODY_LIMIT + 12);
    assert_eq!(
        &encoded.frame[8..12],
        &(SETTINGS_BODY_LIMIT as u32).to_be_bytes()
    );
    let excessive = OsString::from_vec(vec![b'x'; SETTINGS_BODY_LIMIT - 11]);
    assert!(StartupSettings::new(&[excessive], &[]).is_err());
}

#[tokio::test]
async fn physical_writer_closes_its_direction_after_the_complete_frame() {
    let settings = StartupSettings::new(&[], &[]).unwrap();
    let (mut source, mut native) = tokio::net::UnixStream::pair().unwrap();
    settings.write_and_shutdown(&mut source).await.unwrap();
    let mut received = Vec::new();
    native.read_to_end(&mut received).await.unwrap();
    assert_eq!(received, b"ALABSET1\0\0\0\x08\0\0\0\0\0\0\0\0");
    // Closing the settings writer must not close the opposite direction.
    native.write_all(b"still-open").await.unwrap();
    let mut reply = [0; 10];
    source.read_exact(&mut reply).await.unwrap();
    assert_eq!(&reply, b"still-open");
}

#[test]
fn diagnostics_redact_arguments_and_environment_values() {
    let encoded = StartupSettings::new(
        &["private-argument".into()],
        &[("SECRET".into(), "private-value".into())],
    )
    .unwrap();
    assert_eq!(format!("{encoded:?}"), "StartupSettings(<private>)");
}
