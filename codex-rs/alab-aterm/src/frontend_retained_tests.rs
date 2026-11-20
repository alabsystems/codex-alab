// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Fake wire peers and real Unix rights only. These fixtures grant no native
//! SDK/controller/foreground permission and qualify no installed application.
use super::*;
use std::collections::BTreeMap;
use std::os::fd::AsRawFd as _;
use std::os::unix::fs::PermissionsExt as _;
use tokio::io::AsyncBufReadExt as _;
use tokio::io::AsyncReadExt as _;
use tokio::io::AsyncWriteExt as _;
use tokio::io::BufReader;
use tokio::net::UnixListener;
use tokio::sync::Notify;

const MEMBER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const CONTROLLER: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OPERATION: &str = "cccccccccccccccccccccccccccccccc";
const SERVICE: &str = "dddddddddddddddddddddddddddddddd";
const BIRTH: &str = "retained-transport-control";
const NONCE: &[u8; 32] = b"0123456789abcdef0123456789abcdef";

#[derive(Clone, Copy)]
enum Behavior {
    Healthy,
    HoldReply,
    LoseReply,
    ChangedService,
    ExtraField,
    BusySettings,
    StartupField(&'static str, Option<&'static str>),
    StartupRefusal(&'static str),
}

struct Fixture {
    _directory: tempfile::TempDir,
    controller: Controller,
    server: JoinHandle<()>,
    requests: Arc<Mutex<Vec<String>>>,
    frames: Arc<Mutex<Vec<Vec<u8>>>>,
    endpoints: Arc<Mutex<Vec<UnixStream>>>,
    _fences: Arc<Mutex<Vec<UnixStream>>>,
    _callbacks: Arc<Mutex<Vec<UnixStream>>>,
    delivered: Arc<Notify>,
    release: Arc<Notify>,
}
impl Fixture {
    async fn open(behavior: Behavior) -> Self {
        let directory = tempfile::Builder::new().prefix("cx").tempdir().unwrap();
        let sock = directory.path().join("aterm-1.sock");
        let token = directory.path().join("aterm-1.token");
        std::fs::write(&token, b"retained-test\n").unwrap();
        std::fs::set_permissions(token, std::fs::Permissions::from_mode(0o600)).unwrap();
        let listener = UnixListener::bind(&sock).unwrap();
        let endpoint = Endpoint {
            sock,
            sid: "t0pe7873g64".into(),
        };
        let sid = endpoint.sid.clone();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let frames = Arc::new(Mutex::new(Vec::new()));
        let endpoints = Arc::new(Mutex::new(Vec::new()));
        let fences = Arc::new(Mutex::new(Vec::new()));
        let held_fences = fences.clone();
        let callbacks = Arc::new(Mutex::new(Vec::new()));
        let held_callbacks = callbacks.clone();
        let delivered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let (recorded, captured, sockets, sent, resume) = (
            requests.clone(),
            frames.clone(),
            endpoints.clone(),
            delivered.clone(),
            release.clone(),
        );
        let server = tokio::spawn(async move {
            let stream = listener.accept().await.unwrap().0;
            let (reader, mut writer) = stream.into_split();
            let mut reader = BufReader::new(reader);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            assert_eq!(line, "AUTH retained-test\n");
            line.clear();
            reader.read_line(&mut line).await.unwrap();
            assert_eq!(line, "version\n");
            writer
                .write_all(b"OK version=retained-transport-test\n")
                .await
                .unwrap();
            let mut starts = 0;
            loop {
                line.clear();
                if reader.read_line(&mut line).await.unwrap() == 0 {
                    break;
                }
                recorded.lock().unwrap().push(line.clone());
                let words = line.split_ascii_whitespace().collect::<Vec<_>>();
                let action = words[2];
                let fields = words[3..]
                    .iter()
                    .map(|word| word.split_once('=').unwrap())
                    .collect::<BTreeMap<_, _>>();
                let mut frame = serde_json::json!({"schema":1,"request":fields["request"],"operation":fields["operation"],"member":MEMBER,
                    "session":{"sid":sid,"process_pid":std::process::id(),"process_birth":BIRTH,"epoch":"retained-native-peer"}});
                match action {
                    "member-peer" => frame["member"] = serde_json::Value::Null,
                    "controller-open" => {}
                    "controller-startup-mode" => {
                        assert_eq!(words[0], format!("@{sid}"));
                        assert_eq!(words[1], "codex-auth");
                        assert_eq!(words.len(), 8);
                        assert_eq!(fields["schema"], "1");
                        assert_eq!(fields["member"], MEMBER);
                        assert_eq!(fields["controller_operation"], CONTROLLER);
                        assert!(valid_hex(fields["operation"], 32));
                        assert_ne!(fields["operation"], CONTROLLER);
                        assert_ne!(fields["operation"], OPERATION);
                        if let Behavior::StartupRefusal(status) = behavior {
                            writer.write_all(status.as_bytes()).await.unwrap();
                            continue;
                        }
                        frame["mode"] = serde_json::json!("retained");
                        if let Behavior::StartupField(field, value) = behavior {
                            if let Some(json) = value {
                                frame[field] = serde_json::from_str(json).unwrap();
                            } else {
                                frame.as_object_mut().unwrap().remove(field);
                            }
                        }
                    }
                    "controller-service-start" => {
                        assert_eq!(fields["operation"], OPERATION);
                        assert_eq!(fields["controller_operation"], CONTROLLER);
                        let mut callback = UnixStream::connect(decode_path(fields["settings"]))
                            .await
                            .unwrap();
                        let mut data = Vec::new();
                        callback.read_to_end(&mut data).await.unwrap();
                        assert_eq!(
                            callback.peer_cred().unwrap().pid(),
                            Some(std::process::id() as _),
                            "settings EOF must not retire the actual callback peer before native validates it"
                        );
                        captured.lock().unwrap().push(data);
                        starts += 1;
                        if matches!(behavior, Behavior::BusySettings) && starts < 8 {
                            writer.write_all(b"ERR codex-auth busy\n").await.unwrap();
                            continue;
                        }
                        frame["service"] =
                            serde_json::json!(if matches!(behavior, Behavior::ChangedService)
                                && starts > 1
                            {
                                "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
                            } else {
                                SERVICE
                            });
                        frame["state"] =
                            serde_json::json!(if starts == 1 { "starting" } else { "spawned" });
                        if matches!(behavior, Behavior::ExtraField) {
                            frame["sdk_ready"] = serde_json::json!(true);
                        }
                        // A terminal correlated response may precede native
                        // dropping this callback. Source must not wait for its
                        // EOF before accepting that response.
                        held_callbacks.lock().unwrap().push(callback);
                    }
                    "controller-service-attach" => {
                        assert_eq!(fields["service"], SERVICE);
                        let mut callback = UnixStream::connect(decode_path(fields["callback"]))
                            .await
                            .unwrap();
                        let (frontend, mut backend) = UnixStream::pair().unwrap();
                        send_rights(&callback, b'F', &[frontend.as_raw_fd()]);
                        // The real native OwnedService retains active.fence,
                        // the same frontend endpoint, across descriptor delivery.
                        held_fences.lock().unwrap().push(frontend);
                        callback.write_all(b"1").await.unwrap();
                        callback.write_all(NONCE).await.unwrap();
                        let mut receipt = [0; 32];
                        callback.read_exact(&mut receipt).await.unwrap();
                        assert_eq!(&receipt, NONCE);
                        let mut extra = [0; 1];
                        assert_eq!(callback.read(&mut extra).await.unwrap(), 0);
                        held_callbacks.lock().unwrap().push(callback);
                        backend.write_all(b"backend-work").await.unwrap();
                        sockets.lock().unwrap().push(backend);
                        sent.notify_one();
                        if matches!(behavior, Behavior::LoseReply) {
                            break;
                        }
                        if matches!(behavior, Behavior::HoldReply) {
                            resume.notified().await;
                        }
                    }
                    _ => panic!("unexpected route: {action}"),
                }
                writer
                    .write_all(format!("OK codex-auth {action} {frame}\n").as_bytes())
                    .await
                    .unwrap();
            }
        });
        let controller = Controller::open_at(&endpoint, CONTROLLER, BIRTH)
            .await
            .unwrap();
        Self {
            _directory: directory,
            controller,
            server,
            requests,
            frames,
            endpoints,
            _fences: fences,
            _callbacks: callbacks,
            delivered,
            release,
        }
    }

