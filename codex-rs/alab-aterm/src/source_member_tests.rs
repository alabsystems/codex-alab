// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use serde_json::Value;
use serde_json::json;
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt as _;
use tokio::io::AsyncBufReadExt as _;
use tokio::io::AsyncWriteExt as _;
use tokio::io::BufReader;
use tokio::net::UnixListener;
use tokio::sync::Notify;

#[path = "source_target_derived_tests.rs"]
mod target_derived;

const BIRTH: &str = "Fri Nov 6 10:00:00 2026";
const MEMBER: &str = "11111111111111111111111111111111";
const REGISTRATION: &str = "22222222222222222222222222222222";
const PRIVATE_PATH: &str = "/private/member-fixture/auth.json";

#[path = "source_target_retirement_tests.rs"]
mod target_retirement;

fn attempt(operation: &str) -> codex_alab_core::native_preload::Body {
    codex_alab_core::native_preload::Body {
        operation: operation.into(),
        sequence: codex_alab_core::native_preload::Sequence::parse(
            "11111111111111110000000000000001",
        )
        .unwrap(),
    }
}
trait TestMember {
    async fn test_preload(
        &self,
        operation: &str,
        borrow: Option<&core::Borrow>,
    ) -> io::Result<core::Preload>;
    async fn test_preload_use(
        &self,
        operation: &str,
        receipt: &codex_alab_core::native_source_use::UseReceipt,
    ) -> io::Result<core::Preload>;
    async fn test_ready(
        &self,
        operation: &str,
        registration: Option<&str>,
        loaded: &core::Loaded,
    ) -> io::Result<core::ReadyReceipt>;
    async fn test_ready_use(
        &self,
        operation: &str,
        registration: &str,
        loaded: &core::Loaded,
        receipt: &codex_alab_core::native_source_use::UseReceipt,
    ) -> io::Result<core::ReadyReceipt>;
}
impl TestMember for Arc<dyn core::Member> {
    async fn test_preload(
        &self,
        operation: &str,
        borrow: Option<&core::Borrow>,
    ) -> io::Result<core::Preload> {
        self.preload_owned(&attempt(operation), borrow).await
    }
    async fn test_preload_use(
        &self,
        operation: &str,
        receipt: &codex_alab_core::native_source_use::UseReceipt,
    ) -> io::Result<core::Preload> {
        self.preload_owned_use(&attempt(operation), receipt).await
    }
    async fn test_ready(
        &self,
        operation: &str,
        registration: Option<&str>,
        loaded: &core::Loaded,
    ) -> io::Result<core::ReadyReceipt> {
        match loaded {
            core::Loaded::NoNativeReader(_) => self.ready(operation, registration, loaded).await,
            _ => {
                let receipt = self
                    .ready_owned(
                        &attempt(operation),
                        registration.ok_or_else(unavailable)?,
                        loaded,
                    )
                    .await?;
                Ok(core::ReadyReceipt {
                    registration: receipt.registration().into(),
                })
            }
        }
    }
    async fn test_ready_use(
        &self,
        operation: &str,
        registration: &str,
        loaded: &core::Loaded,
        receipt: &codex_alab_core::native_source_use::UseReceipt,
    ) -> io::Result<core::ReadyReceipt> {
        let ready = self
            .ready_owned_use(&attempt(operation), registration, loaded, receipt)
            .await?;
        Ok(core::ReadyReceipt {
            registration: ready.registration().into(),
        })
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    endpoint: Endpoint,
    requests: Arc<Mutex<Vec<String>>>,
    close: Arc<Notify>,
    shutdown: tokio::sync::watch::Sender<bool>,
    server: tokio::task::JoinHandle<()>,
}
impl Fixture {
    async fn start(mutate: fn(&str, &mut Value)) -> io::Result<Self> {
        Self::start_with(mutate, None).await
    }
    async fn start_with(
        mutate: fn(&str, &mut Value),
        busy_ready: Option<&'static str>,
    ) -> io::Result<Self> {
        let dir = tempfile::tempdir()?;
        let sock = dir.path().join("aterm-1.sock");
        let token = dir.path().join("aterm-1.token");
        std::fs::write(&token, b"source-member-fixture-token\n")?;
        std::fs::set_permissions(token, std::fs::Permissions::from_mode(0o600))?;
        let listener = UnixListener::bind(&sock)?;
        let endpoint = Endpoint {
            sock,
            sid: "s-123abc".into(),
        };
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&requests);
        let close = Arc::new(Notify::new());
        let closing = Arc::clone(&close);
        let sid = endpoint.sid.clone();
        let resource = Arc::new(Mutex::new(None::<BTreeMap<String, String>>));
        let (shutdown, mut stopped) = tokio::sync::watch::channel(false);
        let server = tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            let mut first_connection = true;
            loop {
                let stream = tokio::select! {
                    _ = stopped.changed() => break,
                    accepted = listener.accept() => accepted.unwrap().0,
                };
                let recorded = Arc::clone(&recorded);
                let closing = Arc::clone(&closing);
                let sid = sid.clone();
                let mut stopped = stopped.clone();
                let resource = Arc::clone(&resource);
                let work_connection = first_connection;
                first_connection = false;
                connections.spawn(async move {
            let (read, mut write) = stream.into_split();
            let mut read = BufReader::new(read);
            let mut line = String::new();
            read.read_line(&mut line).await.unwrap();
            assert_eq!(line, "AUTH source-member-fixture-token\n");
            line.clear();
            read.read_line(&mut line).await.unwrap();
            assert_eq!(line, "version\n");
            write
                .write_all(b"OK version=member-fixture\n")
                .await
                .unwrap();
            let mut seen = BTreeMap::<String, String>::new();
            let mut prefix = None;
            let mut highwater = 0;
            let mut waited = BTreeMap::new();
            let mut refresh_replies = 0;
            loop {
                line.clear();
                let size = tokio::select! {
                    _ = closing.notified(), if work_connection => break,
                    _ = stopped.changed() => break,
                    result = read.read_line(&mut line) => result.unwrap(),
                };
                if size == 0 {
                    break;
                }
                let request_line = line.trim_end().to_owned();
                recorded.lock().unwrap().push(request_line.clone());
                let words = request_line.split_ascii_whitespace().collect::<Vec<_>>();
                assert_eq!(words[0], format!("@{sid}"));
                assert_eq!(words[1], "codex-auth");
                let action = words[2];
                let args = words[3..]
                    .iter()
                    .map(|word| word.split_once('=').unwrap())
                    .collect::<BTreeMap<_, _>>();
                let request = args["request"];
                assert_eq!(request.len(), 32);
                let current_prefix = &request[..16];
                let counter = u64::from_str_radix(&request[16..], 16).unwrap();
                assert!(counter > 0);
                assert!(
                    prefix
                        .as_ref()
                        .is_none_or(|prefix| prefix == current_prefix)
                );
                prefix = Some(current_prefix.to_owned());
                if let Some(previous) = seen.get(request) {
                    assert_eq!(previous, &request_line, "a retained RPC is immutable");
                } else {
                    assert!(counter > highwater, "wire allocation is serialized");
                    highwater = counter;
                    seen.insert(request.into(), request_line.clone());
                }
                let operation = args["operation"];
                if action == "member-target-open-close" && busy_ready == Some("DROP-OPEN-CLOSE") {
                    break;
                }
                if action == "member-refresh-use" { refresh_replies += 1; }
                if action == "member-use-begin" && args.get("member") == Some(&MEMBER)
                    && (busy_ready == Some("WAIT-BEGIN") || (busy_ready == Some("WAIT-SECOND-BEGIN") && operation.ends_with("0000000000000002")))
                {
                    let _ = stopped.changed().await;
                    break;
                }
                let mut frame = json!({"schema":1,"request":request,"operation":operation,
                    "session":{"sid":sid,"process_pid":std::process::id(),"process_birth":BIRTH,"epoch":"source-host-epoch"},
                    "member": if action == "member-open-derived" { "33333333333333333333333333333333" } else { args.get("member").copied().unwrap_or(MEMBER) }});
                let first = !waited.contains_key(action);
                waited.insert(action.to_owned(), true);
                if matches!(action, "member-ready" | "member-unavailable")
                    && first
                    && let Some(status) = busy_ready.filter(|status| status.starts_with("ERR "))
                {
                    write
                        .write_all(format!("{status}\n").as_bytes())
                        .await
                        .unwrap();
                    continue;
                }
                if action == "member-rebound-ready" && busy_ready == Some("SOURCE-CHANGED") {
                    write.write_all(b"ERR codex-auth rebound changed\n").await.unwrap();
                    continue;
                }
                let mut drop_resource_reply = false;
                let response_action = if (first && matches!(action, "member-open" | "member-preload")) || (action == "member-refresh-use" && refresh_replies == 1)
                {
                    if action == "member-open" {
                        frame["member"] = Value::Null;
                    }
                    "member-waiting"
                } else {
                    match action {
                        "member-peer" => { frame["member"] = Value::Null; }
                        "member-work-rebind" => {
                            frame["state"] = json!("bound");
                            for key in ["open_kind", "use_member", "use_operation", "capability", "assignment", "context"] {
                                if let Some(value) = args.get(key) { frame[key] = json!(value); }
                            }
                        }
                        "member-target-open-close" => {
                            frame["member"] = args.get("member").map(|member| json!(member)).unwrap_or(Value::Null);
                            frame["state"] = json!("closed");
                            for key in ["open_operation", "assignment", "context"] {
                                frame[key] = json!(args[key]);
                            }
                        }
                        "member-target-resource-ready" => {
                            let body = args.iter().filter(|(key, _)| **key != "request")
                                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                                .collect::<BTreeMap<_, _>>();
                            let mut retained = resource.lock().unwrap();
                            if let Some(previous) = retained.as_ref() {
                                assert_eq!(previous, &body, "semantic resource retry retains its full body");
                            } else {
                                *retained = Some(body);
                                drop_resource_reply = busy_ready == Some("DROP-RESOURCE-REPLY");
                            }
                            frame["registration"] = json!(REGISTRATION);
                            frame["resource_receipt"] = json!("9".repeat(64));
                        }
                        "member-target-ack" => {
                            assert_eq!(args["resource_receipt"], "9".repeat(64));
                            let retained = resource.lock().unwrap();
                            let retained = retained.as_ref().unwrap();
                            frame["member"] = Value::Null;
                            frame["phase"] = json!("applied");
                            frame["boundary"] = json!(retained["boundary"]);
                            for key in ["assignment_generation", "credential_generation", "auth_generation", "owner_generation", "source_generation", "login_generation"] {
                                frame[key] = json!(retained[key].parse::<u64>().unwrap());
                            }
                            for key in ["digest", "account", "user"] {
                                frame[key] = json!(retained[key]);
                            }
                        }
                        "member-target-context" => {
                            frame["member"] = Value::Null;
                            frame["context"] = json!("c".repeat(64));
                            frame["assignment_generation"] = json!(5);
                            frame["target_ref"] = json!("d".repeat(64));
                        }
                        "member-preload" | "member-preload-use" | "member-preload-rebound" | "member-preload-target"
                        | "member-preload-resample" | "member-preload-use-resample" | "member-preload-target-resample" => {
                            if action.ends_with("resample") { frame["state"] = json!("preload"); }
                            frame["registration"] = json!(REGISTRATION);
                            frame["source"] = json!({"schema":1,"operation":operation,"session":frame["session"],
                                "slot":"736c6f742f6d656d626572","path":PRIVATE_PATH,"account":"a".repeat(64),"user":"b".repeat(64),
                                "credential_generation":41,"digest":"d".repeat(64),"assignment_generation":5,"native_intent":"e".repeat(32)});
                        }
                        "member-ready" | "member-ready-use" | "member-rebound-ready"
                        | "member-reader-ready" | "member-reader-ready-use"
                        | "member-ready-target" | "member-reader-ready-target"
                        | "member-unavailable-target" | "member-unavailable-use"
                        | "member-unavailable"
                        | "member-unrelated" => frame["registration"] = json!(REGISTRATION),
                        "member-refresh-use" => {
                            frame["slot"] = json!(args["slot"]);
                            frame["expected_digest"] = json!(args["expected_digest"]);
                            frame["state"] = json!(if refresh_replies == 2 { "waiting" } else { "ready" });
                            frame["generation"] = json!(if refresh_replies == 2 { 41 } else { 42 });
                            frame["digest"] = json!(if refresh_replies == 2 { "d".repeat(64) } else { "f".repeat(64) });
                        }
                        "member-use-begin" => {
                            frame["state"] = json!("active");
                            frame["capability"] = json!(operation.repeat(2));
                        }
                        "member-use-end" | "member-use-close" => {
                            frame["state"] = json!("closed");
                            frame["capability"] = Value::Null;
                        }
                        "member-open" | "member-open-target" | "member-open-derived" | "member-retire" | "member-retire-owned" => {}
                        _ => panic!("unexpected member action"),
                    }
                    match action {
                        "member-use-begin" | "member-use-end" | "member-use-close" => "member-use",
                        "member-refresh-use" => "member-refresh",
                        "member-open-derived" => "member-open",
                        "member-preload-use" => "member-preload",
                        "member-ready-use" => "member-ready",
                        "member-reader-ready-use" => "member-reader-ready",
                        "member-unavailable-use" => "member-unavailable",
                        _ => action,
                    }
                };
                // Generic Waiting is the closed common frame, not a completed
                // PRELOAD/READY/retirement receipt. Echo the owned sequence only
                // once this fixture actually returns the settled action.
                if response_action != "member-waiting"
                    && let Some(sequence) = args.get("preload_sequence")
                {
                    frame["preload_sequence"] = json!(sequence);
                }
                let response_action = if action == "member-rebound-ready" && busy_ready == Some("OLD-READY-PREFIX") {
                    "member-ready"
                } else { response_action };
                mutate(response_action, &mut frame);
                if drop_resource_reply { break; }
                if write
                    .write_all(format!("OK codex-auth {response_action} {frame}\n").as_bytes())
                    .await
                    .is_err()
                {
                    break;
                }
            }
                });
            }
            while let Some(result) = connections.join_next().await {
                result.unwrap();
            }
        });
        Ok(Self {
            _dir: dir,
            endpoint,
            requests,
            close,
            shutdown,
            server,
        })
    }
    async fn finish(self) {
        self.shutdown.send(true).unwrap();
        self.server.await.unwrap();
    }
}

