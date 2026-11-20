// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Direct original JSON decoder controls only. Opaque test leaf DATA is not
//! a whole App, installed UI, callback/resource owner or Native/Source proof.
use super::*;
use crate::owned_local_entered_readers::constructor::InitialMessageData;
use pretty_assertions::assert_eq;
use std::os::fd::FromRawFd;
use std::os::fd::OwnedFd;
use std::os::unix::fs::MetadataExt;

fn section() -> NativeSection {
    NativeSection::captured(
        RawValue::from_string(
            "{ \"nativeDraft\" : [\"original\\ntext\", {\"nested\":null}] }".into(),
        )
        .unwrap(),
    )
}
#[test]
fn both_original_ui_arms_decode_direct_json_and_keep_required_raw_sections() {
    let cases = [
        UiData::NoPrimary {
            startup: section(),
            pending_views: section(),
            initial_message: section(),
            presentation: section(),
        },
        UiData::Selected {
            messages: section(),
            composer: section(),
            questions: section(),
            pending_views: section(),
            application: section(),
        },
    ];
    for original in cases {
        let bytes = serde_json::to_vec(&original).unwrap();
        let restored: UiData = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(serde_json::to_vec(&restored).unwrap(), bytes);
        restored.validate_sections().unwrap();
        let original_raw = section().body().get().to_owned();
        match restored {
            UiData::NoPrimary {
                startup,
                pending_views,
                initial_message,
                presentation,
            } => {
                for actual in [startup, pending_views, initial_message, presentation] {
                    assert_eq!(actual.body().get(), original_raw);
                }
            }
            UiData::Selected {
                messages,
                composer,
                questions,
                pending_views,
                application,
            } => {
                for actual in [messages, composer, questions, pending_views, application] {
                    assert_eq!(actual.body().get(), original_raw);
                }
            }
        }
    }
}
#[test]
fn original_requested_editor_raw_bytes_decode_without_claiming_child_or_handback() {
    let original = EditorData::Requested {
        original_request: section(),
    };
    let bytes = serde_json::to_vec(&original).unwrap();
    let restored: EditorData = serde_json::from_slice(&bytes).unwrap();
    restored.validate().unwrap();
    assert_eq!(serde_json::to_vec(&restored).unwrap(), bytes);
    let EditorData::Requested { original_request } = restored else {
        panic!("requested DATA changed state")
    };
    assert_eq!(original_request.body().get(), section().body().get());
}
#[test]
fn ui_variant_unknown_missing_cross_fields_and_map_shaped_state_are_refused() {
    let original = UiData::NoPrimary {
        startup: section(),
        pending_views: section(),
        initial_message: section(),
        presentation: section(),
    };
    let raw = serde_json::to_value(original).unwrap();
    for extra in [
        "callerGranted",
        "messages",
        "composer",
        "questions",
        "application",
    ] {
        let mut bad = raw.clone();
        bad[extra] = serde_json::json!(null);
        assert!(
            serde_json::from_slice::<UiData>(&serde_json::to_vec(&bad).unwrap()).is_err(),
            "{extra}"
        );
    }
    for required in [
        "startup",
        "pending_views",
        "initial_message",
        "presentation",
    ] {
        let mut bad = raw.clone();
        bad.as_object_mut().unwrap().remove(required);
        assert!(
            serde_json::from_slice::<UiData>(&serde_json::to_vec(&bad).unwrap()).is_err(),
            "{required}"
        );
    }
    for state in [
        serde_json::json!({"noPrimary":null}),
        serde_json::json!("other"),
        serde_json::json!(null),
    ] {
        let mut bad = raw.clone();
        bad["state"] = state;
        assert!(serde_json::from_slice::<UiData>(&serde_json::to_vec(&bad).unwrap()).is_err());
    }
    assert!(
        serde_json::from_slice::<UiData>(br#"{"state":"selected","state":"noPrimary"}"#).is_err()
    );
}
#[test]
fn closed_editor_has_no_payload_and_requested_keeps_its_exact_closed_variant() {
    assert!(matches!(
        serde_json::from_slice::<EditorData>(br#"{"state":"closed"}"#).unwrap(),
        EditorData::Closed {}
    ));
    for bad in [
        br#"{"state":"closed","checkpoint":null}"#.as_slice(),
        br#"{"state":{"closed":null}}"#.as_slice(),
        br#"{"state":"requested"}"#.as_slice(),
        br#"{"state":"unknown"}"#.as_slice(),
    ] {
        assert!(serde_json::from_slice::<EditorData>(bad).is_err());
    }
    let mut requested = serde_json::to_value(EditorData::Requested {
        original_request: section(),
    })
    .unwrap();
    requested["checkpoint"] = serde_json::json!(null);
    assert!(
        serde_json::from_slice::<EditorData>(&serde_json::to_vec(&requested).unwrap()).is_err()
    );
}

fn terminal_data() -> io::Result<(TerminalData, [std::fs::File; 2])> {
    let mut master = -1;
    let mut slave = -1;
    // SAFETY: both output slots are writable and every optional argument is null.
    if unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful openpty returned two fresh owned file descriptors.
    let master = std::fs::File::from(unsafe { OwnedFd::from_raw_fd(master) });
    // SAFETY: the slave is distinct and is adopted exactly once.
    let slave = std::fs::File::from(unsafe { OwnedFd::from_raw_fd(slave) });
    let mut mode = std::mem::MaybeUninit::<libc::termios>::uninit();
    use std::os::fd::AsRawFd;
    // SAFETY: the actual owned slave TTY initializes this writable termios.
    if unsafe { libc::tcgetattr(slave.as_raw_fd(), mode.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful tcgetattr initialized all fields.
    let original = unsafe { mode.assume_init() };
    let mut running = original;
    // SAFETY: this modifies only our initialized local DATA; no TTY mode is written.
    unsafe { libc::cfmakeraw(&mut running) };
    fn wide(value: impl Into<u64>) -> u64 {
        value.into()
    }
    fn data(mode: &libc::termios) -> serde_json::Value {
        #[cfg(any(target_os = "linux", target_os = "android"))]
        let line = mode.c_line;
        #[cfg(not(any(target_os = "linux", target_os = "android")))]
        let line = 0_u8;
        // SAFETY: both getters read our initialized local termios.
        let input_speed = unsafe { libc::cfgetispeed(mode) };
        // SAFETY: same initialized local mode, no descriptor or I/O side effect.
        let output_speed = unsafe { libc::cfgetospeed(mode) };
        serde_json::json!({"flags":[wide(mode.c_iflag),wide(mode.c_oflag),wide(mode.c_cflag),wide(mode.c_lflag)],
            "characters":mode.c_cc.to_vec(),"input_speed":wide(input_speed),"output_speed":wide(output_speed),"line":line})
    }
    let metadata = slave.metadata()?;
    // This is actual PTY mode/identity DATA, never terminal_modes::Prepared,
    // a broker fence, Native permission or a restoration/admission receipt.
    let modes = serde_json::from_value(
        serde_json::json!({"version":1,"operating_system":std::env::consts::OS,
        "terminal":{"device":metadata.dev(),"inode":metadata.ino(),"rdevice":metadata.rdev()},
        "original":data(&original),"running":data(&running)}),
    )
    .map_err(io::Error::other)?;
    let startup = codex_alab_tui::upgrade_startup::Snapshot {
        version: 1,
        cursor: [3, 4],
        colors: Some([[1, 2, 3], [4, 5, 6]]),
        enhanced_keys: false,
        terminal_app_over_ssh: false,
        reports_key_releases: false,
        da1_supported: false,
        tmux_image_base: None,
        viewport: [0, 0, 80, 24],
        saved_viewport: None,
        hidden_cursor: false,
        visible_history_rows: 2,
        screen: codex_alab_tui::upgrade_startup::Screen {
            active: false,
            mouse_active: false,
            mouse_capture_disabled: false,
            input_configured: false,
            keyboard_active: false,
            parent_reports_key_releases: false,
            pointer: 0,
        },
        focused: true,
        owned_screen: false,
        alt_screen_enabled: false,
        mouse_capture: 0,
    };
    Ok((
        TerminalData {
            modes,
            startup,
            input: b"original accepted parser suffix".to_vec(),
        },
        [master, slave],
    ))
}

async fn saved_chooser_checkpoints(
    child: ThreadId,
    opening: &str,
) -> io::Result<Vec<Arc<ClientConstructionCheckpoint>>> {
    use codex_app_server_client::ClientConstructionSelection;
    use codex_app_server_client::ClientPresentation;
    use codex_app_server_protocol::RequestId;
    use codex_app_server_protocol::ThreadResumeParams;
    use futures::SinkExt;
    use futures::StreamExt;
    use serde_json::json;
    use std::time::Duration;
    use tokio_tungstenite::tungstenite::Message;

    let child = child.to_string();
    let selection = ClientConstructionSelection {
        presentation: ClientPresentation::SideConversation,
        prospective_create: Default::default(),
        prepared: None,
    };
    let waiting = ClientConstructionOperation::for_saved_resume(
        selection.clone(),
        &uuid::Uuid::new_v4().simple().to_string(),
        &child,
    )?;
    let ordinary = ClientConstructionOperation::new(selection.clone())?;
    let issued = ClientConstructionOperation::for_saved_resume(selection, opening, &child)?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let endpoint =
        crate::resolve_remote_addr(&format!("ws://{}", listener.local_addr()?)).map_err(invalid)?;
    let expected_opening = opening.to_owned();
    let expected_child = child.clone();
    // Capture the original client's actual sent request, then lose its reply.
    let peer = tokio::spawn(async move {
        let (io, _) = listener.accept().await?;
        let mut socket = tokio_tungstenite::accept_async(io).await.map_err(invalid)?;
        while let Some(message) = socket.next().await {
            let Message::Text(text) = message.map_err(invalid)? else {
                continue;
            };
            let request: serde_json::Value = serde_json::from_str(&text)?;
            let result = match request["method"].as_str() {
                Some("initialize") => json!({"userAgent":"saved-opening-test/1.0"}),
                Some("initialized") => continue,
                Some("config/read") => {
                    let prepare = &request["params"]["_meta"]["alab.clientConstructionPrepare"];
                    assert_eq!(prepare["preparation"], expected_opening);
                    assert_eq!(prepare["parameters"]["threadId"], expected_child);
                    json!({"config":{}, "origins":{}, "layers":[], "_meta":{"alab.clientConstructionPrepared":{
                        "phase":"prepared", "preparation":expected_opening, "claim":"a".repeat(32),
                        "sourceOperation":"b".repeat(32), "selection":"c".repeat(32)
                    }}})
                }
                Some("thread/resume") => {
                    assert_eq!(request["params"]["threadId"], expected_child);
                    assert_eq!(
                        request["params"]["_meta"]["alab.clientConstruction"]["prepared"]["preparation"],
                        expected_opening
                    );
                    socket.close(None).await.map_err(invalid)?;
                    return Ok(());
                }
                other => panic!("unexpected original request {other:?}"),
            };
            let reply = json!({"id":request["id"],"result":result});
            socket
                .send(Message::Text(reply.to_string().into()))
                .await
                .map_err(invalid)?;
        }
        Err(invalid("original saved resume was never sent"))
    });
    let client = crate::connect_remote_app_server(endpoint)
        .await
        .map_err(invalid)?;
    let handle = client.request_handle();
    let (result, unreadable, lost) = tokio::time::timeout(
        Duration::from_secs(60),
        issued.run(handle.request_saved_resume_observed(
            RequestId::Integer(19),
            ThreadResumeParams {
                thread_id: child,
                model: Some("original-model".into()),
                ..Default::default()
            },
        )),
    )
    .await
    .map_err(invalid)?;
    assert!(result.is_err() && !unreadable && lost);
    client.shutdown().await?;
    tokio::time::timeout(Duration::from_secs(60), peer)
        .await
        .map_err(invalid)?
        .map_err(invalid)??;
    Ok(vec![
        Arc::new(ordinary.checkpoint()),
        Arc::new(waiting.checkpoint()),
        Arc::new(issued.checkpoint()),
    ])
}

async fn pending_widget_section(selected: bool) -> io::Result<NativeSection> {
    let (mut widget, _sender, _events, mut commands) =
        crate::boxed_future!(crate::chatwidget::tests::make_chatwidget_manual_with_sender()).await;
    while commands.try_recv().is_ok() {}
    widget.set_queue_autosend_suppressed(true);
    assert!(widget.queue_user_message_with_options(
        crate::chatwidget::UserMessage::from("pending compiler follow-up"),
        crate::bottom_pane::QueuedInputAction::Literal,
        Vec::new(),
    ));
    assert!(commands.try_recv().is_err());
    if selected {
        Ok(NativeSection::captured(
            widget.alab_capture_retained_messages()?,
        ))
    } else {
        InitialMessageData::from_widget(&widget, &None)?.section()
    }
}

fn assert_pending_input_preflight(bytes: &[u8], service: &str, selected: bool) -> io::Result<()> {
    let valid: serde_json::Value = serde_json::from_slice(&bytes[MAGIC.len()..])?;
    let field = if selected {
        "messages"
    } else {
        "initial_message"
    };
    for remove_field in [false, true] {
        let mut altered = valid.clone();
        let body = &mut altered["ui"][field]["body"];
        let pending = if selected {
            body
        } else {
            &mut body["widgetMessages"]
        };
        let queue = &mut pending["input_queue"];
        assert_eq!(queue["suppress_queue_autosend"], true);
        assert_eq!(
            queue["queued_user_messages"][0]["user_message"]["text"],
            "pending compiler follow-up"
        );
        if remove_field {
            queue
                .as_object_mut()
                .unwrap()
                .remove("suppress_queue_autosend");
        } else {
            queue["queued_user_messages"]
                .as_array_mut()
                .unwrap()
                .clear();
        }
        let mut bad = MAGIC.to_vec();
        bad.extend(serde_json::to_vec(&altered)?);
        // The outer frame is complete. Both ends must also validate the input it carries.
        let frame = decode(&bad, service)?;
        assert!(
            matches!(decode_with_pending_input(&bad, service), Err(error) if error.kind() == io::ErrorKind::InvalidData)
        );
        let refused = encode(
            service,
            EncodeInputs {
                configuration: frame.configuration,
                selection: frame.selection,
                terminal: frame.terminal,
                ui: frame.ui,
                editor: frame.editor,
                image_custody: frame.image_custody,
                clipboard: frame.clipboard,
            },
        );
        assert_eq!(refused.unwrap_err().kind(), io::ErrorKind::InvalidData);
    }
    Ok(())
}

#[tokio::test]
async fn production_full_frame_encode_decode_preserves_both_ui_arms_and_requested_raw_payload()
-> io::Result<()> {
    const SERVICE: &str = "0123456789abcdef0123456789abcdef";
    // The original public loader completes and supplies its actual typed DATA
    // snapshot. No AuthManager, Native entry or accepted Source owner exists.
    let loader = codex_config::CloudConfigBundleLoader::new(async { Ok(None) });
    let observed = loader.get_snapshot().await;
    let configuration = UiSnapshot::from_backend(
        &observed,
        codex_utils_absolute_path::AbsolutePathBuf::from_absolute_path(std::path::PathBuf::from(
            "/original-workspace",
        ))
        .map_err(io::Error::other)?,
        None,
    )?;
    for selected in [false, true] {
        let (terminal, _actual_pty) = terminal_data()?;
        let thread = ThreadId::new();
        let opening = uuid::Uuid::new_v4().simple().to_string();
        let initial = Some(
            InitialConstructionOperation::new(
                codex_app_server_client::InitialSessionSelection {
                    claim: uuid::Uuid::new_v4().simple().to_string(),
                    prospective_create: None,
                },
                codex_app_server_client::InitialAction::Resume(thread.to_string()),
            )?
            .checkpoint(),
        );
        let (selection, ui) = if selected {
            (
                SelectionData::Selected {
                    primary: thread,
                    displayed: thread,
                    initial,
                    later: saved_chooser_checkpoints(thread, &opening).await?,
                },
                UiData::Selected {
                    messages: pending_widget_section(true).await?,
                    composer: section(),
                    questions: section(),
                    pending_views: section(),
                    application: section(),
                },
            )
        } else {
            (
                SelectionData::NoPrimary { initial },
                UiData::NoPrimary {
                    startup: section(),
                    pending_views: section(),
                    initial_message: pending_widget_section(false).await?,
                    presentation: section(),
                },
            )
        };
        // The actual original image-owner fixture census is empty. Preparing
        // its Offer observes that census; the decoder has no NoFiles default.
        let image_owners = Vec::new();
        let images = codex_alab_tui::upgrade_exec_custody::images::Offer::prepare(&image_owners)?;
        let bytes = encode(
            SERVICE,
            EncodeInputs {
                configuration: configuration.clone(),
                selection,
                terminal,
                ui,
                editor: EditorData::Requested {
                    original_request: section(),
                },
                image_custody: images.locator_data()?.clone(),
                clipboard: ClipboardData::test_joined_original()?,
            },
        )?;
        let (actual, _messages) = decode_with_pending_input(&bytes, SERVICE)?;
        assert_pending_input_preflight(&bytes, SERVICE, selected)?;
        // Compare the previous v2 encoding and the bounded writer on the same
        // original raw sections. The complete frame includes the magic header.
        let mut previous = MAGIC.to_vec();
        previous.extend(serde_json::to_vec(&actual).map_err(io::Error::other)?);
        let encode_at = |limit| {
            upgrade_frame::encode(
                MAGIC,
                &actual,
                limit,
                "retained UI exceeds Native E1 input limit",
            )
        };
        assert_eq!(encode_at(previous.len())?, previous);
        let oversized = encode_at(previous.len() - 1).unwrap_err();
        assert_eq!(oversized.kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            oversized.to_string(),
            "retained UI exceeds Native E1 input limit"
        );
        actual.configuration.validate_data()?;
        assert_eq!(
            actual.configuration.selected_cwd,
            configuration.selected_cwd
        );
        assert_eq!(
            actual.terminal.input_data(),
            b"original accepted parser suffix"
        );
        assert_eq!(matches!(actual.ui, UiData::Selected { .. }), selected);
        let handoffs = actual.selection.saved_resume_queries()?;
        if selected {
            assert_eq!(handoffs.len(), 1);
            assert_eq!(handoffs[0].query().opening(), opening);
            assert_eq!(handoffs[0].query().child(), thread.to_string());

            let mut raw: serde_json::Value =
                serde_json::from_slice(&bytes[MAGIC.len()..]).map_err(io::Error::other)?;
            let later = raw["selection"]["later"].as_array_mut().unwrap();
            later.push(later.last().unwrap().clone());
            let mut duplicate = MAGIC.to_vec();
            duplicate.extend(serde_json::to_vec(&raw).map_err(io::Error::other)?);
            assert!(decode(&duplicate, SERVICE).is_err());
            let later = raw["selection"]["later"].as_array_mut().unwrap();
            later.truncate(later.len() - 1);
            later[0]["schema"] = 42.into();
            let mut wrong_schema = MAGIC.to_vec();
            wrong_schema.extend(serde_json::to_vec(&raw).map_err(io::Error::other)?);
            assert!(decode(&wrong_schema, SERVICE).is_err());
        } else {
            assert!(handoffs.is_empty());
        }
        let EditorData::Requested { original_request } = &actual.editor else {
            panic!("original Requested state changed")
        };
        assert_eq!(original_request.body().get(), section().body().get());
        for mutation in 0..5 {
            let mut raw: serde_json::Value = serde_json::from_slice(&bytes[MAGIC.len()..]).unwrap();
            match mutation {
                0 => {
                    raw["ui"].as_object_mut().unwrap().remove(if selected {
                        "messages"
                    } else {
                        "startup"
                    });
                }
                1 => {
                    raw["ui"]["unknownOriginal"] = true.into();
                }
                2 => {
                    raw["ui"]["state"] = if selected {
                        serde_json::json!({"selected":null})
                    } else {
                        serde_json::json!({"noPrimary":null})
                    };
                }
                3 => {
                    raw["editor"]["unknownOriginal"] = true.into();
                }
                _ => {
                    raw["selection"]["initial"]["request"] = serde_json::json!({
                        "method": "thread/start", "hadParams": false, "params": null
                    });
                }
            }
            let mut bad = MAGIC.to_vec();
            bad.extend_from_slice(&serde_json::to_vec(&raw).unwrap());
            assert!(
                decode(&bad, SERVICE).is_err(),
                "selected={selected}, mutation={mutation}"
            );
        }
        let original = std::str::from_utf8(&bytes[MAGIC.len()..]).unwrap();
        let duplicate = original.replacen("\"version\":1", "\"version\":1,\"version\":1", 1);
        let mut bad = MAGIC.to_vec();
        bad.extend_from_slice(duplicate.as_bytes());
        assert!(decode(&bad, SERVICE).is_err());
        let section = serde_json::to_string(&section()).unwrap();
        let pending = format!("\"pending_views\":{section}");
        assert!(original.contains(&pending));
        let duplicate = original.replacen(&pending, &format!("{pending},{pending}"), 1);
        let mut bad = MAGIC.to_vec();
        bad.extend_from_slice(duplicate.as_bytes());
        assert!(decode(&bad, SERVICE).is_err());
        if let Some(handoff) = handoffs.first() {
            assert_second_saved_opening_frame(actual, handoff, &bytes)?;
        }
    }
    Ok(())
}

fn assert_second_saved_opening_frame(
    frame: Frame,
    handoff: &SavedResumeHandoff,
    first: &[u8],
) -> io::Result<()> {
    let SelectionData::Selected {
        primary,
        displayed,
        initial,
        later,
    } = frame.selection
    else {
        panic!("expected the original selected frame");
    };
    let issued = later
        .iter()
        .find(|checkpoint| {
            checkpoint.saved_opening().is_some_and(|(opening, child)| {
                opening == handoff.query().opening() && child == handoff.query().child()
            })
        })
        .expect("original issued checkpoint");
    assert!(Arc::ptr_eq(issued, handoff.checkpoint()));

    // An old v2 array encoded the plain checkpoint values. Keep the expected
    // second frame's other bytes and replace only that array with the issued row.
    let plain: Vec<_> = later.iter().map(Arc::as_ref).collect();
    let before = format!("\"later\":{}", serde_json::to_string(&plain)?);
    let after = format!("\"later\":{}", serde_json::to_string(&[issued.as_ref()])?);
    let first = std::str::from_utf8(first).map_err(invalid)?;
    assert_eq!(first.matches(&before).count(), 1);
    let expected = first.replacen(&before, &after, 1);

    let second = encode(
        &frame.service,
        EncodeInputs {
            configuration: frame.configuration,
            selection: SelectionData::Selected {
                primary,
                displayed,
                initial,
                later: vec![Arc::clone(handoff.checkpoint())],
            },
            terminal: frame.terminal,
            ui: frame.ui,
            editor: frame.editor,
            image_custody: frame.image_custody,
            clipboard: frame.clipboard,
        },
    )?;
    assert_eq!(second.as_slice(), expected.as_bytes());
    let (frame, _messages) = decode_with_pending_input(&second, &frame.service)?;
    let reentered = frame.selection.saved_resume_queries()?;
    assert_eq!(reentered.len(), 1);
    assert_eq!(reentered[0].query().as_ref(), handoff.query().as_ref());
    assert_eq!(
        serde_json::to_vec(reentered[0].checkpoint())?,
        serde_json::to_vec(issued)?
    );
    Ok(())
}

#[tokio::test]
async fn production_frame_requires_observed_null_instead_of_missing_policy_or_initial_owner()
-> io::Result<()> {
    const SERVICE: &str = "0123456789abcdef0123456789abcdef";
    let loader = codex_config::CloudConfigBundleLoader::new(async { Ok(None) });
    let observed = loader.get_snapshot().await;
    let configuration = UiSnapshot::from_backend(
        &observed,
        codex_utils_absolute_path::AbsolutePathBuf::from_absolute_path(std::path::PathBuf::from(
            "/original-workspace",
        ))
        .map_err(io::Error::other)?,
        None,
    )?;
    for selected in [false, true] {
        let (terminal, _actual_pty) = terminal_data()?;
        let thread = ThreadId::new();
        let (selection, ui) = if selected {
            (
                SelectionData::Selected {
                    primary: thread,
                    displayed: thread,
                    initial: None,
                    later: Vec::new(),
                },
                UiData::Selected {
                    messages: pending_widget_section(true).await?,
                    composer: section(),
                    questions: section(),
                    pending_views: section(),
                    application: section(),
                },
            )
        } else {
            (
                SelectionData::NoPrimary { initial: None },
                UiData::NoPrimary {
                    startup: section(),
                    pending_views: section(),
                    initial_message: InitialMessageData::from_actual(&None).section()?,
                    presentation: section(),
                },
            )
        };
        let image_owners = Vec::new();
        let images = codex_alab_tui::upgrade_exec_custody::images::Offer::prepare(&image_owners)?;
        let bytes = encode(
            SERVICE,
            EncodeInputs {
                configuration: configuration.clone(),
                selection,
                terminal,
                ui,
                editor: EditorData::Closed {},
                image_custody: images.locator_data()?.clone(),
                clipboard: ClipboardData::test_joined_original()?,
            },
        )?;
        let original: serde_json::Value = serde_json::from_slice(&bytes[MAGIC.len()..]).unwrap();
        // The original production serializer emits every observed None as null.
        assert_eq!(original["selection"]["initial"], serde_json::Value::Null);
        for field in ["bundle", "config_server", "oss", "initial_session_claim"] {
            assert!(
                original["configuration"]
                    .as_object()
                    .unwrap()
                    .contains_key(field)
            );
            assert_eq!(original["configuration"][field], serde_json::Value::Null);
        }
        let actual = decode(&bytes, SERVICE)?;
        match actual.selection {
            SelectionData::NoPrimary { initial } | SelectionData::Selected { initial, .. } => {
                assert!(initial.is_none())
            }
        }
        for (section, field) in [
            ("selection", "initial"),
            ("configuration", "bundle"),
            ("configuration", "config_server"),
            ("configuration", "oss"),
            ("configuration", "initial_session_claim"),
        ] {
            let mut omitted = original.clone();
            assert!(
                omitted[section]
                    .as_object_mut()
                    .unwrap()
                    .remove(field)
                    .is_some()
            );
            let mut bad = MAGIC.to_vec();
            bad.extend_from_slice(&serde_json::to_vec(&omitted).unwrap());
            assert!(
                decode(&bad, SERVICE).is_err(),
                "selected={selected}, {section}.{field}"
            );
        }
    }
    Ok(())
}

#[tokio::test]
async fn production_frame_requires_explicit_image_census_and_rejects_malformed_locator()
-> io::Result<()> {
    const SERVICE: &str = "0123456789abcdef0123456789abcdef";
    let loader = codex_config::CloudConfigBundleLoader::new(async { Ok(None) });
    let observed = loader.get_snapshot().await;
    let configuration = UiSnapshot::from_backend(
        &observed,
        codex_utils_absolute_path::AbsolutePathBuf::from_absolute_path(std::path::PathBuf::from(
            "/original-workspace",
        ))
        .map_err(io::Error::other)?,
        None,
    )?;
    let (terminal, _actual_pty) = terminal_data()?;
    // This fixture actually owns no image files. Only the original Offer
    // observes that fact; serde cannot manufacture an observed empty census.
    let original_image_owners = Vec::new();
    let images =
        codex_alab_tui::upgrade_exec_custody::images::Offer::prepare(&original_image_owners)?;
    let bytes = encode(
        SERVICE,
        EncodeInputs {
            configuration,
            selection: SelectionData::NoPrimary { initial: None },
            terminal,
            ui: UiData::NoPrimary {
                startup: section(),
                pending_views: section(),
                initial_message: InitialMessageData::from_actual(&None).section()?,
                presentation: section(),
            },
            editor: EditorData::Requested {
                original_request: section(),
            },
            image_custody: images.locator_data()?.clone(),
            clipboard: ClipboardData::test_joined_original()?,
        },
    )?;
    let actual = decode(&bytes, SERVICE)?;
    actual.image_custody.validate_data()?;
    let original: serde_json::Value = serde_json::from_slice(&bytes[MAGIC.len()..]).unwrap();
    assert_eq!(
        original["image_custody"],
        serde_json::json!({"schema":1,"bundle":null})
    );
    for mutation in 0..7 {
        let mut bad = original.clone();
        match mutation {
            0 => {
                bad.as_object_mut().unwrap().remove("image_custody");
            }
            1 => bad["image_custody"] = serde_json::Value::Null,
            2 => {
                bad["image_custody"]
                    .as_object_mut()
                    .unwrap()
                    .remove("bundle");
            }
            3 => bad["image_custody"]["schema"] = 2.into(),
            4 => bad["image_custody"]["filesApplied"] = true.into(),
            5 => {
                bad["image_custody"]["bundle"] =
                    serde_json::json!({"nonce":vec![0_u8;32],"files":[]})
            }
            _ => bad["image_custody"]["schema"] = serde_json::json!({"1":null}),
        }
        let mut encoded = MAGIC.to_vec();
        encoded.extend_from_slice(&serde_json::to_vec(&bad).unwrap());
        assert!(decode(&encoded, SERVICE).is_err(), "mutation={mutation}");
    }
    let encoded = std::str::from_utf8(&bytes[MAGIC.len()..]).unwrap();
    let locator = serde_json::to_string(images.locator_data()?).unwrap();
    let field = format!("\"image_custody\":{locator}");
    assert!(encoded.contains(&field));
    let duplicate = encoded.replacen(&field, &format!("{field},{field}"), 1);
    let mut encoded = MAGIC.to_vec();
    encoded.extend_from_slice(duplicate.as_bytes());
    assert!(decode(&encoded, SERVICE).is_err());
    Ok(())
}

#[tokio::test]
async fn production_frame_requires_original_joined_clipboard_data_and_rejects_omission_null_duplicate_or_unknown_shape()
-> io::Result<()> {
    const SERVICE: &str = "0123456789abcdef0123456789abcdef";
    let loader = codex_config::CloudConfigBundleLoader::new(async { Ok(None) });
    let observed = loader.get_snapshot().await;
    let configuration = UiSnapshot::from_backend(
        &observed,
        codex_utils_absolute_path::AbsolutePathBuf::from_absolute_path(std::path::PathBuf::from(
            "/original-workspace",
        ))
        .map_err(io::Error::other)?,
        None,
    )?;
    let (terminal, _actual_pty) = terminal_data()?;
    let actual_image_owners = Vec::new();
    let images =
        codex_alab_tui::upgrade_exec_custody::images::Offer::prepare(&actual_image_owners)?;
    // Same production worker join/capture path: never-started is observed by
    // its actual private ledger. No DTO/default/no-worker constructor or Native
    // permission is used by this outer decoder control.
    let clipboard = ClipboardData::test_joined_original()?;
    clipboard.validate_data()?;
    let original_clipboard = serde_json::to_value(&clipboard).map_err(io::Error::other)?;
    let bytes = encode(
        SERVICE,
        EncodeInputs {
            configuration,
            selection: SelectionData::NoPrimary { initial: None },
            terminal,
            ui: UiData::NoPrimary {
                startup: section(),
                pending_views: section(),
                initial_message: InitialMessageData::from_actual(&None).section()?,
                presentation: section(),
            },
            editor: EditorData::Requested {
                original_request: section(),
            },
            image_custody: images.locator_data()?.clone(),
            clipboard,
        },
    )?;
    let actual = decode(&bytes, SERVICE)?;
    assert_eq!(actual.version, 2);
    assert_eq!(section().schema, 1);
    let mut old_magic = b"ALAB-RETAIN-UI1\n".to_vec();
    old_magic.extend_from_slice(&bytes[MAGIC.len()..]);
    assert!(decode(&old_magic, SERVICE).is_err());
    let mut old_header: serde_json::Value =
        serde_json::from_slice(&bytes[MAGIC.len()..]).map_err(io::Error::other)?;
    old_header["version"] = 1.into();
    let mut old_encoded = MAGIC.to_vec();
    old_encoded.extend_from_slice(&serde_json::to_vec(&old_header).map_err(io::Error::other)?);
    assert!(decode(&old_encoded, SERVICE).is_err());
    actual.clipboard.validate_data()?;
    assert_eq!(
        serde_json::to_value(&actual.clipboard).map_err(io::Error::other)?,
        original_clipboard
    );
    let original: serde_json::Value =
        serde_json::from_slice(&bytes[MAGIC.len()..]).map_err(io::Error::other)?;
    for mutation in 0..5 {
        let mut bad = original.clone();
        match mutation {
            0 => {
                bad.as_object_mut()
                    .ok_or_else(|| io::Error::other("original frame is not object"))?
                    .remove("clipboard");
            }
            1 => bad["clipboard"] = serde_json::Value::Null,
            2 => bad["clipboard"]["schema"] = 2.into(),
            3 => bad["clipboard"]["workerStopped"] = true.into(),
            _ => {
                bad["clipboard"]
                    .as_object_mut()
                    .ok_or_else(|| io::Error::other("original clipboard data is not object"))?
                    .remove("pending");
            }
        }
        let mut encoded = MAGIC.to_vec();
        encoded.extend_from_slice(&serde_json::to_vec(&bad).map_err(io::Error::other)?);
        assert!(decode(&encoded, SERVICE).is_err(), "mutation={mutation}");
    }
    let original_json = std::str::from_utf8(&bytes[MAGIC.len()..]).map_err(io::Error::other)?;
    let field = format!(
        "\"clipboard\":{}",
        serde_json::to_string(&actual.clipboard).map_err(io::Error::other)?
    );
    assert!(original_json.contains(&field));
    let duplicate = original_json.replacen(&field, &format!("{field},{field}"), 1);
    let mut encoded = MAGIC.to_vec();
    encoded.extend_from_slice(duplicate.as_bytes());
    assert!(decode(&encoded, SERVICE).is_err());
    Ok(())
}