    fn assert_routes(&self, expected: &[&str]) {
        let requests = self.requests.lock().unwrap();
        let routes = requests
            .iter()
            .map(|line| line.split_ascii_whitespace().nth(2).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(routes, expected);
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

fn decode_path(encoded: &str) -> std::path::PathBuf {
    use std::os::unix::ffi::OsStringExt as _;
    let bytes = encoded
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect();
    std::path::PathBuf::from(std::ffi::OsString::from_vec(bytes))
}
fn settings() -> StartupSettings {
    use std::os::unix::ffi::OsStringExt as _;
    StartupSettings::new(
        &["x".into()],
        &[("K".into(), std::ffi::OsString::from_vec(vec![0xff]))],
    )
    .unwrap()
}

fn send_rights(stream: &UnixStream, byte: u8, rights: &[libc::c_int]) {
    let mut payload = [byte];
    let mut control = [0_usize; 272];
    let mut iov = libc::iovec {
        iov_base: payload.as_mut_ptr().cast(),
        iov_len: 1,
    };
    // SAFETY: all msghdr buffers are live and large enough for these test rights.
    unsafe {
        let mut message: libc::msghdr = std::mem::zeroed();
        message.msg_iov = &mut iov;
        message.msg_iovlen = 1;
        message.msg_control = control.as_mut_ptr().cast();
        message.msg_controllen = libc::CMSG_SPACE(std::mem::size_of_val(rights) as _) as _;
        let header = libc::CMSG_FIRSTHDR(&message);
        (*header).cmsg_level = libc::SOL_SOCKET;
        (*header).cmsg_type = libc::SCM_RIGHTS;
        (*header).cmsg_len = libc::CMSG_LEN(std::mem::size_of_val(rights) as _) as _;
        std::ptr::copy_nonoverlapping(
            rights.as_ptr().cast::<u8>(),
            libc::CMSG_DATA(header),
            std::mem::size_of_val(rights),
        );
        assert_eq!(libc::sendmsg(stream.as_raw_fd(), &message, 0), 1);
    }
}

#[tokio::test]
async fn failed_start_waiter_keeps_the_same_operation_callback_and_settings() {
    let fixture = Fixture::open(Behavior::Healthy).await;
    let mut start = fixture
        .controller
        .prepare_retained_start(OPERATION, settings())
        .unwrap();
    let callback = start.callback.clone();
    let settings = start.settings.clone();
    let worker = tokio::spawn(std::future::pending::<ServiceObservation>());
    worker.abort();
    start.pending = Some(worker);
    assert_eq!(
        start.observe().await,
        ServiceObservation::Unresolved(io::ErrorKind::Other)
    );
    assert_eq!(start.operation(), OPERATION);
    assert!(Arc::ptr_eq(&start.callback, &callback));
    assert!(Arc::ptr_eq(&start.settings, &settings));
    assert!(matches!(
        start.observe().await,
        ServiceObservation::Observed { .. }
    ));
    assert_eq!(
        fixture
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|line| line.contains("controller-service-start "))
            .count(),
        1
    );
}

#[tokio::test]
async fn failed_attachment_waiter_preserves_actual_custody_without_a_reply_or_resend() {
    let fixture = Fixture::open(Behavior::Healthy).await;
    let mut attachment = fixture
        .controller
        .prepare_retained_attachment(OPERATION, SERVICE)
        .unwrap();
    let callback = attachment.callback.clone();
    // Local custody failure control, not native delivery/SDK permission. Even
    // an actually owned descriptor cannot replace the correlated native reply.
    let (stream, mut peer) = UnixStream::pair().unwrap();
    *attachment.received.lock().unwrap() = Some(stream);
    let worker = tokio::spawn(std::future::pending::<AttachmentObservation>());
    worker.abort();
    attachment.pending = Some(worker);
    let result = attachment.observe().await;
    assert_eq!(
        result,
        AttachmentObservation::Unresolved {
            error: io::ErrorKind::Other,
            descriptor_received: true
        }
    );
    assert_eq!(attachment.observe().await, result);
    assert_eq!(attachment.operation(), OPERATION);
    assert!(Arc::ptr_eq(&attachment.callback, &callback));
    assert!(attachment.take_stream().is_err());
    peer.write_all(b"custody").await.unwrap();
    let mut bytes = [0; 7];
    // Inspect the fixture's private custody without an await under the mutex.
    // The public take_stream API above still refuses this unconfirmed stream.
    let mut retained = attachment.received.lock().unwrap().take().unwrap();
    retained.read_exact(&mut bytes).await.unwrap();
    *attachment.received.lock().unwrap() = Some(retained);
    assert_eq!(&bytes, b"custody");
    assert!(
        !fixture
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|line| line.contains("controller-service-attach "))
    );
}

#[tokio::test]
async fn bound_controller_reports_embedded_startup_mode() {
    let fixture = Fixture::open(Behavior::StartupField("mode", Some(r#""embedded""#))).await;
    assert_eq!(
        fixture.controller.startup_mode().await.unwrap(),
        StartupMode::Embedded
    );
    fixture.assert_routes(&["member-peer", "controller-open", "controller-startup-mode"]);
}

#[tokio::test]
async fn fresh_start_refuses_embedded_invalid_and_unavailable_policy_before_service_io() {
    use io::ErrorKind::InvalidInput;
    use io::ErrorKind::Other;
    use io::ErrorKind::PermissionDenied;
    use io::ErrorKind::Unsupported;

    for (behavior, expected) in [
        (
            Behavior::StartupField("mode", Some(r#""embedded""#)),
            Unsupported,
        ),
        (Behavior::StartupField("mode", Some(r#""future""#)), Other),
        (
            Behavior::StartupField("mode", Some(r#"{"retained":null}"#)),
            Other,
        ),
        (Behavior::StartupField("mode", None), Other),
        (Behavior::StartupField("sdk_ready", Some("true")), Other),
        (
            Behavior::StartupField("member", Some(r#""cccccccccccccccccccccccccccccccc""#)),
            Other,
        ),
        (
            Behavior::StartupField("operation", Some(r#""aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa""#)),
            Other,
        ),
        (
            Behavior::StartupRefusal("ERR codex-auth denied\n"),
            PermissionDenied,
        ),
        (
            Behavior::StartupRefusal("ERR codex-auth invalid request\n"),
            InvalidInput,
        ),
    ] {
        let fixture = Fixture::open(behavior).await;
        let result =
            retained_service::prepare_start(&fixture.controller, OPERATION, settings()).await;
        assert_eq!(result.err().expect("fresh start accepted").kind(), expected);
        fixture.assert_routes(&["member-peer", "controller-open", "controller-startup-mode"]);
        assert!(fixture.frames.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn startup_observations_keep_the_same_operation_and_complete_native_settings() {
    let fixture = Fixture::open(Behavior::Healthy).await;
    let mut start = retained_service::prepare_start(&fixture.controller, OPERATION, settings())
        .await
        .unwrap();
    assert_eq!(start.operation(), OPERATION);
    assert_eq!(
        start.observe().await,
        ServiceObservation::Observed {
            service: SERVICE.into(),
            phase: RetainedPhase::Starting
        }
    );
    assert_eq!(
        start.observe().await,
        ServiceObservation::Observed {
            service: SERVICE.into(),
            phase: RetainedPhase::Spawned
        }
    );
    let frames = fixture.frames.lock().unwrap();
    assert_eq!(frames.len(), 2);
    assert_eq!(
        frames[0],
        b"ALABSET1\0\0\0\x17\0\0\0\x01\0\0\0\x01x\0\0\0\x01\0\0\0\x01K\0\0\0\x01\xff"
    );
    assert_eq!(frames[0], frames[1]);
    fixture.assert_routes(&[
        "member-peer",
        "controller-open",
        "controller-startup-mode",
        "controller-service-start",
        "controller-service-start",
    ]);
}

#[tokio::test]
async fn settings_busy_retries_release_each_callback_without_changing_the_start_intent() {
    let fixture = Fixture::open(Behavior::BusySettings).await;
    let mut start = fixture
        .controller
        .prepare_retained_start(OPERATION, settings())
        .unwrap();
    assert_eq!(
        start.observe().await,
        ServiceObservation::Observed {
            service: SERVICE.into(),
            phase: RetainedPhase::Spawned
        }
    );
    let frames = fixture.frames.lock().unwrap();
    assert_eq!(
        frames.len(),
        8,
        "the valid eighth native response must not lose its settings owner"
    );
    assert!(frames.iter().all(|frame| frame == &frames[0]));
    assert_eq!(
        fixture
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| request.contains("controller-service-start "))
            .count(),
        8
    );
}

#[tokio::test]
async fn native_startup_bytes_after_settings_eof_are_rejected() {
    let fixture = Fixture::open(Behavior::Healthy).await;
    let context = Context::capture(&fixture.controller).unwrap();
    let callback = Callback::bind().unwrap();
    let mut native = UnixStream::connect(&callback.path).await.unwrap();
    let settings = settings();
    let (result, ()) = tokio::join!(
        callback.settings(&context.transport, context.peer, &settings),
        async {
            let mut body = Vec::new();
            native.read_to_end(&mut body).await.unwrap();
            assert!(!body.is_empty());
            native.write_all(b"unexpected").await.unwrap();
        }
    );
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);
}

#[tokio::test]
async fn changed_service_or_extra_authority_claims_do_not_become_admission() {
    for behavior in [Behavior::ChangedService, Behavior::ExtraField] {
        let fixture = Fixture::open(behavior).await;
        let mut start = fixture
            .controller
            .prepare_retained_start(OPERATION, settings())
            .unwrap();
        let first = start.observe().await;
        if matches!(behavior, Behavior::ChangedService) {
            assert!(matches!(first, ServiceObservation::Observed { .. }));
            assert!(matches!(
                start.observe().await,
                ServiceObservation::Unresolved(_)
            ));
        } else {
            assert!(matches!(first, ServiceObservation::Unresolved(_)));
        }
    }
}

#[tokio::test]
async fn cancelled_waiter_joins_one_physical_delivery_and_exposes_one_stream() {
    let fixture = Fixture::open(Behavior::HoldReply).await;
    let mut attachment = fixture
        .controller
        .prepare_retained_attachment(OPERATION, SERVICE)
        .unwrap();
    assert_eq!(attachment.operation(), OPERATION);
    assert!(
        tokio::time::timeout(Duration::from_millis(5), attachment.observe())
            .await
            .is_err()
    );
    tokio::time::timeout(Duration::from_secs(10), fixture.delivered.notified())
        .await
        .unwrap();
    assert!(attachment.take_stream().is_err());
    fixture.release.notify_one();
    assert_eq!(attachment.observe().await, AttachmentObservation::Confirmed);
    let mut stream = attachment.take_stream().unwrap();
    assert!(attachment.take_stream().is_err());
    let mut value = [0; 12];
    stream.read_exact(&mut value).await.unwrap();
    assert_eq!(&value, b"backend-work");
    assert_eq!(fixture.endpoints.lock().unwrap().len(), 1);
    assert_eq!(
        fixture
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|line| line.contains("controller-service-attach "))
            .count(),
        1
    );
}

#[tokio::test]
async fn lost_reply_keeps_received_custody_and_never_resends_the_delivery() {
    let fixture = Fixture::open(Behavior::LoseReply).await;
    let mut attachment = fixture
        .controller
        .prepare_retained_attachment(OPERATION, SERVICE)
        .unwrap();
    let result = attachment.observe().await;
    assert!(matches!(
        result,
        AttachmentObservation::Unresolved {
            descriptor_received: true,
            ..
        }
    ));
    assert!(attachment.take_stream().is_err());
    assert_eq!(attachment.observe().await, result);
    assert!(attachment.received.lock().unwrap().is_some());
    assert_eq!(
        fixture
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|line| line.contains("controller-service-attach "))
            .count(),
        1
    );
}

#[tokio::test]
async fn receiver_requires_one_connected_private_unix_stream_and_sets_cloexec() {
    let (callback, sender) = UnixStream::pair().unwrap();
    let (endpoint, _backend) = UnixStream::pair().unwrap();
    send_rights(&sender, b'F', &[endpoint.as_raw_fd()]);
    callback.readable().await.unwrap();
    let received = callback::receive(&callback).unwrap();
    // SAFETY: inspecting this actual owned descriptor.
    assert_ne!(
        unsafe { libc::fcntl(received.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC,
        0
    );
    for rights in [vec![], vec![endpoint.as_raw_fd(), endpoint.as_raw_fd()]] {
        let (callback, sender) = UnixStream::pair().unwrap();
        if rights.is_empty() {
            let mut sender = sender;
            sender.write_all(b"F").await.unwrap();
        } else {
            send_rights(&sender, b'F', &rights);
        }
        callback.readable().await.unwrap();
        assert!(callback::receive(&callback).is_err());
    }
    let file = std::fs::File::open("/dev/null").unwrap();
    let tcp = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let outgoing = std::net::TcpStream::connect(tcp.local_addr().unwrap()).unwrap();
    let _accepted = tcp.accept().unwrap();
    for right in [file.as_raw_fd(), outgoing.as_raw_fd(), tcp.as_raw_fd()] {
        let (callback, sender) = UnixStream::pair().unwrap();
        send_rights(&sender, b'F', &[right]);
        callback.readable().await.unwrap();
        assert!(callback::receive(&callback).is_err());
    }
}

#[tokio::test]
async fn malformed_marker_and_trailing_callback_data_refuse_the_endpoint() {
    let fixture = Fixture::open(Behavior::Healthy).await;
    let context = Context::capture(&fixture.controller).unwrap();
    for (marker, trailing) in [(b'X', false), (b'F', true)] {
        let callback = Callback::bind().unwrap();
        let mut sender = UnixStream::connect(&callback.path).await.unwrap();
        let (right, _backend) = UnixStream::pair().unwrap();
        send_rights(&sender, marker, &[right.as_raw_fd()]);
        if trailing {
            sender.write_all(b"unexpected").await.unwrap();
        }
        drop(sender);
        assert!(
            callback
                .endpoint(&context.transport, context.peer)
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn valid_callback_retains_the_original_endpoint_and_its_backend_bytes() {
    let fixture = Fixture::open(Behavior::Healthy).await;
    let context = Context::capture(&fixture.controller).unwrap();
    let callback = Callback::bind().unwrap();
    let mut native = UnixStream::connect(&callback.path).await.unwrap();
    let (frontend, mut backend) = UnixStream::pair().unwrap();
    let producer = tokio::spawn(async move {
        send_rights(&native, b'F', &[frontend.as_raw_fd()]);
        native.write_all(b"1").await.unwrap();
        native.write_all(NONCE).await.unwrap();
        backend.write_all(b"backend-work").await.unwrap();
        let mut receipt = [0; 32];
        native.read_exact(&mut receipt).await.unwrap();
        assert_eq!(&receipt, NONCE);
        let mut extra = [0; 1];
        assert_eq!(native.read(&mut extra).await.unwrap(), 0);
        (frontend, backend, native)
    });
    let received = Mutex::new(None);
    let delivery = callback
        .endpoint(&context.transport, context.peer)
        .await
        .unwrap();
    delivery.acknowledge(&received).await.unwrap();
    let _native_owners = producer.await.unwrap();
    let mut stream = received.lock().unwrap().take().unwrap();
    let mut bytes = [0; 12];
    stream.read_exact(&mut bytes).await.unwrap();
    assert_eq!(&bytes, b"backend-work");
}

#[tokio::test]
async fn legacy_callback_closed_before_peer_observation_never_yields_an_endpoint() {
    let fixture = Fixture::open(Behavior::Healthy).await;
    let context = Context::capture(&fixture.controller).unwrap();
    let callback = Callback::bind().unwrap();
    let native = UnixStream::connect(&callback.path).await.unwrap();
    let (frontend, _backend) = UnixStream::pair().unwrap();
    send_rights(&native, b'F', &[frontend.as_raw_fd()]);
    drop(native);
    assert!(
        callback
            .endpoint(&context.transport, context.peer)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn versioned_callback_rejects_invalid_nonce_version_and_additional_rights() {
    let fixture = Fixture::open(Behavior::Healthy).await;
    let context = Context::capture(&fixture.controller).unwrap();
    for (version, nonce, extra_right) in [
        (b'2', NONCE.as_slice(), false),
        (b'1', b"g123456789abcdef0123456789abcdef".as_slice(), false),
        (b'1', NONCE.as_slice(), true),
    ] {
        let callback = Callback::bind().unwrap();
        let mut native = UnixStream::connect(&callback.path).await.unwrap();
        let (frontend, _backend) = UnixStream::pair().unwrap();
        send_rights(&native, b'F', &[frontend.as_raw_fd()]);
        if extra_right {
            send_rights(&native, version, &[frontend.as_raw_fd()]);
        } else {
            native.write_all(&[version]).await.unwrap();
        }
        native.write_all(nonce).await.unwrap();
        assert!(
            callback
                .endpoint(&context.transport, context.peer)
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn callback_peer_must_equal_the_original_authenticated_kernel_peer() {
    let fixture = Fixture::open(Behavior::Healthy).await;
    let context = Context::capture(&fixture.controller).unwrap();
    let callback = Callback::bind().unwrap();
    let mut sender = UnixStream::connect(&callback.path).await.unwrap();
    assert!(
        tokio::time::timeout(
            Duration::from_millis(10),
            callback.settings(&context.transport, context.peer + 1, &settings())
        )
        .await
        .is_err()
    );
    let mut byte = [0];
    assert_eq!(
        sender.read(&mut byte).await.unwrap(),
        0,
        "a mismatched kernel peer receives no startup bytes"
    );
}

#[cfg(target_vendor = "apple")]
#[test]
fn selected_std_spawns_and_pre_exec_strip_an_unlisted_non_cloexec_socket() {
    use std::ffi::CString;
    use std::os::fd::FromRawFd as _;
    use std::os::fd::OwnedFd;
    use std::os::unix::process::CommandExt as _;
    let (socket, _peer) = std::os::unix::net::UnixStream::pair().unwrap();
    // A high descriptor avoids confusing the socket with a shell's own small
    // stdio/script descriptors. This is synthetic test IO, never native control.
    let duplicated = unsafe { libc::fcntl(socket.as_raw_fd(), libc::F_DUPFD, 128) };
    assert!(duplicated >= 128);
    let leaked = unsafe { OwnedFd::from_raw_fd(duplicated) };
    assert_eq!(
        unsafe { libc::fcntl(leaked.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC,
        0
    );
    let script = "test ! -S \"/dev/fd/$1\"";
    for fork_path in [false, true] {
        let mut command = std::process::Command::new("/bin/sh");
        command.args(["-c", script, "fd-control", &leaked.as_raw_fd().to_string()]);
        if fork_path {
            // An empty async-signal-safe pre_exec selects std's fork route.
            unsafe {
                command.pre_exec(|| Ok(()));
            }
        }
        assert!(
            command.status().unwrap().success(),
            "selected std fork path={fork_path} leaked an unlisted socket"
        );
    }

    // Negative control explicitly lists this same socket in a raw Darwin file
    // action. CLOEXEC_DEFAULT strips every other unlisted descriptor, so the
    // control cannot accidentally inherit another test's concurrent receipt.
    let words = [
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        script.to_owned(),
        "fd-control".to_owned(),
        leaked.as_raw_fd().to_string(),
    ];
    let words = words
        .into_iter()
        .map(|word| CString::new(word).unwrap())
        .collect::<Vec<_>>();
    let mut argv = words
        .iter()
        .map(|word| word.as_ptr().cast_mut())
        .collect::<Vec<_>>();
    argv.push(std::ptr::null_mut());
    let environment = CString::new("PATH=/usr/bin:/bin").unwrap();
    let envp = [environment.as_ptr().cast_mut(), std::ptr::null_mut()];
    let mut actions = std::mem::MaybeUninit::uninit();
    let mut attributes = std::mem::MaybeUninit::uninit();
    let mut pid = 0;
    // SAFETY: initialized spawn objects, live NUL-terminated argv/envp, one
    // explicitly held synthetic descriptor and an ordinary system child.
    unsafe {
        assert_eq!(libc::posix_spawn_file_actions_init(actions.as_mut_ptr()), 0);
        assert_eq!(libc::posix_spawnattr_init(attributes.as_mut_ptr()), 0);
        let mut actions = actions.assume_init();
        let mut attributes = attributes.assume_init();
        assert_eq!(
            libc::posix_spawn_file_actions_adddup2(
                &mut actions,
                leaked.as_raw_fd(),
                leaked.as_raw_fd()
            ),
            0
        );
        assert_eq!(
            libc::posix_spawnattr_setflags(&mut attributes, libc::POSIX_SPAWN_CLOEXEC_DEFAULT as _),
            0
        );
        let spawned = libc::posix_spawn(
            &mut pid,
            words[0].as_ptr(),
            &actions,
            &attributes,
            argv.as_ptr(),
            envp.as_ptr(),
        );
        libc::posix_spawn_file_actions_destroy(&mut actions);
        libc::posix_spawnattr_destroy(&mut attributes);
        assert_eq!(spawned, 0);
    }
    let mut status = 0;
    loop {
        let waited = unsafe { libc::waitpid(pid, &mut status, 0) };
        if waited == pid {
            break;
        }
        assert!(waited < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted);
    }
    assert!(libc::WIFEXITED(status));
    assert_eq!(
        libc::WEXITSTATUS(status),
        1,
        "the negative control must detect the explicitly inherited socket"
    );
}
