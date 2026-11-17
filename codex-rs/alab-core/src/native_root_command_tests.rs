// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::os::unix::ffi::OsStringExt as _;

fn command() -> Command {
    Command {
        program: "/bin/sh".into(),
        arg0: Some("shell-name".into()),
        arguments: vec![OsString::from_vec(vec![255])],
        environment: vec![("KEY".into(), "value".into())],
        directory: "/tmp".into(),
    }
}

#[test]
fn encoding_preserves_original_argument_bytes_and_override() {
    let value: serde_json::Value = serde_json::from_slice(&command().encode().unwrap()).unwrap();
    assert_eq!(value["arg0"], serde_json::json!(b"shell-name"));
    assert_eq!(value["arguments"], serde_json::json!([[255]]));
    assert_eq!(value["setup"], "pipe");
}

#[test]
fn invalid_commands_fail_before_requesting_native_admission() {
    let mut value = command();
    value.arg0 = Some(OsString::from_vec(vec![0]));
    assert!(value.encode().is_err());
    value = command();
    value.environment.push(("KEY".into(), "duplicate".into()));
    assert!(value.encode().is_err());
    value = command();
    value.directory = "relative".into();
    assert!(value.encode().is_err());
    value = command();
    value.arguments = vec![OsString::from("x".repeat(512 * 1024))];
    assert!(value.encode().is_err());
}