#[test]
fn generic_waiting_refuses_settled_preload_sequence_authority() {
    let mut frame = json!({
        "schema": 1,
        "request": "1".repeat(32),
        "operation": "2".repeat(32),
        "session": {
            "sid": "s-123abc", "process_pid": std::process::id(),
            "process_birth": BIRTH, "epoch": "source-host-epoch"
        },
        "member": MEMBER
    });
    assert!(serde_json::from_value::<WaitingFrame>(frame.clone()).is_ok());
    frame["preload_sequence"] = json!("11111111111111110000000000000001");
    assert!(serde_json::from_value::<WaitingFrame>(frame).is_err());
}

async fn preload_reader(member: &Arc<dyn core::Member>) -> io::Result<()> {
    let preload = member.test_preload(&"b".repeat(32), None).await?;
    member
        .test_ready(
            &preload.source.operation,
            Some(&preload.registration),
            &core::Loaded::ReaderSource {
                source: Box::new(preload.source.clone()),
            },
        )
        .await?;
    Ok(())
}

#[tokio::test]
async fn member_channel_keeps_authenticated_transport_and_exact_waiting_requests() -> io::Result<()>
{
    let fixture = Fixture::start(|_, _| {}).await?;
    let member = open_at(&fixture.endpoint, &"a".repeat(32), BIRTH).await?;
    assert!(member.live());
    let preload = member.test_preload(&"b".repeat(32), None).await?;
    assert_eq!(preload.source.revision.generation, 41);
    assert_eq!(preload.source.assignment_generation, 5);
    let loaded = core::Loaded::Source {
        source: Box::new(preload.source.clone()),
        generations: codex_alab_core::assignment::Generations {
            auth: 13,
            owner: 2,
            source: 4,
            login: 7,
        },
    };
    let ready = member
        .test_ready(&"b".repeat(32), Some(&preload.registration), &loaded)
        .await?;
    assert_eq!(ready.registration, REGISTRATION);
    assert!(member.live(), "Ready must retain the native connection");
    let requests = fixture.requests.lock().unwrap().clone();
    assert_eq!(
        requests.len(),
        6,
        "peer hello precedes open; open and preload each wait once"
    );
    assert!(requests[0].contains(" codex-auth member-peer "));
    assert_eq!(requests[1], requests[2]);
    assert_eq!(requests[3], requests[4]);
    assert!(
        requests[5].contains(
            "auth_generation=13 owner_generation=2 source_generation=4 login_generation=7"
        )
    );
    assert!(!requests.iter().any(|line| line.contains(PRIVATE_PATH)
        || line.contains("supervisor")
        || line.contains("harness")));
    member.retire(Some(&ready.registration)).await?;
    assert!(!member.live());
    fixture.finish().await;
    Ok(())
}

