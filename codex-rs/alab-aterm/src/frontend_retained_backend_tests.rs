// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! DRAFT wire/socket controls only. No fake frame grants an SDK, Source,
//! foreground, backend constructor or app-server attachment permission.
use super::*;
use std::collections::BTreeMap;
use std::os::fd::AsRawFd as _;
use std::os::unix::fs::PermissionsExt as _;
use tokio::io::AsyncBufReadExt as _;
use tokio::io::AsyncWriteExt as _;
use tokio::io::BufReader;
use tokio::net::UnixListener;

const CONTROLLER: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const MEMBER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const NONCE: &[u8; 32] = b"0123456789abcdef0123456789abcdef";
const BIRTH: &str = "independent-backend-control-fixture";

struct Host {
    _directory: tempfile::TempDir,
    endpoint: crate::Endpoint,
    private: Arc<UnixStream>,
    calls: Arc<Mutex<Vec<String>>>,
    server: tokio::task::JoinHandle<()>,
}
impl Host {
    async fn open(bad_challenge: bool) -> (Self, Controller, std::os::unix::net::UnixStream) {
        let (host, child) = Self::listen(bad_challenge);
        let controller = Controller::open_at(&host.endpoint, CONTROLLER, BIRTH)
            .await
            .unwrap();
        (host, controller, child)
    }

    fn listen(bad_challenge: bool) -> (Self, std::os::unix::net::UnixStream) {
        let directory = tempfile::Builder::new().prefix("cx").tempdir().unwrap();
        let path = directory.path().join("aterm-1.sock");
        let token = directory.path().join("aterm-1.token");
        std::fs::write(&token, b"backend-control-fixture\n").unwrap();
        std::fs::set_permissions(token, std::fs::Permissions::from_mode(0o600)).unwrap();
        let listener = UnixListener::bind(&path).unwrap();
        let endpoint = crate::Endpoint {
            sock: path,
            sid: "t0pe7873g64".into(),
        };
        let sid = endpoint.sid.clone();
        let (private, child) = std::os::unix::net::UnixStream::pair().unwrap();
        private.set_nonblocking(true).unwrap();
        let private = Arc::new(UnixStream::from_std(private).unwrap());
        let issued = private.clone();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let recorded = calls.clone();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let caller = socket.peer_cred().unwrap().pid().unwrap();
            let (read, mut write) = socket.into_split();
            let mut read = BufReader::new(read);
            let mut line = String::new();
            read.read_line(&mut line).await.unwrap();
            assert_eq!(line, "AUTH backend-control-fixture\n");
            line.clear();
            read.read_line(&mut line).await.unwrap();
            assert_eq!(line, "version\n");
            write
                .write_all(b"OK version=backend-control-fixture\n")
                .await
                .unwrap();
            let mut binding = None;
            loop {
                line.clear();
                if read.read_line(&mut line).await.unwrap() == 0 {
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
                    "session":{"sid":sid,"process_pid":caller,"process_birth":BIRTH,"epoch":"backend-control-native"}});
                match action {
                    "member-peer" => frame["member"] = serde_json::Value::Null,
                    "controller-open" => assert_eq!(fields["operation"], CONTROLLER),
                    "controller-control-begin" => {
                        assert_eq!(fields["controller_operation"], CONTROLLER);
                        binding = Some(fields["operation"].to_owned());
                        let prefix = if bad_challenge {
                            b"BADFRAME"
                        } else {
                            b"ALABCTL1"
                        };
                        write_shared(&issued, prefix).await;
                        write_shared(&issued, NONCE).await;
                    }
                    "controller-control-ack" => {
                        assert_eq!(fields["controller_operation"], CONTROLLER);
                        assert_eq!(Some(fields["operation"]), binding.as_deref());
                        assert_eq!(fields["nonce"].as_bytes(), NONCE);
                    }
                    _ => panic!("unexpected credential, Root or fallback request: {action}"),
                }
                write
                    .write_all(format!("OK codex-auth {action} {frame}\n").as_bytes())
                    .await
                    .unwrap();
            }
        });
        (
            Self {
                _directory: directory,
                endpoint,
                private,
                calls,
                server,
            },
            child,
        )
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        self.server.abort();
    }
}

