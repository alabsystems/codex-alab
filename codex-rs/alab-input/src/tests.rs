// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::*;
use std::fs::File;
use std::io::Read;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::fd::FromRawFd;
use std::os::unix::process::CommandExt;
use std::sync::Arc;
use std::task::Context;
use std::task::Poll;
use std::task::Wake;
use std::task::Waker;
use std::time::Duration;
use std::time::Instant;
fn all(parser: &mut Parser) -> Vec<InternalEvent> {
    std::iter::from_fn(|| parser.pop()).collect()
}
#[test]
fn every_split_retains_ready_events_partial_sequences_and_query_data() {
    let input = "aé🦀\x1b[A\x1b[1;5D\x1b[<0;3;4M\x1b[200~paste 🦀\x1b[201~\x1b[I\x1b[O\x1b[?3u\x1b[20;10R\x1b]10;rgb:ffff/0000/7777\x1b\\".as_bytes();
    let mut whole = Parser::default();
    whole.feed(input, true).unwrap();
    let wanted = all(&mut whole);
    for cut in 0..=input.len() {
        let mut source = Parser::default();
        source.feed(&input[..cut], true).unwrap();
        let mut destination = Parser::restore(&source.checkpoint().unwrap()).unwrap();
        destination.feed(&input[cut..], true).unwrap();
        assert_eq!(all(&mut destination), wanted, "input split {cut}");
    }
    let mut raw = Parser::default();
    raw.feed(b"\n\x1b[10u", true).unwrap();
    raw.resize(120, 40);
    let mut destination = Parser::restore(&raw.checkpoint().unwrap()).unwrap();
    assert_eq!(all(&mut destination), all(&mut raw));
}
#[test]
fn checkpoint_keeps_escape_deadline_and_explicit_quarantine() {
    let mut source = Parser::default();
    source.feed(b"\x1b", true).unwrap();
    let before = source.checkpoint().unwrap();
    source.feed(b"", true).unwrap();
    assert_eq!(source.checkpoint().unwrap(), before);
    std::thread::sleep(Duration::from_millis(30));
    let mut destination = Parser::restore(&before).unwrap();
    destination.flush_escape().unwrap();
    assert!(
        matches!(destination.pop(), Some(InternalEvent::Event(crossterm::event::Event::Key(key))) if key.code == crossterm::event::KeyCode::Esc)
    );
    assert!(destination.pop().is_none());
    for prefix in [
        b"\x1b[200~unfinished".as_slice(),
        b"\x1b]10;unfinished",
        b"\x1b[<0;",
    ] {
        let mut source = Parser::default();
        source.feed(prefix, true).unwrap();
        source.discard_buffered_input();
        let mut destination = Parser::restore(&source.checkpoint().unwrap()).unwrap();
        let suffix: &[u8] = if prefix.starts_with(b"\x1b[200") {
            b"rest\x1b[201~x".as_slice()
        } else if prefix.starts_with(b"\x1b]") {
            b"rest\x1b\\x"
        } else {
            b"1;2Mx"
        };
        source.feed(suffix, true).unwrap();
        destination.feed(suffix, true).unwrap();
        assert_eq!(all(&mut destination), all(&mut source));
    }
}
#[test]
fn malformed_checkpoints_refuse_instead_of_becoming_absence_or_extra_keys() {
    let mut source = Parser::default();
    source.feed(b"a", true).unwrap();
    let original: serde_json::Value =
        serde_json::from_slice(&source.checkpoint().unwrap()).unwrap();
    for mutation in 0..8 {
        let mut value = original.clone();
        match mutation {
            0 => value["version"] = serde_json::json!(2),
            1 => {
                value.as_object_mut().unwrap().remove("escape");
            }
            2 => value["unknown"] = serde_json::json!(true),
            3 => value["buffer"] = serde_json::json!(b"complete".to_vec()),
            4 => value["ready"][0]["Input"]["bytes"] = serde_json::json!(b"ab".to_vec()),
            5 => value["ready"][0]["Input"]["bytes"] = serde_json::json!(b"\x1b[".to_vec()),
            6 => value["discarded_sequence"] = serde_json::json!({"PasteBody": 99}),
            7 => value["escape"] = serde_json::json!([1, 0, 1_000_000_000u32]),
            _ => unreachable!(),
        }
        assert!(
            Parser::restore(&serde_json::to_vec(&value).unwrap()).is_err(),
            "mutation {mutation}"
        );
    }
}
// SIGWINCH is process-wide; independent PTY fixtures must not inject resize
// events into one another's consumer queues.
static PTY_TESTS: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn pty() -> (File, File) {
    let (mut master, mut slave) = (-1, -1);
    // SAFETY: openpty initializes both output descriptors, with default termios.
    assert_eq!(
        unsafe {
            libc::openpty(
                &raw mut master,
                &raw mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        },
        0
    );
    // SAFETY: successful openpty created exactly these uniquely owned descriptors.
    let (master, slave) = unsafe { (File::from_raw_fd(master), File::from_raw_fd(slave)) };
    // SAFETY: zeroed termios is storage initialized by the checked tcgetattr.
    let mut settings: libc::termios = unsafe { std::mem::zeroed() };
    assert_eq!(
        unsafe { libc::tcgetattr(slave.as_raw_fd(), &raw mut settings) },
        0
    );
    unsafe { libc::cfmakeraw(&raw mut settings) };
    assert_eq!(
        unsafe { libc::tcsetattr(slave.as_raw_fd(), libc::TCSANOW, &raw const settings) },
        0
    );
    assert_eq!(
        unsafe { libc::fcntl(slave.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) },
        0
    );
    (master, slave)
}
struct ThreadWake(std::thread::Thread);
impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}
fn event(driver: &Driver) -> InternalEvent {
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match driver.poll_event(&mut context) {
            Poll::Ready(Some(event)) => return event.unwrap(),
            Poll::Ready(None) => panic!("input driver ended"),
            Poll::Pending => {
                assert!(Instant::now() < deadline, "input did not arrive");
                std::thread::park_timeout(Duration::from_millis(10));
            }
        }
    }
}
#[test]
fn joined_reader_retains_partial_paste_and_os_input_on_the_original_pty() {
    let _serial = PTY_TESTS.lock().unwrap();
    let (mut master, slave) = pty();
    let driver = Driver::start(slave, Parser::default(), true).unwrap();
    master.write_all(b"a\x1b[200~before").unwrap();
    assert!(
        matches!(event(&driver), InternalEvent::Event(crossterm::event::Event::Key(key)) if key.code == crossterm::event::KeyCode::Char('a'))
    );
    let frozen = driver.freeze().unwrap();
    let snapshot = frozen.parser.checkpoint().unwrap();
    let value: serde_json::Value = serde_json::from_slice(&snapshot).unwrap();
    assert_eq!(
        value["buffer"],
        serde_json::json!(b"\x1b[200~before".to_vec())
    );
    // The old producer is joined. Bytes accepted while frozen remain on this
    // original PTY, and can only be read by the replacement owner below.
    master.write_all(b" after\x1b[201~z").unwrap();
    let driver = Driver::start(frozen.input, Parser::restore(&snapshot).unwrap(), true).unwrap();
    assert_eq!(
        event(&driver),
        InternalEvent::Event(crossterm::event::Event::Paste("before after".into()))
    );
    assert!(
        matches!(event(&driver), InternalEvent::Event(crossterm::event::Event::Key(key)) if key.code == crossterm::event::KeyCode::Char('z'))
    );
    let frozen = driver.freeze().unwrap();
    assert!(!frozen.parser.has_events());
}
#[test]
fn actual_same_pid_exec_restores_the_input_queue_and_partial_parser() {
    const STAGE: &str = "ALAB_INPUT_EXEC_STAGE";
    const DATA: &str = "ALAB_INPUT_EXEC_DATA";
    const PID: &str = "ALAB_INPUT_EXEC_PID";
    const SUCCESS: &str = "ALAB_INPUT_EXEC_OK";
    const TEST: &str = "tests::actual_same_pid_exec_restores_the_input_queue_and_partial_parser";
    match std::env::var(STAGE).ok().as_deref() {
        None => {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", TEST, "--nocapture"])
                .env(STAGE, "source")
                .stdout(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(15);
            loop {
                if let Some(status) = child.try_wait().unwrap() {
                    assert!(status.success());
                    let mut output = String::new();
                    child
                        .stdout
                        .take()
                        .unwrap()
                        .read_to_string(&mut output)
                        .unwrap();
                    assert!(
                        output.contains(SUCCESS),
                        "child case did not complete: {output}"
                    );
                    break;
                }
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!("input exec timed out");
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        Some("source") => {
            let mut parser = Parser::default();
            parser.feed(b"queued\x1b[200~partial", true).unwrap();
            let data = String::from_utf8(parser.checkpoint().unwrap()).unwrap();
            let error = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", TEST, "--nocapture"])
                .env(STAGE, "destination")
                .env(DATA, data)
                .env(PID, std::process::id().to_string())
                .exec();
            panic!("exec failed: {error}");
        }
        Some("destination") => {
            assert_eq!(std::env::var(PID).unwrap(), std::process::id().to_string());
            // These variables select and carry fixture data only; production
            // uses authenticated SCM custody, never an environment grant.
            let mut parser = Parser::restore(std::env::var(DATA).unwrap().as_bytes()).unwrap();
            parser.feed(b" paste\x1b[201~", true).unwrap();
            let events = all(&mut parser);
            assert_eq!(events.len(), 7);
            assert_eq!(
                events.last(),
                Some(&InternalEvent::Event(crossterm::event::Event::Paste(
                    "partial paste".into()
                )))
            );
            println!("{SUCCESS}");
        }
        other => panic!("invalid test stage {other:?}"),
    }
}

#[test]
fn idle_reader_wakes_for_resize_and_joins_without_input() {
    let _serial = PTY_TESTS.lock().unwrap();
    let (_master, mut slave) = pty();
    for _ in 0..20 {
        let driver = Driver::start(slave, Parser::default(), true).unwrap();
        slave = driver.freeze().unwrap().input;
    }
    let driver = Driver::start(slave, Parser::default(), true).unwrap();
    // First consume a key, proving the reader installed its native signal route.
    let mut master = _master;
    master.write_all(b"a").unwrap();
    event(&driver);
    // The master and slave share this PTY's window dimensions.
    let size = libc::winsize {
        ws_row: 45,
        ws_col: 123,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    assert_eq!(
        unsafe { libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ, &raw const size) },
        0
    );
    signal_hook::low_level::raise(signal_hook::consts::SIGWINCH).unwrap();
    assert_eq!(
        event(&driver),
        InternalEvent::Event(crossterm::event::Event::Resize(123, 45))
    );
    driver.freeze().unwrap();
}

fn subscription_event(
    source: &Subscription,
    wanted: impl Fn(&InternalEvent) -> bool,
) -> InternalEvent {
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match source.poll_where(&mut context, &wanted) {
            Poll::Ready(Some(event)) => return event.unwrap(),
            Poll::Ready(None) => panic!("subscription ended"),
            Poll::Pending => {
                assert!(Instant::now() < deadline, "subscription did not wake");
                std::thread::park_timeout(Duration::from_millis(10));
            }
        }
    }
}
#[test]
fn broker_preserves_filtered_input_across_shared_owners_pause_and_fence_rollback() {
    let _serial = PTY_TESTS.lock().unwrap();
    let (mut master, slave) = pty();
    let input = Arc::new(slave);
    let broker = Broker::new(move || input.try_clone());
    let app = broker.subscribe(true).unwrap();
    let query = broker.subscribe(true).unwrap();
    master.write_all(b"a\x1b[3;4Rb").unwrap();
    assert_eq!(
        subscription_event(&query, |event| matches!(
            event,
            InternalEvent::CursorPosition(..)
        )),
        InternalEvent::CursorPosition(3, 2)
    );
    for wanted in ['a', 'b'] {
        assert!(
            matches!(subscription_event(&app, |_| true), InternalEvent::Event(crossterm::event::Event::Key(key)) if key.code == crossterm::event::KeyCode::Char(wanted))
        );
    }
    drop(query);
    master.write_all(b"c\x1b[200~paused").unwrap();
    subscription_event(&app, |_| true);
    drop(app);
    broker.buffer_input(b" paste\x1b[201~", true).unwrap();
    let app = broker.subscribe(true).unwrap();
    assert_eq!(
        subscription_event(&app, |_| true),
        InternalEvent::Event(crossterm::event::Event::Paste("paused paste".into()))
    );
    let mut fence = broker.prepare().unwrap();
    assert!(broker.subscribe(true).is_err());
    assert!(broker.buffer_input(b"not accepted", true).is_err());
    assert!(!fence.commit_if(|| false));
    master.write_all(b"d").unwrap();
    drop(fence);
    assert!(
        matches!(subscription_event(&app, |_| true), InternalEvent::Event(crossterm::event::Event::Key(key)) if key.code == crossterm::event::KeyCode::Char('d'))
    );
    let mut fence = broker.prepare().unwrap();
    Parser::restore(fence.snapshot()).unwrap();
    assert!(fence.commit_if(|| true));
    drop(fence);
    drop(app);
    assert!(
        broker.subscribe(true).is_err(),
        "committed input never silently reopens"
    );
}
#[test]
fn rejected_reader_construction_and_terminal_failure_retain_accepted_parser_data() {
    let mut parser = Parser::default();
    parser.feed(b"accepted", true).unwrap();
    let before = parser.checkpoint().unwrap();
    assert!(Driver::resume(tempfile::tempfile().unwrap(), &mut parser, true).is_err());
    assert_eq!(parser.checkpoint().unwrap(), before);
    let (input, peer) = std::os::unix::net::UnixStream::pair().unwrap();
    input.set_nonblocking(true).unwrap();
    use std::os::fd::OwnedFd;
    let input = File::from(OwnedFd::from(input));
    let original = input.try_clone().unwrap();
    let mut driver = Driver::resume(input, &mut parser, true).unwrap();
    drop(peer);
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if matches!(
            driver.poll_available(&mut context, |_| false),
            Poll::Ready(Some(Err(_)))
        ) {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::park_timeout(Duration::from_millis(10));
    }
    assert!(driver.freeze_into(&mut parser).is_err());
    assert_eq!(parser.checkpoint().unwrap(), before);
    assert!(original.metadata().is_ok());
}

#[test]
fn filtered_query_wakes_all_waiters_and_rollback_wakes_the_original_stream() {
    let _serial = PTY_TESTS.lock().unwrap();
    let (mut master, slave) = pty();
    let input = Arc::new(slave);
    let broker = Broker::new(move || input.try_clone());
    let app = broker.subscribe(true).unwrap();
    let query = broker.subscribe(true).unwrap();
    struct Count(std::sync::atomic::AtomicUsize);
    impl Wake for Count {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }
    let a = Arc::new(Count(std::sync::atomic::AtomicUsize::new(0)));
    let q = Arc::new(Count(std::sync::atomic::AtomicUsize::new(0)));
    let aw = Waker::from(a.clone());
    let qw = Waker::from(q.clone());
    assert!(
        app.poll_where(&mut Context::from_waker(&aw), |_| true)
            .is_pending()
    );
    assert!(
        query
            .poll_where(&mut Context::from_waker(&qw), |event| matches!(
                event,
                InternalEvent::CursorPosition(..)
            ))
            .is_pending()
    );
    master.write_all(b"a").unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while a.0.load(std::sync::atomic::Ordering::SeqCst) == 0
        || q.0.load(std::sync::atomic::Ordering::SeqCst) == 0
    {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    drop(query);
    assert!(
        matches!(subscription_event(&app, |_| true), InternalEvent::Event(crossterm::event::Event::Key(key)) if key.code == crossterm::event::KeyCode::Char('a'))
    );
    let fence = broker.prepare().unwrap();
    assert!(
        app.poll_where(&mut Context::from_waker(&aw), |_| true)
            .is_pending()
    );
    let before = a.0.load(std::sync::atomic::Ordering::SeqCst);
    drop(fence);
    assert!(a.0.load(std::sync::atomic::Ordering::SeqCst) > before);
}

#[test]
fn destination_restore_keeps_partial_paste_and_retries_failed_validation() {
    let _serial = PTY_TESTS.lock().unwrap();
    let (mut master, slave) = pty();
    let input = Arc::new(slave);
    let broker = Broker::new(move || input.try_clone());
    let mut parser = Parser::default();
    parser.feed(b"a\x1b[200~before", true).unwrap();
    let snapshot = parser.checkpoint().unwrap();
    assert!(
        broker
            .restore_with(b"invalid", || panic!("invalid data reached validation"))
            .is_err()
    );
    assert!(
        broker
            .restore_with(&snapshot, || Err(std::io::Error::other(
                "terminal rejected"
            )))
            .is_err()
    );
    broker.restore_with(&snapshot, || Ok(())).unwrap();
    assert!(
        broker
            .restore_with(&snapshot, || panic!("duplicate reached validation"))
            .is_err()
    );
    let app = broker.subscribe(true).unwrap();
    assert!(
        matches!(subscription_event(&app, |_| true), InternalEvent::Event(crossterm::event::Event::Key(key)) if key.code == crossterm::event::KeyCode::Char('a'))
    );
    master.write_all(b" after\r\n\x03\x1b[201~z").unwrap();
    assert_eq!(
        subscription_event(&app, |_| true),
        InternalEvent::Event(crossterm::event::Event::Paste(
            "before after\r\n\u{3}".into()
        ))
    );
    assert!(
        matches!(subscription_event(&app, |_| true), InternalEvent::Event(crossterm::event::Event::Key(key)) if key.code == crossterm::event::KeyCode::Char('z'))
    );
}

#[test]
fn destination_restore_refuses_every_prior_input_owner() {
    let _serial = PTY_TESTS.lock().unwrap();
    let snapshot = Parser::default().checkpoint().unwrap();
    for action in 0..4 {
        let (_master, slave) = pty();
        let input = Arc::new(slave);
        let broker = Broker::new(move || input.try_clone());
        match action {
            0 => broker.buffer_input(b"", true).unwrap(),
            1 => {
                broker.discard_buffered_input().unwrap();
            }
            2 => {
                drop(broker.subscribe(true).unwrap());
            }
            3 => {
                drop(broker.prepare().unwrap());
            }
            _ => unreachable!(),
        }
        assert!(
            broker
                .restore_with(&snapshot, || panic!("late restore reached validation"))
                .is_err()
        );
    }
}