#[tokio::test]
async fn unrelated_completion_has_a_real_registration_and_no_dummy_source() -> io::Result<()> {
    let fixture = Fixture::start(|_, _| {}).await?;
    let operation = "a".repeat(32);
    let member = open_at(&fixture.endpoint, &operation, BIRTH).await?;
    let receipt = member
        .test_ready(
            &operation,
            None,
            &core::Loaded::NoNativeReader(core::NoNativeReader::ApiKey),
        )
        .await?;
    assert_eq!(receipt.registration, REGISTRATION);
    let count = fixture.requests.lock().unwrap().len();
    assert!(member.test_preload(&"b".repeat(32), None).await.is_err());
    assert_eq!(
        fixture.requests.lock().unwrap().len(),
        count,
        "a kind change needs a new issued member"
    );
    assert!(
        fixture
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|line| line.contains("member-unrelated") && line.ends_with("kind=api_key"))
    );
    assert!(!fixture.requests.lock().unwrap().iter().any(|line| {
        line.contains("credential_generation=0") || line.contains("member-unavailable")
    }));
    member.retire(Some(&receipt.registration)).await?;
    fixture.finish().await;
    Ok(())
}

#[tokio::test]
async fn idle_eof_revokes_member_liveness_without_another_request() -> io::Result<()> {
    let fixture = Fixture::start(|_, _| {}).await?;
    let member = open_at(&fixture.endpoint, &"a".repeat(32), BIRTH).await?;
    assert!(member.live());
    let original_work = member.work_incarnation();
    fixture.close.notify_one();
    tokio::time::timeout(Duration::from_secs(2), async {
        while member.live() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("idle EOF must be observed");
    // EOF revokes the old connection. A later read may authenticate an
    // existing-work rebind, but must use its fresh transport incarnation.
    let operation = "b".repeat(32);
    let preload = member.test_preload(&operation, None).await?;
    assert_ne!(member.work_incarnation(), original_work);
    assert_eq!(preload.source.operation, operation);
    fixture.finish().await;
    Ok(())
}

#[cfg(debug_assertions)]
#[tokio::test]
async fn debug_eof_fault_closes_only_its_actual_member_and_normal_transport_rebinds()
-> io::Result<()> {
    let first = Fixture::start(|_, _| {}).await?;
    let second = Fixture::start(|_, _| {}).await?;
    let member = open_at(&first.endpoint, &"a".repeat(32), BIRTH).await?;
    let unrelated = open_at(&second.endpoint, &"b".repeat(32), BIRTH).await?;
    let incarnation = member.work_incarnation();
    member.debug_close_work()?;
    tokio::time::timeout(Duration::from_secs(2), member.closed())
        .await
        .expect("owned debug socket did not close")?;
    assert!(!member.live());
    assert!(
        unrelated.live(),
        "fault must not close another member's socket"
    );
    member.test_preload(&"c".repeat(32), None).await?;
    assert_ne!(member.work_incarnation(), incarnation);
    assert!(
        first
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|line| line.contains("member-work-rebind"))
    );
    assert!(unrelated.live());
    first.finish().await;
    second.finish().await;
    Ok(())
}

#[tokio::test]
async fn changed_epoch_and_unknown_fields_close_the_private_channel_without_echoing_data()
-> io::Result<()> {
    let fixture = Fixture::start(|action, frame| {
        if action == "member-preload" {
            frame["session"]["epoch"] = json!("replacement-host");
        }
    })
    .await?;
    let member = open_at(&fixture.endpoint, &"a".repeat(32), BIRTH).await?;
    let error = member
        .test_preload(&"b".repeat(32), None)
        .await
        .err()
        .unwrap()
        .to_string();
    assert!(!member.live());
    assert!(!error.contains(PRIVATE_PATH));
    fixture.finish().await;
    let fixture = Fixture::start(|action, frame| {
        if action == "member-open" {
            frame["private-canary"] = json!(PRIVATE_PATH);
        }
    })
    .await?;
    let error = open_at(&fixture.endpoint, &"a".repeat(32), BIRTH)
        .await
        .err()
        .unwrap()
        .to_string();
    assert!(!error.contains(PRIVATE_PATH));
    fixture.finish().await;
    Ok(())
}

#[test]
fn connection_nonce_sequence_is_monotone_and_never_wraps() {
    let mut sequence = Sequence {
        prefix: 0x1234,
        next: Some(u64::MAX - 1),
    };
    let one = sequence.allocate().unwrap();
    let two = sequence.allocate().unwrap();
    assert_eq!(&one[..16], &two[..16]);
    assert!(one < two);
    assert!(sequence.allocate().is_err());
}

#[tokio::test]
async fn terminal_busy_uses_a_new_counter_for_the_exact_same_semantic_action() -> io::Result<()> {
    let fixture = Fixture::start_with(|_, _| {}, Some("ERR codex-auth busy")).await?;
    let member = open_at(&fixture.endpoint, &"a".repeat(32), BIRTH).await?;
    let preload = member.test_preload(&"b".repeat(32), None).await?;
    let loaded = core::Loaded::Source {
        source: Box::new(preload.source),
        generations: codex_alab_core::assignment::Generations {
            auth: 13,
            owner: 2,
            source: 4,
            login: 7,
        },
    };
    member
        .test_ready(&"b".repeat(32), Some(&preload.registration), &loaded)
        .await?;
    let requests = fixture
        .requests
        .lock()
        .unwrap()
        .iter()
        .filter(|line| line.contains(" member-ready "))
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(requests.len(), 2);
    let semantic = |line: &str| {
        line.split_ascii_whitespace()
            .filter(|word| !word.starts_with("request="))
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    assert_ne!(requests[0], requests[1]);
    assert_eq!(semantic(&requests[0]), semantic(&requests[1]));
    member.retire(Some(&preload.registration)).await?;
    fixture.finish().await;
    Ok(())
}

#[tokio::test]
async fn oversized_private_status_is_refused_before_accepting_a_member() -> io::Result<()> {
    let fixture = Fixture::start(|action, frame| {
        if action == "member-open" {
            frame["private-canary"] = json!("x".repeat(MAX_FRAME));
        }
    })
    .await?;
    let error = open_at(&fixture.endpoint, &"a".repeat(32), BIRTH)
        .await
        .err()
        .unwrap()
        .to_string();
    assert!(!error.contains("private-canary"));
    assert!(!error.contains(PRIVATE_PATH));
    fixture.finish().await;
    Ok(())
}

#[tokio::test]
async fn malformed_busy_suffix_is_not_retried_or_exposed() -> io::Result<()> {
    let fixture =
        Fixture::start_with(|_, _| {}, Some("ERR codex-auth busy private-canary")).await?;
    let member = open_at(&fixture.endpoint, &"a".repeat(32), BIRTH).await?;
    let preload = member.test_preload(&"b".repeat(32), None).await?;
    let error = member
        .test_ready(
            &"b".repeat(32),
            Some(&preload.registration),
            &core::Loaded::PolicyUnavailable,
        )
        .await
        .unwrap_err();
    assert!(!error.to_string().contains("private-canary"));
    assert_eq!(
        fixture
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|line| line.contains(" member-unavailable "))
            .count(),
        1
    );
    drop(member);
    fixture.finish().await;
    Ok(())
}

#[tokio::test]
async fn concurrent_readiness_updates_serialize_nonce_allocation_with_wire_order() -> io::Result<()>
{
    let fixture = Fixture::start(|_, _| {}).await?;
    let member = open_at(&fixture.endpoint, &"a".repeat(32), BIRTH).await?;
    let operation = "b".repeat(32);
    let preload = member.test_preload(&operation, None).await?;
    let loaded = core::Loaded::Source {
        source: Box::new(preload.source),
        generations: codex_alab_core::assignment::Generations {
            auth: 13,
            owner: 2,
            source: 4,
            login: 7,
        },
    };
    let (first, second) = tokio::join!(
        member.test_ready(&operation, Some(&preload.registration), &loaded),
        member.test_ready(&operation, Some(&preload.registration), &loaded)
    );
    first?;
    second?;
    assert_eq!(
        fixture
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|line| line.contains(" member-ready "))
            .count(),
        2
    );
    member.retire(Some(&preload.registration)).await?;
    fixture.finish().await;
    Ok(())
}

