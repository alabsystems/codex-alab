// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Codec shape/ownership checks. Actual Native A remains separately required.
use super::*;
use serde_json::json;
type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
#[test]
fn original_delete_claim_keeps_all_unowned_params_and_refuses_combined_scope() -> TestResult {
    let base = json!({"id":1,"method":"thread/delete","params":{"threadId":"actual-child-data","_meta":{"foreign":[1,null],"alab.sideRollback":{"phase":"rollback","schema":1,"claim":"a".repeat(32),"operation":"b".repeat(32)}}}});
    let mut wire: JSONRPCRequest = serde_json::from_value(base.clone())?;
    let claim = take(&mut wire)?.ok_or("owned claim missing")?;
    assert_eq!(
        claim.parameters,
        json!({"threadId":"actual-child-data","_meta":{"foreign":[1,null]}})
    );
    for key in [
        "alab.sideView",
        "alab.sideInjection",
        "alab.initialSession",
        "alab.clientConstruction",
        "alab.retainedCallbacks",
        "alab.retainedConfigured",
    ] {
        let mut value = base.clone();
        value["params"]["_meta"][key] = json!({});
        assert!(take(&mut serde_json::from_value(value)?).is_err());
    }
    let mut value = base.clone();
    value["params"]["_meta"][metadata::KEY]["nativeCurrent"] = json!(true);
    assert!(take(&mut serde_json::from_value(value)?).is_err());
    let mut value = base;
    value["method"] = json!("thread/unsubscribe");
    assert!(take(&mut serde_json::from_value(value)?).is_err());
    Ok(())
}