async fn write_shared(socket: &UnixStream, mut bytes: &[u8]) {
    while !bytes.is_empty() {
        socket.writable().await.unwrap();
        match socket.try_write(bytes) {
            Ok(0) => panic!("fixture socket closed during write"),
            Ok(count) => bytes = &bytes[count..],
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
            Err(error) => panic!("fixture write: {error}"),
        }
    }
}
async fn receipt(socket: &UnixStream) -> [u8; 32] {
    let mut bytes = [0; 32];
    let mut offset = 0;
    while offset < bytes.len() {
        socket.readable().await.unwrap();
        match socket.try_read(&mut bytes[offset..]) {
            Ok(0) => panic!("backend closed before nonce receipt"),
            Ok(count) => offset += count,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
            Err(error) => panic!("fixture receipt: {error}"),
        }
    }
    bytes
}

fn send_attachment(socket: &UnixStream, endpoint: &std::os::unix::net::UnixStream) {
    send_attachment_with_nonce(socket, endpoint, NONCE);
}
fn send_attachment_with_nonce(
    socket: &UnixStream,
    endpoint: &std::os::unix::net::UnixStream,
    nonce: &[u8; 32],
) {
    let mut payload = [0; 33];
    payload[0] = b'A';
    payload[1..].copy_from_slice(nonce);
    send_packet(socket, endpoint, &payload);
}
fn send_packet(socket: &UnixStream, endpoint: &std::os::unix::net::UnixStream, payload: &[u8]) {
    let mut storage = [0_usize; 8];
    let mut iov = libc::iovec {
        iov_base: payload.as_ptr().cast_mut().cast(),
        iov_len: payload.len(),
    };
    // SAFETY: live payload/ancillary buffers and one actual held synthetic fd.
    unsafe {
        let mut message: libc::msghdr = std::mem::zeroed();
        message.msg_iov = &mut iov;
        message.msg_iovlen = 1;
        message.msg_control = storage.as_mut_ptr().cast();
        message.msg_controllen = libc::CMSG_SPACE(std::mem::size_of::<libc::c_int>() as _) as _;
        let header = libc::CMSG_FIRSTHDR(&message);
        (*header).cmsg_level = libc::SOL_SOCKET;
        (*header).cmsg_type = libc::SCM_RIGHTS;
        (*header).cmsg_len = libc::CMSG_LEN(std::mem::size_of::<libc::c_int>() as _) as _;
        libc::CMSG_DATA(header)
            .cast::<libc::c_int>()
            .write_unaligned(endpoint.as_raw_fd());
        assert_eq!(
            libc::sendmsg(socket.as_raw_fd(), &message, 0),
            payload.len() as libc::ssize_t
        );
    }
}