#[tokio::test]
async fn direct_reader_ready_has_no_manager_counters() -> io::Result<()> {
    let fixture = Fixture::start(|_, _| {}).await?;
    let member = open_at(&fixture.endpoint, &"a".repeat(32), BIRTH).await?;
    let preload = member.test_preload(&"b".repeat(32), None).await?;
    let ready = member
        .test_ready(
            &"b".repeat(32),
            Some(&preload.registration),
            &core::Loaded::ReaderSource {
                source: Box::new(preload.source),
            },
        )
        .await?;
    let requests = fixture.requests.lock().unwrap().clone();
    let request = requests.last().unwrap();
    assert!(request.contains("member-reader-ready"));
    assert!(request.contains("credential_generation=41"));
    for field in [
        "auth_generation=",
        "owner_generation=",
        "source_generation=",
        "login_generation=",
    ] {
        assert!(!request.contains(field));
    }
    member.retire(Some(&ready.registration)).await?;
    fixture.finish().await;
    Ok(())
}

#[test]
fn transport_failures_are_typed_and_never_expose_wire_or_io_text() {
    for (error, kind) in [
        (crate::Error::Closed, io::ErrorKind::ConnectionReset),
        (crate::Error::Unanswered, io::ErrorKind::TimedOut),
        (
            crate::Error::Refused("private-cookie".into()),
            io::ErrorKind::PermissionDenied,
        ),
        (
            crate::Error::Malformed("private-frame".into()),
            io::ErrorKind::InvalidData,
        ),
        (
            crate::Error::Io(io::Error::new(io::ErrorKind::BrokenPipe, "private-path")),
            io::ErrorKind::BrokenPipe,
        ),
    ] {
        let mapped = transport_error(error);
        assert_eq!(mapped.kind(), kind);
        assert!(!mapped.to_string().contains("private-"));
    }
}

const WEBSOCKET_CHILD: &str = "ALAB_NATIVE_SOURCE_WEBSOCKET_CHILD";
#[test]
fn native_websocket_holds_source_activity_and_observes_real_member_eof() {
    let test = concat!(module_path!(), "::native_websocket_child")
        .split_once("::")
        .unwrap()
        .1;
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test])
        .env(WEBSOCKET_CHILD, "1")
        .env(core::LAUNCH_ENV, "fixture-native-source-constructor")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
}
struct WebSocketHost(Endpoint);
impl core::Host for WebSocketHost {
    fn source<'a>(&'a self, _: &'a str) -> core::SourceFuture<'a> {
        Box::pin(async { Err(unavailable()) })
    }
    fn open_member<'a>(&'a self, operation: &'a str) -> core::MemberFuture<'a> {
        Box::pin(async move { open_at(&self.0, operation, BIRTH).await })
    }
}
#[tokio::test]
async fn native_websocket_child() -> io::Result<()> {
    if std::env::var_os(WEBSOCKET_CHILD).is_none() {
        return Ok(());
    }
    use codex_alab_core::native_source_lifetime::Lifetime;
    use codex_http_client::HttpClientFactory;
    use codex_http_client::NetworkPolicy;
    use codex_websocket_client::WebSocketConnector;
    use codex_websocket_client::WebSocketTlsMode;
    use futures::SinkExt;
    use futures::StreamExt;
    use tokio_tungstenite::tungstenite::Message;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
    let fixture = Fixture::start(|_, _| {}).await?;
    core::install(Arc::new(WebSocketHost(fixture.endpoint.clone())));
    codex_alab_core::native_source_launch::enter().await?;
    let lifetime = Lifetime::new(
        codex_alab_core::native_source_launch::allocate_manager()
            .await?
            .unwrap(),
    )?;
    let domain = Arc::new(codex_alab_core::assignment::Domain::default());
    lifetime.attach_domain(domain.clone())?;
    let preload = lifetime.preload(&"b".repeat(32), None).await?;
    lifetime.reader_ready(&preload.source).await?;
    let factory = HttpClientFactory::new(codex_http_client::OutboundProxyPolicy::ReqwestDefault)
        .with_network_policy(
            NetworkPolicy::unmanaged().with_native_source(Ok(lifetime.network_scope()?)),
        );
    let connector =
        WebSocketConnector::new_with_tls_mode(&factory, WebSocketTlsMode::TungsteniteDefault)
            .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("ws://{}", listener.local_addr()?);
    let echo = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
        while let Some(Ok(message)) = socket.next().await {
            if socket.send(message).await.is_err() {
                break;
            }
        }
    });
    let (mut socket, _) = connector
        .connect_loopback_direct(
            url.into_client_request().unwrap(),
            WebSocketConfig::default(),
        )
        .await
        .unwrap();
    socket
        .send(Message::Text("source-lease".into()))
        .await
        .unwrap();
    assert_eq!(
        socket.next().await.unwrap().unwrap(),
        Message::Text("source-lease".into())
    );
    let operation = "a".repeat(32);
    domain
        .request(codex_alab_core::assignment::Intent {
            operation: operation.clone(),
            thread: "00000000-0000-0000-0000-000000000001".into(),
            selection: codex_alab_core::assignment::Selection::Auto {
                revision: "a".repeat(64),
            },
        })
        .unwrap();
    let retiring = domain.try_retire(&operation).unwrap();
    let retiring = match retiring.freeze() {
        Err(codex_alab_core::assignment::FreezeError::Pending(retiring)) => retiring,
        _ => panic!("real WebSocket must retain actual source activity"),
    };
    fixture.close.notify_one();
    let error = tokio::time::timeout(Duration::from_secs(10), socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert_eq!(
        codex_websocket_client::network_policy_denial(&error),
        Some(codex_http_client::NetworkPolicyDenied::Revoked)
    );
    assert!(!lifetime.live());
    drop(socket);
    let _boundary = tokio::time::timeout(Duration::from_secs(10), retiring.drained())
        .await
        .expect("native End acknowledgement must settle after work EOF")
        .expect("idle factory and connector have no activity lease");
    assert!(
        fixture
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|request| request.contains(" member-use-end ")),
        "local zero and EOF do not replace native End acknowledgement"
    );
    assert!(
        !fixture
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|request| request.contains("member-retire")),
        "EOF never fabricates retirement"
    );
    tokio::time::timeout(Duration::from_secs(10), echo)
        .await
        .unwrap()
        .unwrap();
    fixture.finish().await;
    Ok(())
}

