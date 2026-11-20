// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual native UI cells/render behavior, no Native/SDK/backend grants.
use super::*;
use pretty_assertions::assert_eq;
fn encoded(capture: &mut Capture, max_bytes: usize) -> io::Result<Vec<u8>> {
    capture.encode(&mut native::Capture::default(), max_bytes)
}
fn decoded(bytes: &[u8], max_bytes: usize) -> io::Result<Restored> {
    let mut shared = native::Restored::new(native::Capture::default().export()?)?;
    Restored::decode(
        bytes,
        max_bytes,
        &mut shared,
        &mut RestoreContext::default(),
    )
}
fn user(presentation: UserMessagePresentation) -> Arc<dyn HistoryCell> {
    Arc::new(UserHistoryCell {
        message: "actual user text\nwith a second line".into(),
        text_elements: Vec::new(),
        local_image_paths: Vec::new(),
        remote_image_urls: vec!["https://example.invalid/original-image".into()],
        presentation,
    })
}
#[test]
fn retained_finding_command_keeps_its_scoped_pending_identity() {
    let thread = codex_protocol::ThreadId::new();
    let mut exec = crate::exec_cell::new_active_exec_command(
        "read".into(),
        vec!["cat".into(), "answer.txt".into()],
        vec![],
        codex_app_server_protocol::CommandExecutionSource::UnifiedExecStartup,
        None,
        false,
    );
    assert!(exec.append_output("read", "partial bytes before the finding"));
    let exec = Arc::new(exec);
    let pending = crate::chatwidget::AlabPendingCommand::new(
        thread,
        "turn".into(),
        "read".into(),
        "cat answer.txt".into(),
    )
    .with_original(exec.clone(), Some("418".into()));
    let original: Arc<dyn HistoryCell> = Arc::new(pending);
    let original_exec: Arc<dyn HistoryCell> = exec;
    let mut capture = Capture::default();
    let key = capture.intern(&original).unwrap();
    let exec_key = capture.intern(&original_exec).unwrap();
    let other = capture.intern(&user(UserMessagePresentation::Typed)).unwrap();
    let bytes = encoded(&mut capture, 64 * 1024).unwrap();
    let mut wrong: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    wrong["cells"][key.0 as usize]["original"]["command"] = serde_json::json!(other.0);
    assert!(decoded(&serde_json::to_vec(&wrong).unwrap(), 64 * 1024).is_err());
    let copy = decoded(&bytes, 64 * 1024).unwrap();
    let cell = copy.cell(key).unwrap();
    let pending = cell
        .as_any()
        .downcast_ref::<crate::chatwidget::AlabPendingCommand>()
        .unwrap();
    assert!(pending.matches(thread, "turn", "read"));
    assert_eq!(pending.process_id(), Some("418"));
    let restored_exec: Arc<dyn HistoryCell> = pending.original().unwrap().clone();
    assert!(Arc::ptr_eq(&copy.cell(exec_key).unwrap(), &restored_exec));
    assert_eq!(cell.display_lines(90), original.display_lines(90));
    assert_eq!(cell.raw_lines(), original.raw_lines());
    assert!(
        cell.raw_lines()
            .iter()
            .any(|line| line.to_string().contains("partial bytes"))
    );
}