async fn pending_endpoint(owner: &RetainedBackendOwner) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if owner
                .pending
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|pending| pending.stream.is_some())
            {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn cancelled_app_server_waiter_does_not_remove_an_actual_pending_endpoint() {
    use std::io::Write as _;
    let (host, controller, child) = Host::open(false).await;
    let mut owner = controller
        .bind_retained_backend_socket(child)
        .await
        .unwrap();
    let mut connections = owner.take_connections().unwrap();
    let (frontend, mut backend) = std::os::unix::net::UnixStream::pair().unwrap();
    backend.write_all(b"backend-work").unwrap();
    send_packet(&host.private, &frontend, b"A");
    pending_endpoint(&owner).await;
    assert!(
        tokio::time::timeout(Duration::from_millis(10), connections.next())
            .await
            .is_err()
    );
    assert!(
        owner
            .pending
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .stream
            .is_some()
    );
    write_shared(&host.private, NONCE).await;
    assert_eq!(receipt(&host.private).await, *NONCE);
    let mut endpoint = connections.next().await.unwrap().into_stream();
    let mut data = [0; 12];
    endpoint.read_exact(&mut data).await.unwrap();
    assert_eq!(&data, b"backend-work");
    assert!(
        tokio::time::timeout(Duration::from_millis(10), connections.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn owner_drop_withdraws_pending_io_while_its_actual_descriptor_stays_in_custody() {
    use std::io::Read as _;
    let (host, controller, child) = Host::open(false).await;
    let owner = controller
        .bind_retained_backend_socket(child)
        .await
        .unwrap();
    let (frontend, mut backend) = std::os::unix::net::UnixStream::pair().unwrap();
    backend.set_nonblocking(true).unwrap();
    send_packet(&host.private, &frontend, b"A");
    pending_endpoint(&owner).await;
    let retained = owner.pending.clone();
    drop(owner);
    assert!(retained.lock().unwrap().as_ref().unwrap().stream.is_some());
    let mut byte = [0];
    assert_eq!(backend.read(&mut byte).unwrap(), 0);
}

#[tokio::test]
async fn stalled_nonce_retires_original_channel_but_keeps_received_custody_without_ack() {
    let (host, controller, child) = Host::open(false).await;
    let owner = controller
        .bind_retained_backend_socket(child)
        .await
        .unwrap();
    tokio::time::pause();
    let (frontend, _backend) = std::os::unix::net::UnixStream::pair().unwrap();
    send_packet(&host.private, &frontend, b"A");
    pending_endpoint(&owner).await;
    tokio::time::advance(Duration::from_secs(61)).await;
    owner.withdrawn().await;
    assert!(
        owner
            .pending
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .stream
            .is_some()
    );
    let mut byte = [0; 1];
    assert_eq!(host.private.try_read(&mut byte).unwrap(), 0);
}

#[tokio::test]
async fn unwritable_nonce_receipt_has_the_same_bounded_transaction_after_queue_transfer() {
    let (host, controller, child) = Host::open(false).await;
    let mut owner = controller
        .bind_retained_backend_socket(child)
        .await
        .unwrap();
    let mut connections = owner.take_connections().unwrap();
    // Synthetic congestion control: fill the backend->native direction before
    // native reads any bytes, so the actual ACK cannot become writable.
    let filler = [b'x'; 8192];
    loop {
        match owner.socket.try_write(&filler) {
            Ok(0) => panic!("fixture socket closed"),
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
            Err(error) => panic!("fixture congestion: {error}"),
        }
    }
    tokio::time::pause();
    let (frontend, _backend) = std::os::unix::net::UnixStream::pair().unwrap();
    send_attachment(&host.private, &frontend);
    // The queue must own the endpoint before any receipt is attempted.
    let queued = connections.next().await.unwrap();
    assert!(
        owner
            .pending
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|pending| pending.stream.is_none())
    );
    tokio::time::advance(Duration::from_secs(61)).await;
    owner.withdrawn().await;
    drop(queued);
}

#[tokio::test]
async fn independent_controller_binds_once_then_persistent_a_frames_queue_before_ack() {
    use std::io::Write as _;
    let (host, controller, child) = Host::open(false).await;
    let mut owner = controller
        .bind_retained_backend_socket(child)
        .await
        .unwrap();
    let mut connections = owner.take_connections().unwrap();
    assert!(owner.take_connections().is_err());
    for _ in 0..2 {
        let (frontend, mut backend) = std::os::unix::net::UnixStream::pair().unwrap();
        backend.write_all(b"backend-work").unwrap();
        send_attachment(&host.private, &frontend);
        assert_eq!(receipt(&host.private).await, *NONCE);
        let mut endpoint = connections.next().await.unwrap().into_stream();
        let mut bytes = [0; 12];
        endpoint.read_exact(&mut bytes).await.unwrap();
        assert_eq!(&bytes, b"backend-work");
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(10), owner.withdrawn())
            .await
            .is_err()
    );
    let calls = host.calls.lock().unwrap();
    assert_eq!(
        calls
            .iter()
            .filter(|line| line.contains("controller-control-begin "))
            .count(),
        1
    );
    assert_eq!(
        calls
            .iter()
            .filter(|line| line.contains("controller-control-ack "))
            .count(),
        1
    );
    assert!(
        !calls
            .iter()
            .any(|line| line.contains("member-open ") || line.contains("member-ready "))
    );
}

#[tokio::test]
async fn original_channel_eof_is_observed_while_queue_is_full_and_pending_fd_stays_owned() {
    let (host, controller, child) = Host::open(false).await;
    let owner = controller
        .bind_retained_backend_socket(child)
        .await
        .unwrap();
    let mut retained = Vec::new();
    for index in 0..QUEUED_CONNECTIONS {
        let (frontend, backend) = std::os::unix::net::UnixStream::pair().unwrap();
        let nonce: [u8; 32] = format!("{index:032x}").into_bytes().try_into().unwrap();
        send_attachment_with_nonce(&host.private, &frontend, &nonce);
        assert_eq!(receipt(&host.private).await, nonce);
        retained.push((frontend, backend));
    }
    let (frontend, backend) = std::os::unix::net::UnixStream::pair().unwrap();
    send_attachment(&host.private, &frontend);
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if owner
                .pending
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|pending| pending.stream.is_some())
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    // SAFETY: release-only shutdown of this exact fixture-owned native channel.
    assert_eq!(
        unsafe { libc::shutdown(host.private.as_raw_fd(), libc::SHUT_WR) },
        0
    );
    tokio::time::timeout(Duration::from_secs(2), owner.withdrawn())
        .await
        .unwrap();
    assert!(
        owner
            .pending
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .stream
            .is_some()
    );
    drop((frontend, backend, retained));
}

#[tokio::test]
async fn queue_backpressure_expires_the_actual_pending_transaction_without_ack_or_rebirth() {
    let (host, controller, child) = Host::open(false).await;
    let owner = controller
        .bind_retained_backend_socket(child)
        .await
        .unwrap();
    let mut retained = Vec::new();
    for index in 0..QUEUED_CONNECTIONS {
        let (frontend, backend) = std::os::unix::net::UnixStream::pair().unwrap();
        let nonce: [u8; 32] = format!("{index:032x}").into_bytes().try_into().unwrap();
        send_attachment_with_nonce(&host.private, &frontend, &nonce);
        assert_eq!(receipt(&host.private).await, nonce);
        retained.push((frontend, backend));
    }
    tokio::time::pause();
    let (frontend, backend) = std::os::unix::net::UnixStream::pair().unwrap();
    send_attachment(&host.private, &frontend);
    loop {
        if owner
            .pending
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|pending| pending.stream.is_some())
        {
            break;
        }
        tokio::task::yield_now().await;
    }
    tokio::time::advance(Duration::from_secs(61)).await;
    owner.withdrawn().await;
    assert!(
        owner
            .pending
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .stream
            .is_some()
    );
    assert_eq!(
        host.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|line| line.contains("controller-control-begin "))
            .count(),
        1
    );
    drop((frontend, backend, retained));
}