#[tokio::test]
async fn native_use_cleanup_authenticates_a_separate_connection_after_work_eof() -> io::Result<()> {
    use codex_alab_core::native_source_use::UseAttempt;
    use codex_alab_core::native_source_use::UseOutcome;
    let fixture = Fixture::start(|_, _| {}).await?;
    let member = open_at(&fixture.endpoint, &"a".repeat(32), BIRTH).await?;
    preload_reader(&member).await?;
    let attempt = UseAttempt::default();
    let receipt = match member.begin_use(&attempt, REGISTRATION).await? {
        UseOutcome::Active(receipt) => receipt,
        _ => panic!("expected an admitted native use"),
    };
    attempt.finish_admission()?;
    fixture.close.notify_one();
    tokio::time::timeout(Duration::from_secs(2), member.closed()).await??;
    assert!(!member.live());
    member.end_use(&receipt).await?;
    let requests = fixture.requests.lock().unwrap().clone();
    let begin = requests
        .iter()
        .find(|line| line.contains(" member-use-begin "))
        .unwrap();
    let end = requests
        .iter()
        .find(|line| line.contains(" member-use-end "))
        .unwrap();
    let fields = |line: &str| {
        line.split_ascii_whitespace()
            .filter_map(|word| word.split_once('='))
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect::<BTreeMap<_, _>>()
    };
    let begin = fields(begin);
    let end = fields(end);
    assert_eq!(begin["operation"], end["operation"]);
    assert_ne!(
        &begin["request"][..16],
        &end["request"][..16],
        "cleanup authenticates a fresh transport"
    );
    assert_eq!(end["capability"], receipt.private_capability());
    assert!(
        !end.contains_key("registration"),
        "cleanup does not need current Ready"
    );
    fixture.finish().await;
    Ok(())
}

#[tokio::test]
async fn contextual_preload_and_reader_completion_retain_the_actual_parent_use() -> io::Result<()> {
    use codex_alab_core::native_source_use::UseAttempt;
    use codex_alab_core::native_source_use::UseOutcome;
    let fixture = Fixture::start(|_, _| {}).await?;
    let member = open_at(&fixture.endpoint, &"a".repeat(32), BIRTH).await?;
    let initial = member.test_preload(&"b".repeat(32), None).await?;
    member
        .test_ready(
            &initial.source.operation,
            Some(&initial.registration),
            &core::Loaded::ReaderSource {
                source: Box::new(initial.source.clone()),
            },
        )
        .await?;
    let attempt = UseAttempt::default();
    let receipt = match member.begin_use(&attempt, REGISTRATION).await? {
        UseOutcome::Active(receipt) => receipt,
        _ => panic!("expected a real admitted parent use"),
    };
    attempt.finish_admission()?;
    let next = member.test_preload_use(&"c".repeat(32), &receipt).await?;
    member
        .test_ready_use(
            &next.source.operation,
            &next.registration,
            &core::Loaded::ReaderSource {
                source: Box::new(next.source.clone()),
            },
            &receipt,
        )
        .await?;
    for action in ["member-preload-use", "member-reader-ready-use"] {
        let requests = fixture.requests.lock().unwrap();
        let line = requests
            .iter()
            .find(|line| line.contains(&format!(" {action} ")))
            .unwrap();
        assert!(line.contains(&format!(
            " use_member={} use_operation={} capability={}",
            receipt.member(),
            receipt.operation(),
            receipt.private_capability()
        )));
        assert!(
            !line.contains("auth_generation="),
            "reader completion invents no manager state"
        );
    }
    member.end_use(&receipt).await?;
    fixture.finish().await;
    Ok(())
}

#[tokio::test]
async fn derived_member_keeps_its_own_incarnation_and_transport_after_parent_retirement()
-> io::Result<()> {
    use codex_alab_core::native_source_use::UseAttempt;
    use codex_alab_core::native_source_use::UseOutcome;
    let fixture = Fixture::start(|_, _| {}).await?;
    let parent = open_at(&fixture.endpoint, &"a".repeat(32), BIRTH).await?;
    preload_reader(&parent).await?;
    let attempt = UseAttempt::default();
    let receipt = match parent.begin_use(&attempt, REGISTRATION).await? {
        UseOutcome::Active(receipt) => receipt,
        _ => panic!("expected actual parent use"),
    };
    attempt.finish_admission()?;
    let child = parent.open_derived(&"c".repeat(32), &receipt).await?;
    assert_ne!(child.id(), parent.id());
    let preload = child.test_preload_use(&"d".repeat(32), &receipt).await?;
    child
        .test_ready_use(
            &preload.source.operation,
            &preload.registration,
            &core::Loaded::ReaderSource {
                source: Box::new(preload.source.clone()),
            },
            &receipt,
        )
        .await?;
    parent.end_use(&receipt).await?;
    parent.retire(None).await?;
    assert!(!parent.live());
    assert!(
        child.live(),
        "retiring one native member must not close its derived sibling's transport"
    );
    child.retire(Some(&preload.registration)).await?;
    assert!(!child.live());
    fixture.finish().await;
    Ok(())
}

const PENDING_BEGIN_CHILD: &str = "ALAB_NATIVE_PENDING_BEGIN_CHILD";
#[test]
fn ordinary_policy_revocation_interrupts_a_pending_native_begin_without_transport_effect() {
    let test = concat!(module_path!(), "::pending_native_begin_child")
        .split_once("::")
        .unwrap()
        .1;
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test])
        .env(PENDING_BEGIN_CHILD, "1")
        .env(core::LAUNCH_ENV, "fixture-native-source-constructor")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
}
#[tokio::test]
async fn pending_native_begin_child() -> io::Result<()> {
    if std::env::var_os(PENDING_BEGIN_CHILD).is_none() {
        return Ok(());
    }
    use codex_http_client::DestinationPolicy;
    use codex_http_client::NetworkPolicyController;
    use codex_http_client::NetworkPolicyDenied;
    let fixture = Fixture::start_with(|_, _| {}, Some("WAIT-BEGIN")).await?;
    core::install(Arc::new(WebSocketHost(fixture.endpoint.clone())));
    codex_alab_core::native_source_launch::enter().await?;
    let lifetime = codex_alab_core::native_source_lifetime::Lifetime::new(
        codex_alab_core::native_source_launch::allocate_manager()
            .await?
            .unwrap(),
    )?;
    let domain = Arc::new(codex_alab_core::assignment::Domain::default());
    lifetime.attach_domain(domain.clone())?;
    let loaded = lifetime.preload(&"b".repeat(32), None).await?;
    lifetime.reader_ready(&loaded.source).await?;
    let controller = NetworkPolicyController::default();
    let policy = controller.policy();
    let revision = policy.revision();
    assert!(controller.publish(revision, DestinationPolicy::Unrestricted));
    let policy = policy.with_native_source(Ok(lifetime.network_scope()?));
    let permit = policy
        .acquire(&"https://example.com/".parse().unwrap())
        .unwrap();
    let effects = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = effects.clone();
    let pending = tokio::spawn(async move {
        permit
            .run(async {
                observed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            })
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if fixture
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|line| line.contains(" member-use-begin "))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    controller.unavailable(revision);
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), pending)
            .await
            .unwrap()
            .unwrap(),
        Err(NetworkPolicyDenied::Revoked)
    );
    assert_eq!(
        effects.load(std::sync::atomic::Ordering::Relaxed),
        0,
        "transport future must never be polled before Begin ACK"
    );
    let operation = "a".repeat(32);
    domain
        .request(codex_alab_core::assignment::Intent {
            operation: operation.clone(),
            thread: "00000000-0000-0000-0000-000000000001".into(),
            selection: codex_alab_core::assignment::Selection::Auto {
                revision: "a".repeat(64),
            },
        })
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(2),
        domain.try_retire(&operation).unwrap().drained(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        fixture
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|line| line.contains(" member-use-close ")),
        "pending Begin is owned until native Close ACK"
    );
    fixture.finish().await;
    Ok(())
}