#[test]
fn actual_repeated_arc_is_shared_but_equal_distinct_cells_keep_distinct_identity() {
    let first = user(UserMessagePresentation::Typed);
    let second = user(UserMessagePresentation::Typed);
    let mut captured = Capture::default();
    let a = captured.intern(&first).unwrap();
    assert_eq!(captured.intern(&first).unwrap(), a);
    let b = captured.intern(&second).unwrap();
    assert_ne!(a, b);
    let restored = decoded(&encoded(&mut captured, 1024 * 1024).unwrap(), 1024 * 1024).unwrap();
    assert!(Arc::ptr_eq(
        &restored.cell(a).unwrap(),
        &restored.cell(a).unwrap()
    ));
    assert!(!Arc::ptr_eq(
        &restored.cell(a).unwrap(),
        &restored.cell(b).unwrap()
    ));
    for width in [20, 80] {
        assert_eq!(
            restored.cell(a).unwrap().display_lines(width),
            first.display_lines(width)
        );
        assert_eq!(restored.cell(a).unwrap().raw_lines(), first.raw_lines());
    }
}
#[test]
fn actual_async_reply_presentation_cwd_and_complete_answers_survive_resize() {
    let cell = user(UserMessagePresentation::AsyncReplies {
        replies: vec![crate::async_question_reply::AsyncQuestionReply {
            question_item_id: "original-item".into(),
            question: "Original question caption?".into(),
            answer: "first paragraph\n\nsecond original paragraph".into(),
        }],
        cwd: std::path::PathBuf::from("/original/session/directory"),
    });
    let mut capture = Capture::default();
    let index = capture.intern(&cell).unwrap();
    let restored = decoded(&encoded(&mut capture, 1024 * 1024).unwrap(), 1024 * 1024)
        .unwrap()
        .cell(index)
        .unwrap();
    for width in [16, 100] {
        assert_eq!(restored.display_lines(width), cell.display_lines(width));
        assert_eq!(restored.raw_lines(), cell.raw_lines());
    }
    let actual = restored.as_any().downcast_ref::<UserHistoryCell>().unwrap();
    assert!(
        matches!(&actual.presentation,UserMessagePresentation::AsyncReplies {cwd,..} if cwd==std::path::Path::new("/original/session/directory"))
    );
}
#[test]
fn malformed_graph_and_foreign_indices_cannot_create_a_native_cell() {
    assert!(decoded(br#"{"schema":5,"cells":[],"inline":{"schema":1,"caches":[]},"activity":{"rows":[]},"frames":0,"status":{"cards":[],"agents":[],"rates":[],"usage":{"estimates":[],"labels":[]}},"nativeCurrent":true}"#,1024).is_err());
    let empty=decoded(br#"{"schema":5,"cells":[],"inline":{"schema":1,"caches":[]},"activity":{"rows":[]},"frames":0,"status":{"cards":[],"agents":[],"rates":[],"usage":{"estimates":[],"labels":[]}}}"#,1024).unwrap();
    assert!(empty.cell(CellRef(0)).is_err());
    let cell = user(UserMessagePresentation::Spoken);
    let mut capture = Capture::default();
    capture.intern(&cell).unwrap();
    assert!(encoded(&mut capture, 1).is_err());
    let mut data: serde_json::Value =
        serde_json::from_slice(&encoded(&mut capture, 1024 * 1024).unwrap()).unwrap();
    data["cells"][0]["original"]["presentation"] =
        serde_json::json!({"kind":"spoken","callerGranted":true});
    assert!(decoded(&serde_json::to_vec(&data).unwrap(), 1024 * 1024).is_err());
}

#[test]
fn original_non_utf8_image_path_is_data_not_a_lossy_string() {
    use std::os::unix::ffi::OsStringExt;
    let path = std::path::PathBuf::from(std::ffi::OsString::from_vec(
        b"/actual/original/image-\xff.png".to_vec(),
    ));
    let original: Arc<dyn HistoryCell> = Arc::new(UserHistoryCell {
        message: "actual original attachment".into(),
        text_elements: Vec::new(),
        local_image_paths: vec![path.clone()],
        remote_image_urls: Vec::new(),
        presentation: UserMessagePresentation::Typed,
    });
    let mut capture = Capture::default();
    let index = capture.intern(&original).unwrap();
    let restored = decoded(&encoded(&mut capture, 1024 * 1024).unwrap(), 1024 * 1024)
        .unwrap()
        .cell(index)
        .unwrap();
    assert_eq!(
        restored
            .as_any()
            .downcast_ref::<UserHistoryCell>()
            .unwrap()
            .local_image_paths,
        vec![path]
    );
}

#[test]
fn capture_retains_actual_original_arc_until_its_identity_table_retires() {
    let original = user(UserMessagePresentation::Typed);
    let weak = Arc::downgrade(&original);
    let mut capture = Capture::default();
    let index = capture.intern(&original).unwrap();
    drop(original);
    let same = weak
        .upgrade()
        .expect("C1 identity table must retain actual cell while interning");
    assert_eq!(capture.intern(&same).unwrap(), index);
    drop(same);
    drop(capture);
    assert!(weak.upgrade().is_none());
}