#[tokio::test]
async fn duplicate_nonce_with_an_actual_live_origin_withdraws_without_a_second_attachment() {
    let (host, controller, child) = Host::open(false).await;
    let mut owner = controller
        .bind_retained_backend_socket(child)
        .await
        .unwrap();
    let mut connections = owner.take_connections().unwrap();
    let (first, first_peer) = std::os::unix::net::UnixStream::pair().unwrap();
    send_attachment(&host.private, &first);
    assert_eq!(receipt(&host.private).await, *NONCE);
    let original = connections.next().await.unwrap();

    // Keep the actual first receipt alive while a second physical endpoint
    // repeats its nonce. Wire syntax alone cannot replace that receipt Arc.
    let (duplicate, duplicate_peer) = std::os::unix::net::UnixStream::pair().unwrap();
    send_attachment(&host.private, &duplicate);
    tokio::time::timeout(Duration::from_secs(2), owner.withdrawn())
        .await
        .unwrap();
    assert!(connections.next().await.is_none());
    assert_eq!(original.origin.attachment(), *NONCE);
    assert_eq!(
        host.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|line| line.contains("controller-control-begin "))
            .count(),
        1
    );
    drop((original, first, first_peer, duplicate, duplicate_peer));
}

#[tokio::test]
async fn invalid_physical_challenge_never_acknowledges_or_falls_back() {
    let (host, controller, child) = Host::open(true).await;
    assert!(
        controller
            .bind_retained_backend_socket(child)
            .await
            .is_err()
    );
    let calls = host.calls.lock().unwrap();
    assert!(
        !calls
            .iter()
            .any(|line| line.contains("controller-control-ack ") || line.contains("member-open "))
    );
}

