// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::fake::FakeAterm;
use crate::fake::Mode;
use serde_json::Value;
use serde_json::json;

const BIRTH: &str = "Fri Nov 6 10:00:00 2026";
const PRIVATE_PATH: &str = "/private/source-fixture/private-auth.json";

fn ready(sid: &str, operation: &str) -> Value {
    json!({
        "schema": 1, "operation": operation,
        "session": {"sid":sid, "process_pid":std::process::id(),
            "process_birth":BIRTH, "epoch":"native-host-epoch"},
        "slot":"736c6f742f6163746f72", "path":PRIVATE_PATH,
        "account":"a".repeat(64), "user":"b".repeat(64),
        "credential_generation":7, "digest":"d".repeat(64),
        "assignment_generation":2, "native_intent":"e".repeat(32)
    })
}

fn line(value: &Value) -> String {
    format!("OK codex-auth source {value}")
}

#[test]
fn private_source_binds_caller_operation_and_actual_generations() {
    let operation = "c".repeat(32);
    let value = ready("fixture", &operation);
    let Outcome::Ready(source) = parse(&line(&value), &operation, "fixture", BIRTH).unwrap() else {
        panic!("ready receipt");
    };
    assert_eq!(source.revision.generation, 7);
    assert_eq!(source.assignment_generation, 2);
    assert_eq!(source.path, PathBuf::from(PRIVATE_PATH));
    assert_eq!(format!("{source:?}"), "NativeSource(<private>)");

    let mut bootstrap = value.clone();
    bootstrap["assignment_generation"] = json!(0);
    bootstrap["native_intent"] = Value::Null;
    assert!(matches!(
        parse(&line(&bootstrap), &operation, "fixture", BIRTH),
        Ok(Outcome::Ready(_))
    ));
    // Bootstrap is descriptive only; the manager's rebind barrier decides whether
    // the receipt can activate a source. Waiting claims neither generation.
    let waiting = format!(
        "OK codex-auth source-waiting {}",
        json!({"schema":1,"operation":operation,"session":value["session"]})
    );
    assert!(matches!(
        parse(&waiting, &operation, "fixture", BIRTH),
        Ok(Outcome::Waiting)
    ));
    assert!(parse(&waiting, &"f".repeat(32), "fixture", BIRTH).is_err());
    assert!(parse(&waiting, &operation, "other", BIRTH).is_err());
    assert!(parse(&waiting, &operation, "fixture", "replacement birth").is_err());
}

#[test]
fn malformed_or_unbound_source_never_echoes_private_data() {
    let operation = "c".repeat(32);
    let original = ready("fixture", &operation);
    let mut invalid = Vec::new();
    for (key, value) in [
        ("schema", json!(2)),
        ("operation", json!("f".repeat(32))),
        ("slot", json!("ff")),
        ("path", json!("relative/auth.json")),
        ("path", json!("/private/../auth.json")),
        ("path", json!("/private/./auth.json")),
        ("path", json!("/private/auth\ncanary")),
        ("path", json!(format!("/{}", "x".repeat(4096)))),
        ("account", json!("bad-account")),
        ("user", json!("bad-user")),
        ("digest", json!("bad-digest")),
        ("credential_generation", json!(0)),
        ("credential_generation", json!(-1)),
        ("assignment_generation", json!(0)),
        ("native_intent", Value::Null),
        ("native_intent", json!("bad-intent")),
        ("token", json!("private-token-canary")),
    ] {
        let mut reply = original.clone();
        reply[key] = value;
        invalid.push(line(&reply));
    }
    for (key, value) in [
        ("sid", json!("other")),
        ("process_pid", json!(std::process::id() + 1)),
        ("process_birth", json!("replacement birth")),
        ("epoch", json!("bad\nepoch")),
    ] {
        let mut reply = original.clone();
        reply["session"][key] = value;
        invalid.push(line(&reply));
    }
    let mut missing = original.clone();
    missing.as_object_mut().unwrap().remove("native_intent");
    invalid.push(line(&missing));
    let mut duplicate = line(&original);
    duplicate.pop();
    duplicate.push_str(",\"schema\":1}");
    invalid.push(duplicate);
    invalid.push(format!("OK codex-auth source {}", "x".repeat(MAX_FRAME)));
    invalid.push("ERR private-token-canary private-auth.json".into());
    for reply in invalid {
        let error = parse(&reply, &operation, "fixture", BIRTH)
            .err()
            .expect("unsafe receipt rejected")
            .to_string();
        assert!(!error.contains(PRIVATE_PATH));
        assert!(!error.contains("private-token-canary"));
        assert!(!error.contains("private-auth.json"));
    }
}

#[tokio::test]
async fn authenticated_source_wait_reuses_operation_without_supervision() -> io::Result<()> {
    let dir = tempfile::tempdir()?;
    let fake = FakeAterm::start(dir.path(), Mode::Current).await?;
    fake.set_effective(false, "interface off");
    let operation = "c".repeat(32);
    let command = format!("@{} codex-auth source operation={operation}", fake.sid());
    let source = ready(&fake.sid(), &operation);
    fake.reply_once(
        &command,
        &format!(
            "OK codex-auth source-waiting {}\n",
            json!({"schema":1,"operation":operation,"session":source["session"]})
        ),
    );
    let publish = async {
        while !fake.requests().contains(&command) {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        fake.reply_once(&command, &format!("{}\n", line(&source)));
    };
    let endpoint = fake.endpoint();
    let (result, ()) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(source_at(&endpoint, &operation, BIRTH), publish)
    })
    .await
    .expect("source owner must answer");
    assert_eq!(result?.revision.generation, 7);
    assert_eq!(
        fake.requests()
            .iter()
            .filter(|line| **line == command)
            .count(),
        2
    );
    assert!(
        fake.requests()
            .iter()
            .all(|line| !line.contains("supervisor") && !line.contains("harness policy"))
    );
    let requests = fake.requests().len();
    assert!(
        source_at(&endpoint, "invalid operation", BIRTH)
            .await
            .is_err()
    );
    assert_eq!(
        fake.requests().len(),
        requests,
        "invalid input sends nothing"
    );
    Ok(())
}
