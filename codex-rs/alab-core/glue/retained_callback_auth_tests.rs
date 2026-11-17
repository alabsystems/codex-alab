// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::*;
#[test]
fn bookkeeping_proof_is_owner_domain_and_content_bound() -> std::io::Result<()> {
    let owner = Auth::new();
    let value = serde_json::json!({"ordinal":3,"outcome":{"state":"response"}});
    let proof = owner.sign("cut", &value)?;
    assert!(owner.verify("cut", &value, &proof)?);
    assert!(!owner.verify("descriptor", &value, &proof)?);
    assert!(!Auth::new().verify("cut", &value, &proof)?);
    assert!(!owner.verify(
        "cut",
        &serde_json::json!({"ordinal":4,"outcome":{"state":"response"}}),
        &proof
    )?);
    Ok(())
}
#[test]
fn malformed_multibyte_and_noncanonical_proofs_refuse_without_slicing_panic() -> std::io::Result<()>
{
    let owner = Auth::new();
    for proof in [
        "é".repeat(32),
        "a".repeat(63),
        "A".repeat(64),
        "g".repeat(64),
    ] {
        assert!(!owner.verify("cut", &0, &proof)?);
    }
    Ok(())
}