const STDIN_SOCKET_ENV: &str = "ALAB_RETAINED_STDIN_CONTROL_SOCKET";
const STDIN_SID_ENV: &str = "ALAB_RETAINED_STDIN_CONTROL_SID";

#[test]
#[ignore = "invoked only by the isolated stdin-adoption parent with an actual inherited socket"]
fn retained_backend_stdin_child() {
    let socket =
        std::env::var_os(STDIN_SOCKET_ENV).expect("stdin-adoption parent supplies endpoint");
    let sid = std::env::var(STDIN_SID_ENV).expect("stdin-adoption parent supplies session data");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let endpoint = crate::Endpoint {
            sock: socket.into(),
            sid,
        };
        let controller = Controller::open_at(&endpoint, CONTROLLER, BIRTH)
            .await
            .unwrap();
        let mut owner = controller.bind_retained_backend_stdin().await.unwrap();
        // Only this explicitly spawned child changes stdin. Check the actual
        // resulting descriptor, not a path or a claimed replacement flag.
        let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
        // SAFETY: fstat initializes the live output on success; read's one-byte
        // destination is writable. This child exclusively owns its stdin.
        unsafe {
            assert_eq!(libc::fstat(libc::STDIN_FILENO, stat.as_mut_ptr()), 0);
            assert_eq!(stat.assume_init().st_mode & libc::S_IFMT, libc::S_IFCHR);
            let mut byte = 0_u8;
            assert_eq!(
                libc::read(libc::STDIN_FILENO, (&mut byte as *mut u8).cast(), 1),
                0
            );
        }
        let mut connections = owner.take_connections().unwrap();
        println!("stdin-adopted");
        std::io::Write::flush(&mut std::io::stdout()).unwrap();
        let connection = tokio::time::timeout(Duration::from_secs(10), connections.next())
            .await
            .unwrap()
            .unwrap();
        let mut stream = connection.into_stream();
        let mut bytes = [0; 12];
        stream.read_exact(&mut bytes).await.unwrap();
        assert_eq!(&bytes, b"backend-work");
        println!("private-endpoint-used");
    });
}

#[tokio::test]
async fn actual_inherited_stdin_is_bound_replaced_and_delivers_the_private_endpoint() {
    use std::io::Write as _;
    use std::os::fd::OwnedFd;
    use std::process::Stdio;
    let (host, child_socket) = Host::listen(false);
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command.args([
        "--ignored",
        "--exact",
        "frontend_control::retained_backend::tests::retained_backend_stdin_child",
        "--nocapture",
    ]);
    command.env(STDIN_SOCKET_ENV, host.endpoint.sock.as_os_str());
    command.env(STDIN_SID_ENV, &host.endpoint.sid);
    command.stdin(Stdio::from(OwnedFd::from(child_socket)));
    command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn().unwrap();
    let actual_pid = child.id().unwrap();
    assert_ne!(actual_pid, std::process::id());
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut output = String::new();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let mut line = String::new();
            assert_ne!(
                stdout.read_line(&mut line).await.unwrap(),
                0,
                "stdin-adoption child ended before its proof"
            );
            let ready = line.contains("stdin-adopted");
            output.push_str(&line);
            if ready {
                break;
            }
        }
    })
    .await
    .unwrap();
    let (frontend, mut backend) = std::os::unix::net::UnixStream::pair().unwrap();
    backend.write_all(b"backend-work").unwrap();
    send_attachment(&host.private, &frontend);
    assert_eq!(receipt(&host.private).await, *NONCE);
    let status = tokio::time::timeout(Duration::from_secs(10), child.wait())
        .await
        .unwrap()
        .unwrap();
    stdout.read_to_string(&mut output).await.unwrap();
    let mut errors = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut errors)
        .await
        .unwrap();
    assert!(
        status.success(),
        "isolated stdin child failed: {output}; {errors}"
    );
    assert!(output.contains("private-endpoint-used"));
    assert_eq!(
        host.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|line| line.contains("controller-control-begin "))
            .count(),
        1
    );
}
