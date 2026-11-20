// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use serde_json::json;

#[test]
fn side_result_queries_admit_only_the_exact_read_carrier() -> anyhow::Result<()> {
    let view = json!({"schema":1,"claim":"a".repeat(32),"threadId":"child"});
    let parameters = json!({"threadId":"child","_meta":{"extension":[2,null]}});
    for phase in ["readResult", "acknowledgeResult"] {
        let query =
            json!({"phase":phase,"view":view,"operation":"b".repeat(32),"parameters":parameters});
        let envelope = json!({"id":3,"method":"config/read","params":{"includeLayers":false,"_meta":{(metadata::KEY):query}}});
        let mut wire: JSONRPCRequest = serde_json::from_value(envelope.clone())?;
        let claim = take(&mut wire)?.expect("query selector");
        assert_eq!(serde_json::to_value(claim.selection)?, query);
        assert_eq!(wire.params, Some(json!({"includeLayers":false})));
        for params in [
            json!({"_meta":{(metadata::KEY):query}}),
            json!({"includeLayers":true,"_meta":{(metadata::KEY):query}}),
            json!({"includeLayers":false,"cwd":null,"_meta":{(metadata::KEY):query}}),
            json!({"includeLayers":false,"_meta":{(metadata::KEY):query,"foreign":1}}),
        ] {
            let mut malformed = envelope.clone();
            malformed["params"] = params;
            assert!(take(&mut serde_json::from_value(malformed)?).is_err());
        }
        let mut mutating = envelope.clone();
        mutating["method"] = json!("thread/unsubscribe");
        mutating["params"]["threadId"] = json!("child");
        assert!(take(&mut serde_json::from_value(mutating)?).is_err());
        let mut wrong_child = envelope;
        wrong_child["params"]["_meta"][metadata::KEY]["parameters"]["threadId"] = json!("other");
        assert!(take(&mut serde_json::from_value(wrong_child)?).is_err());
    }
    Ok(())
}