#[tokio::test]
async fn owned_retirement_uses_exact_attempt_and_fresh_cleanup_after_work_eof() -> io::Result<()> {
    let fixture = Fixture::start(|_, _| {}).await?;
    let member = open_at(&fixture.endpoint, &"a".repeat(32), BIRTH).await?;
    preload_reader(&member).await?;
    fixture.close.notify_one();
    tokio::time::timeout(Duration::from_secs(2), member.closed()).await??;
    let request = core::Retirement {
        operation: "d".repeat(32),
        preload_operation: Some("c".repeat(32)),
        preload_sequence: Some(attempt(&"c".repeat(32)).sequence),
        registration: None,
    };
    member.retire_captured(&request).await?;
    {
        let requests = fixture.requests.lock().unwrap();
        let line = requests
            .iter()
            .find(|line| line.contains(" member-retire-owned "))
            .unwrap();
        assert!(line.contains(&format!(
            " operation={} member={MEMBER} preload_operation={} preload_sequence=11111111111111110000000000000001 registration=none",
            request.operation,
            "c".repeat(32)
        )));
        assert!(
            !line.contains("slot=") && !line.contains("digest="),
            "retirement does not read or refresh A"
        );
    }
    fixture.finish().await;
    Ok(())
}
#[tokio::test]
async fn unknown_retirement_fields_do_not_fabricate_closed_or_close_other_members() -> io::Result<()>
{
    let fixture = Fixture::start(|action, frame| {
        if action == "member-retire-owned" {
            frame["closed"] = json!(true);
        }
    })
    .await?;
    let member = open_at(&fixture.endpoint, &"a".repeat(32), BIRTH).await?;
    preload_reader(&member).await?;
    assert!(
        member
            .retire_captured(&core::Retirement {
                operation: "d".repeat(32),
                preload_operation: Some("b".repeat(32)),
                preload_sequence: Some(attempt(&"b".repeat(32)).sequence),
                registration: Some(REGISTRATION.into())
            })
            .await
            .is_err()
    );
    assert!(
        member.live(),
        "only exact native Closed success updates retirement state"
    );
    fixture.finish().await;
    Ok(())
}
#[tokio::test]
async fn derived_child_progresses_after_parent_close_ack_with_old_work_reply_withheld()
-> io::Result<()> {
    use codex_alab_core::native_source_use::UseAttempt;
    use codex_alab_core::native_source_use::UseOutcome;
    let fixture = Fixture::start_with(|_, _| {}, Some("WAIT-SECOND-BEGIN")).await?;
    let parent = open_at(&fixture.endpoint, &"a".repeat(32), BIRTH).await?;
    preload_reader(&parent).await?;
    let first = UseAttempt::default();
    let first_receipt = match parent.begin_use(&first, REGISTRATION).await? {
        UseOutcome::Active(receipt) => receipt,
        _ => panic!("actual parent use required"),
    };
    first.finish_admission()?;
    let child = parent.open_derived(&"c".repeat(32), &first_receipt).await?;
    let loaded = child
        .test_preload_use(&"d".repeat(32), &first_receipt)
        .await?;
    child
        .test_ready_use(
            &loaded.source.operation,
            &loaded.registration,
            &core::Loaded::ReaderSource {
                source: Box::new(loaded.source.clone()),
            },
            &first_receipt,
        )
        .await?;
    let second = Arc::new(UseAttempt::default());
    let second_task = second.clone();
    let parent_task = parent.clone();
    let pending =
        tokio::spawn(async move { parent_task.begin_use(&second_task, REGISTRATION).await });
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if second.body().unwrap().is_some() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let body = second.body()?.unwrap();
    assert!(matches!(parent.close_use(&body).await?, UseOutcome::Closed));
    pending.abort(); // Only after the correlated native Closed proof.
    let _ = pending.await;
    second.finish_admission()?;
    let child_attempt = UseAttempt::default();
    let child_receipt = match tokio::time::timeout(
        Duration::from_secs(2),
        child.begin_use(&child_attempt, REGISTRATION),
    )
    .await
    .unwrap()?
    {
        UseOutcome::Active(receipt) => receipt,
        _ => panic!("child has an independent work channel"),
    };
    child_attempt.finish_admission()?;
    child.end_use(&child_receipt).await?;
    parent.end_use(&first_receipt).await?;
    assert!(
        parent.live() && child.live(),
        "cleanup does not close either work channel"
    );
    fixture.finish().await;
    Ok(())
}

#[tokio::test]
async fn contextual_refresh_preserves_canonical_action_across_unknown_and_known_waiting()
-> io::Result<()> {
    use codex_alab_core::native_source_use::UseAttempt;
    use codex_alab_core::native_source_use::UseOutcome;
    let fixture = Fixture::start(|_, _| {}).await?;
    let member = open_at(&fixture.endpoint, &"a".repeat(32), BIRTH).await?;
    preload_reader(&member).await?;
    let attempt = UseAttempt::default();
    let receipt = match member.begin_use(&attempt, REGISTRATION).await? {
        UseOutcome::Active(receipt) => receipt,
        _ => panic!("actual native use required"),
    };
    attempt.finish_admission()?;
    let request = codex_alab_core::managed_auth::Request {
        slot: "736c6f742f6d656d626572".into(),
        operation: "e".repeat(32),
        expected_digest: "d".repeat(64),
    };
    let mut wrong = request.clone();
    wrong.slot = "6f74686572".into();
    assert!(member.refresh_use(&wrong, &receipt).await.is_err());
    let revision = member.refresh_use(&request, &receipt).await?;
    assert_eq!(revision.generation, 42);
    assert_eq!(revision.digest, "f".repeat(64));
    {
        let all = fixture.requests.lock().unwrap();
        let refresh = all
            .iter()
            .filter(|line| line.contains(" member-refresh-use "))
            .collect::<Vec<_>>();
        assert_eq!(refresh.len(), 3);
        assert_eq!(
            refresh[0], refresh[1],
            "unknown transport waiting retries exact RPC"
        );
        assert_ne!(
            refresh[1], refresh[2],
            "known semantic waiting gets a fresh RPC receipt"
        );
        for line in refresh {
            assert!(line.contains(&format!(" operation={} member={MEMBER} slot={} expected_digest={} use_member={MEMBER} use_operation={} capability={}", request.operation, request.slot, request.expected_digest, receipt.operation(), receipt.private_capability())));
            assert!(!line.contains("refresh_token"));
        }
    }
    member.end_use(&receipt).await?;
    fixture.finish().await;
    Ok(())
}
#[tokio::test]
async fn contextual_refresh_rejects_unknown_zero_and_backward_revisions() -> io::Result<()> {
    use codex_alab_core::native_source_use::UseAttempt;
    use codex_alab_core::native_source_use::UseOutcome;
    let mutations: [fn(&str, &mut Value); 4] = [
        |action, frame| {
            if action == "member-refresh" {
                frame["generation"] = json!(0);
            }
        },
        |action, frame| {
            if action == "member-refresh" {
                frame["unexpected"] = json!(true);
            }
        },
        |action, frame| {
            if action == "member-refresh" && frame["state"] == "ready" {
                frame["generation"] = json!(40);
            }
        },
        |action, frame| {
            if action == "member-refresh" && frame["state"] == "ready" {
                frame["generation"] = json!(41);
            }
        },
    ];
    for mutate in mutations {
        let fixture = Fixture::start(mutate).await?;
        let member = open_at(&fixture.endpoint, &"a".repeat(32), BIRTH).await?;
        preload_reader(&member).await?;
        let attempt = UseAttempt::default();
        let receipt = match member.begin_use(&attempt, REGISTRATION).await? {
            UseOutcome::Active(receipt) => receipt,
            _ => panic!("actual native use required"),
        };
        attempt.finish_admission()?;
        assert!(
            member
                .refresh_use(
                    &codex_alab_core::managed_auth::Request {
                        slot: "736c6f742f6d656d626572".into(),
                        operation: "e".repeat(32),
                        expected_digest: "d".repeat(64)
                    },
                    &receipt
                )
                .await
                .is_err()
        );
        fixture.finish().await;
    }
    Ok(())
}

