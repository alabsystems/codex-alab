// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Private-protocol controls only; these do not qualify native grant custody.

use super::*;
use serde_json::Value;
use serde_json::json;
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::net::UnixListener;

const MEMBER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const BIRTH: &str = "original-process-birth";

struct Fixture {
    _dir: tempfile::TempDir,
    endpoint: Endpoint,
    requests: Arc<Mutex<Vec<String>>>,
    server: tokio::task::JoinHandle<()>,
}

impl Fixture {
    async fn start(mutate: fn(&mut Value) -> bool) -> io::Result<Self> {
        let dir = tempfile::tempdir()?;
        let sock = dir.path().join("aterm-1.sock");
        let token = dir.path().join("aterm-1.token");
        std::fs::write(&token, b"work-rebind-fixture\n")?;
        std::fs::set_permissions(token, std::fs::Permissions::from_mode(0o600))?;
        let listener = UnixListener::bind(&sock)?;
        let endpoint = Endpoint {
            sock,
            sid: "s-123abc".into(),
        };
        let sid = endpoint.sid.clone();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&requests);
        let server = tokio::spawn(async move {
            let mut workers = tokio::task::JoinSet::new();
            for _ in 0..2 {
                let stream = listener.accept().await.unwrap().0;
                let sid = sid.clone();
                let recorded = Arc::clone(&recorded);
                workers.spawn(async move {
                    let (read, mut write) = stream.into_split();
                    let mut read = BufReader::new(read);
                    let mut line = String::new();
                    read.read_line(&mut line).await.unwrap();
                    assert_eq!(line, "AUTH work-rebind-fixture\n");
                    line.clear();
                    read.read_line(&mut line).await.unwrap();
                    assert_eq!(line, "version\n");
                    write.write_all(b"OK version=rebind-fixture\n").await.unwrap();
                    loop {
                        line.clear();
                        if read.read_line(&mut line).await.unwrap() == 0 { break; }
                        recorded.lock().unwrap().push(line.trim().into());
                        let words = line.split_ascii_whitespace().collect::<Vec<_>>();
                        let action = words[2];
                        let fields = words[3..].iter().map(|word| word.split_once('=').unwrap())
                            .collect::<BTreeMap<_, _>>();
                        let mut frame = json!({"schema":1,"request":fields["request"],
                            "operation":fields["operation"],"member":MEMBER,
                            "session":{"sid":sid,"process_pid":std::process::id(),"process_birth":BIRTH,"epoch":"original-native"}});
                        let response = match action {
                            "member-peer" => { frame["member"] = Value::Null; "member-peer" },
                            "member-open" | "member-open-derived" => "member-open",
                            "member-open-target" => "member-open-target",
                            "member-work-rebind" => {
                                frame["state"] = json!("bound");
                                for key in ["open_kind", "use_member", "use_operation", "capability", "assignment", "context"] {
                                    if let Some(value) = fields.get(key) { frame[key] = json!(value); }
                                }
                                if !mutate(&mut frame) { break; }
                                "member-work-rebind"
                            }
                            _ => panic!("unexpected allocation or fallback: {action}"),
                        };
                        write.write_all(format!("OK codex-auth {response} {frame}\n").as_bytes()).await.unwrap();
                    }
                });
            }
            while let Some(result) = workers.join_next().await {
                result.unwrap();
            }
        });
        Ok(Self {
            _dir: dir,
            endpoint,
            requests,
            server,
        })
    }

    async fn capture(&self, body: OriginalBody) -> io::Result<Binding> {
        let transport = connect_transport(&self.endpoint, BIRTH).await?;
        assert!(
            PeerIdentity::capture(&transport).is_err(),
            "an unanswered peer is not an epoch"
        );
        let operation = "a".repeat(32);
        let (action, fields) = match &body {
            OriginalBody::Ordinary => ("member-open", String::new()),
            OriginalBody::Derived {
                use_member,
                use_operation,
                capability,
            } => (
                "member-open-derived",
                format!(
                    " use_member={use_member} use_operation={use_operation} capability={capability}"
                ),
            ),
            OriginalBody::Target {
                assignment,
                context,
            } => (
                "member-open-target",
                format!(" assignment={assignment} context={context}"),
            ),
        };
        transport.call(action, &operation, &fields, None).await?;
        assert_eq!(
            PeerIdentity::capture(&transport)?.epoch(),
            "original-native"
        );
        let binding = Binding::capture(&transport, &operation, MEMBER, body)?;
        transport.conn.close();
        Ok(binding)
    }

    async fn finish(self) {
        tokio::time::timeout(Duration::from_secs(60), self.server)
            .await
            .expect("fixture completion hang detector")
            .unwrap();
    }
}

fn derived() -> OriginalBody {
    OriginalBody::Derived {
        use_member: "c".repeat(32),
        use_operation: "d".repeat(32),
        capability: "e".repeat(64),
    }
}

#[tokio::test]
async fn existing_work_rebind_preserves_each_original_open_body_without_allocation()
-> io::Result<()> {
    for body in [
        OriginalBody::Ordinary,
        derived(),
        OriginalBody::Target {
            assignment: "f".repeat(32),
            context: "1".repeat(64),
        },
    ] {
        let fixture = Fixture::start(|_| true).await?;
        let binding = fixture.capture(body).await?;
        assert_eq!(format!("{binding:?}"), "NativeWorkBinding(<private>)");
        let channel = binding.reconnect().await?;
        assert_eq!(channel.epoch()?.as_deref(), Some("original-native"));
        channel.conn.close();
        let requests = fixture
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|line| !line.contains(" codex-auth member-peer "))
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(requests.len(), 2);
        assert!(requests[1].contains(" codex-auth member-work-rebind "));
        assert!(requests[1].contains(&format!(" operation={} member={MEMBER}", "a".repeat(32))));
        fixture.finish().await;
    }
    Ok(())
}

#[tokio::test]
async fn work_rebind_refuses_foreign_roles_instances_and_absent_acknowledgements() -> io::Result<()>
{
    let changes: [fn(&mut Value) -> bool; 8] = [
        |v| {
            v["state"] = json!("ready");
            true
        },
        |v| {
            v["session"]["epoch"] = json!("replacement-native");
            true
        },
        |v| {
            v["member"] = json!("0".repeat(32));
            true
        },
        |v| {
            v["open_kind"] = json!("ordinary");
            true
        },
        |v| {
            v["use_operation"] = json!("2".repeat(32));
            true
        },
        |v| {
            v["capability"] = json!("3".repeat(64));
            true
        },
        |v| {
            v["source"] = json!(null);
            true
        },
        |_| false,
    ];
    for change in changes {
        let fixture = Fixture::start(change).await?;
        let binding = fixture.capture(derived()).await?;
        assert!(binding.reconnect().await.is_err());
        let requests = fixture
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|line| !line.contains(" codex-auth member-peer "))
            .count();
        assert_eq!(requests, 2, "a refused rebind cannot fall back to OPEN");
        fixture.finish().await;
    }
    Ok(())
}