#[tokio::test]
async fn target_context_opens_its_own_member_and_uses_target_preload_and_reader_ready()
-> io::Result<()> {
    let fixture = Fixture::start(|_, _| {}).await?;
    let target = target::context_at(&fixture.endpoint, &"e".repeat(32), BIRTH).await?;
    assert_eq!(format!("{target:?}"), "NativeTarget(<private>)");
    let destination: Box<dyn codex_alab_core::native_target::Destination> = Box::new(target);
    assert_eq!(destination.context().assignment_generation(), 5);
    let member = destination.open_member(&"a".repeat(32)).await?;
    preload_reader(&member).await?;
    member.retire(Some(REGISTRATION)).await?;
    let requests = fixture.requests.lock().unwrap().clone();
    for action in [
        "member-open-target",
        "member-preload-target",
        "member-reader-ready-target",
    ] {
        let request = requests
            .iter()
            .find(|line| line.contains(&format!(" {action} ")))
            .unwrap();
        assert!(request.contains(&format!(
            " assignment={} context={}",
            "e".repeat(32),
            "c".repeat(64)
        )));
        assert_eq!(
            request.contains(" assignment_generation=5"),
            action == "member-reader-ready-target"
        );
    }
    assert!(!requests.iter().any(|line| line.contains(" member-open ")));
    fixture.finish().await;
    Ok(())
}

#[tokio::test]
async fn target_preload_cannot_substitute_another_installed_assignment() -> io::Result<()> {
    let fixture = Fixture::start(|action, frame| {
        if action == "member-preload-target" {
            frame["source"]["native_intent"] = json!("f".repeat(32));
        }
    })
    .await?;
    let target = target::context_at(&fixture.endpoint, &"e".repeat(32), BIRTH).await?;
    let member = target.open(&"a".repeat(32)).await?;
    assert!(member.test_preload(&"b".repeat(32), None).await.is_err());
    assert!(!member.live());
    assert!(
        !fixture
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|line| line.contains(" member-reader-ready-target "))
    );
    fixture.finish().await;
    Ok(())
}

#[tokio::test]
async fn target_open_refuses_a_new_native_instance_and_unknown_context_fields() -> io::Result<()> {
    let fixture = Fixture::start(|action, frame| {
        if action == "member-open-target" {
            frame["session"]["epoch"] = json!("another-native-instance");
        }
    })
    .await?;
    let target = target::context_at(&fixture.endpoint, &"e".repeat(32), BIRTH).await?;
    assert!(target.open(&"a".repeat(32)).await.is_err());
    fixture.finish().await;
    let fixture = Fixture::start(|action, frame| {
        if action == "member-target-context" {
            frame["unexpected"] = json!(true);
        }
    })
    .await?;
    assert!(
        target::context_at(&fixture.endpoint, &"e".repeat(32), BIRTH)
            .await
            .is_err()
    );
    fixture.finish().await;
    Ok(())
}

async fn target_resources(
    fixture: &Fixture,
) -> io::Result<(
    Box<dyn codex_alab_core::native_target::Destination>,
    Arc<dyn core::Member>,
    codex_alab_core::native_target::ResourceReport,
)> {
    let target = target::context_at(&fixture.endpoint, &"e".repeat(32), BIRTH).await?;
    let member = target.open(&"a".repeat(32)).await?;
    let preload = member.test_preload(&"b".repeat(32), None).await?;
    let generations = codex_alab_core::assignment::Generations {
        auth: 7,
        owner: 11,
        source: 13,
        login: 17,
    };
    member
        .test_ready(
            &preload.source.operation,
            Some(&preload.registration),
            &core::Loaded::Source {
                source: Box::new(preload.source.clone()),
                generations: generations.clone(),
            },
        )
        .await?;
    let report = codex_alab_core::native_target::ResourceReport::from_loaded(
        target.context(),
        member.id().into(),
        preload.registration,
        "6".repeat(32),
        preload.source,
        generations,
    )?;
    Ok((Box::new(target), member, report))
}

#[tokio::test]
async fn resource_ready_uses_exact_member_and_ack_preserves_original_private_receipt()
-> io::Result<()> {
    let fixture = Fixture::start(|_, _| {}).await?;
    let (target, member, report) = target_resources(&fixture).await?;
    let receipt = member
        .target_resource_ready(&"7".repeat(32), &report)
        .await?;
    assert_eq!(format!("{receipt:?}"), "ResourceReceipt(<private>)");
    for _ in 0..2 {
        let applied = target.acknowledge(&receipt).await?;
        assert_eq!(applied.report().binding(), report.binding());
    }
    let requests = fixture.requests.lock().unwrap().clone();
    let resource = requests
        .iter()
        .find(|line| line.contains(" member-target-resource-ready "))
        .unwrap();
    assert!(resource.contains(&format!(" member={MEMBER}")));
    assert!(resource.contains(&format!(" registration={REGISTRATION}")));
    assert_eq!(
        requests
            .iter()
            .filter(|line| line.contains(" member-target-ack "))
            .count(),
        2
    );
    assert!(
        !requests
            .iter()
            .any(|line| line.contains("codex-accounts-change"))
    );
    member.retire(Some(REGISTRATION)).await?;
    fixture.finish().await;
    Ok(())
}

#[tokio::test]
async fn resource_ready_does_not_select_another_member_by_equal_generations() -> io::Result<()> {
    let fixture = Fixture::start(|_, _| {}).await?;
    let (target, member, report) = target_resources(&fixture).await?;
    let wrong = codex_alab_core::native_target::ResourceReport::from_loaded(
        target.context(),
        "8".repeat(32),
        report.registration().into(),
        report.boundary().into(),
        report.source().clone(),
        report.binding().generations,
    )?;
    assert!(
        member
            .target_resource_ready(&"7".repeat(32), &wrong)
            .await
            .is_err()
    );
    assert!(
        !fixture
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|line| line.contains(" member-target-resource-ready "))
    );
    member.retire(Some(REGISTRATION)).await?;
    fixture.finish().await;
    Ok(())
}

#[tokio::test]
async fn private_ack_rejects_owner_latest_replacing_original_resource_observation() -> io::Result<()>
{
    let fixture = Fixture::start(|action, frame| {
        if action == "member-target-ack" {
            frame["credential_generation"] = json!(42);
        }
    })
    .await?;
    let (target, member, report) = target_resources(&fixture).await?;
    let receipt = member
        .target_resource_ready(&"7".repeat(32), &report)
        .await?;
    assert!(target.acknowledge(&receipt).await.is_err());
    member.retire(Some(REGISTRATION)).await?;
    fixture.finish().await;
    Ok(())
}

#[tokio::test]
async fn lost_resource_reply_repairs_exact_attempt_without_reopening_or_reloading_b()
-> io::Result<()> {
    let fixture = Fixture::start_with(|_, _| {}, Some("DROP-RESOURCE-REPLY")).await?;
    let (target, member, report) = target_resources(&fixture).await?;
    let resource_operation = "7".repeat(32);
    assert!(
        member
            .target_resource_ready(&resource_operation, &report)
            .await
            .is_err()
    );
    assert!(!member.live(), "the resource work channel actually ended");
    let receipt = member
        .target_resource_ready(&resource_operation, &report)
        .await?;
    let applied = target.acknowledge(&receipt).await?;
    assert_eq!(applied.report().binding(), report.binding());
    let requests = fixture.requests.lock().unwrap().clone();
    for action in ["member-open-target", "member-preload-target"] {
        assert_eq!(
            requests
                .iter()
                .filter(|line| line.contains(&format!(" {action} ")))
                .count(),
            1
        );
    }
    assert_eq!(
        requests
            .iter()
            .filter(|line| line.contains(" member-target-resource-ready "))
            .count(),
        2
    );
    member.retire(Some(REGISTRATION)).await?;
    fixture.finish().await;
    Ok(())
}

const REBIND_CHILD: &str = "ALAB_NATIVE_WORK_REBIND_CHILD";
#[test]
fn existing_work_rebind_preserves_member_but_cannot_revive_reader_ready() {
    let test = concat!(module_path!(), "::existing_work_rebind_child")
        .split_once("::")
        .unwrap()
        .1;
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test])
        .env(REBIND_CHILD, "1")
        .env(core::LAUNCH_ENV, "fixture-native-source-constructor")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
}
struct RecordingHost {
    endpoint: Endpoint,
    issued: Mutex<Option<Arc<dyn core::Member>>>,
}
impl core::Host for RecordingHost {
    fn source<'a>(&'a self, _: &'a str) -> core::SourceFuture<'a> {
        Box::pin(async { Err(unavailable()) })
    }
    fn open_member<'a>(&'a self, operation: &'a str) -> core::MemberFuture<'a> {
        Box::pin(async move {
            let member = open_at(&self.endpoint, operation, BIRTH).await?;
            *self.issued.lock().unwrap() = Some(member.clone());
            Ok(member)
        })
    }
}
#[tokio::test]
async fn existing_work_rebind_child() -> io::Result<()> {
    if std::env::var_os(REBIND_CHILD).is_none() {
        return Ok(());
    }
    let fixture = Fixture::start(|_, _| {}).await?;
    let host = Arc::new(RecordingHost {
        endpoint: fixture.endpoint.clone(),
        issued: Mutex::new(None),
    });
    core::install(host.clone());
    codex_alab_core::native_source_launch::enter().await?;
    let lifetime = codex_alab_core::native_source_lifetime::Lifetime::new(
        codex_alab_core::native_source_launch::allocate_manager()
            .await?
            .unwrap(),
    )?;
    lifetime.attach_domain(Arc::new(codex_alab_core::assignment::Domain::default()))?;
    let member = host.issued.lock().unwrap().clone().unwrap();
    let packet = lifetime.preload(&"b".repeat(32), None).await?;
    lifetime.reader_ready(&packet.source).await?;
    assert!(lifetime.ready());
    fixture.close.notify_one();
    tokio::time::timeout(Duration::from_secs(2), member.closed()).await??;
    assert!(!lifetime.ready());
    member.reconcile_work().await?;
    assert_eq!(member.id(), MEMBER);
    assert_eq!(member.work_incarnation(), 1);
    assert!(
        !lifetime.ready(),
        "Bound alone cannot revive the previous Ready"
    );
    assert!(lifetime.reader_loaded().is_err());
    lifetime.reader_ready(&packet.source).await?;
    assert!(
        lifetime.ready(),
        "only the exact owned completion qualifies this channel"
    );
    let requests = fixture.requests.lock().unwrap().clone();
    assert_eq!(
        requests
            .iter()
            .filter(|line| line.contains(" codex-auth member-open "))
            .count(),
        2,
        "only the original OPEN and its same-RPC Waiting poll allocated a member"
    );
    assert_eq!(
        requests
            .iter()
            .filter(|line| line.contains(" codex-auth member-work-rebind "))
            .count(),
        1
    );
    assert_eq!(
        requests
            .iter()
            .filter(|line| line.contains(" codex-auth member-preload "))
            .count(),
        2,
        "rebind cannot load a new source"
    );
    fixture.finish().await;
    Ok(())
}

fn committed_resample_frame(action: &str, frame: &mut serde_json::Value) {
    if !action.ends_with("resample") {
        return;
    }
    let fields = frame.as_object_mut().unwrap();
    fields.remove("source");
    fields.remove("registration");
    fields.insert("state".into(), json!("ready_committed"));
    fields.insert("previous_registration".into(), json!(REGISTRATION));
    fields.insert("expected_credential_generation".into(), json!(41));
    fields.insert("expected_digest".into(), json!("d".repeat(64)));
}
#[tokio::test]
async fn resample_committed_hint_is_exact_metadata_not_source_or_ready() -> io::Result<()> {
    let fixture = Fixture::start(committed_resample_frame).await?;
    let member = open_at(&fixture.endpoint, &"a".repeat(32), BIRTH).await?;
    let body = attempt(&"b".repeat(32));
    let packet = member.preload_owned(&body, None).await?;
    let previous = codex_alab_core::native_preload::Predecessor {
        registration: packet.registration,
        revision: packet.source.revision,
    };
    let result = member.resample_owned(&body, &previous, None).await?;
    let codex_alab_core::native_preload::Resample::ReadyCommitted(committed) = result else {
        panic!("committed metadata must never become a source packet");
    };
    committed.validate(&body, &previous)?;
    let mut foreign = previous.clone();
    foreign.revision.generation += 1;
    assert!(committed.validate(&body, &foreign).is_err());
    assert_eq!(format!("{committed:?}"), "NativeReadyCommitted(<private>)");
    fixture.finish().await;
    Ok(())
}
#[tokio::test]
async fn resample_committed_hint_refuses_mixed_authority_and_foreign_predecessor() -> io::Result<()>
{
    for mutate in [
        (|action: &str, frame: &mut serde_json::Value| {
            committed_resample_frame(action, frame);
            if action.ends_with("resample") {
                frame["registration"] = json!(REGISTRATION);
            }
        }) as fn(&str, &mut serde_json::Value),
        |action, frame| {
            committed_resample_frame(action, frame);
            if action.ends_with("resample") {
                frame["expected_digest"] = json!("f".repeat(64));
            }
        },
    ] {
        let fixture = Fixture::start(mutate).await?;
        let member = open_at(&fixture.endpoint, &"a".repeat(32), BIRTH).await?;
        let body = attempt(&"b".repeat(32));
        let packet = member.preload_owned(&body, None).await?;
        let previous = codex_alab_core::native_preload::Predecessor {
            registration: packet.registration,
            revision: packet.source.revision,
        };
        assert!(member.resample_owned(&body, &previous, None).await.is_err());
        fixture.finish().await;
    }
    Ok(())
}

#[cfg(debug_assertions)]
#[tokio::test]
async fn rebound_actions_require_their_own_exact_wire_receipts() -> io::Result<()> {
    for wrong in [None, Some("OLD-READY-PREFIX"), Some("SOURCE-CHANGED")] {
        let fixture = Fixture::start_with(|_, _| {}, wrong).await?;
        let member = open_at(&fixture.endpoint, &"a".repeat(32), BIRTH).await?;
        let original = attempt(&"b".repeat(32));
        let preload = member.preload_owned(&original, None).await?;
        let loaded = core::Loaded::Source {
            source: Box::new(preload.source),
            generations: codex_alab_core::assignment::Generations {
                auth: 13,
                owner: 2,
                source: 4,
                login: 7,
            },
        };
        member
            .ready_owned(&original, &preload.registration, &loaded)
            .await?;
        member.debug_close_work()?;
        member.closed().await?;
        let result = member
            .rebound_ready_owned(&original, &preload.registration, &loaded)
            .await;
        match wrong {
            None => assert!(matches!(result, Ok(core::ReboundReadyOutcome::Ready(_)))),
            Some("SOURCE-CHANGED") => assert!(matches!(
                result,
                Ok(core::ReboundReadyOutcome::SourceChanged)
            )),
            Some(_) => assert!(
                result.is_err(),
                "a historical READY is not rebound authority"
            ),
        }
        if wrong.is_none() {
            let mut next = attempt(&"c".repeat(32));
            next.sequence = codex_alab_core::native_preload::Sequence::parse(
                "11111111111111110000000000000002",
            )?;
            let fresh = member.rebound_preload_owned(&next).await?;
            assert_eq!(fresh.sequence, next.sequence);
            let requests = fixture.requests.lock().unwrap();
            for action in ["member-rebound-ready", "member-preload-rebound"] {
                assert!(
                    requests
                        .iter()
                        .any(|line| line.contains(&format!(" {action} ")))
                );
            }
            assert!(
                !requests
                    .iter()
                    .any(|line| line.contains("member-use-begin"))
            );
        }
        fixture.finish().await;
    }
    Ok(())
}
