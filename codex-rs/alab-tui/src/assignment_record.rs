// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! An inert native journal locator. It never authorizes an account operation.

use codex_alab_core::assignment as core;
use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Stamp {
    credential_generation: u64,
    assignment_generation: u64,
    auth_generation: u64,
    owner_generation: u64,
    source_generation: u64,
    login_generation: u64,
}
impl From<&core::ReceiptStamp> for Stamp {
    fn from(stamp: &core::ReceiptStamp) -> Self {
        Self {
            credential_generation: stamp.credential_generation,
            assignment_generation: stamp.assignment_generation,
            auth_generation: stamp.generations.auth,
            owner_generation: stamp.generations.owner,
            source_generation: stamp.generations.source,
            login_generation: stamp.generations.login,
        }
    }
}
impl From<Stamp> for core::ReceiptStamp {
    fn from(stamp: Stamp) -> Self {
        Self {
            credential_generation: stamp.credential_generation,
            assignment_generation: stamp.assignment_generation,
            generations: core::Generations {
                auth: stamp.auth_generation,
                owner: stamp.owner_generation,
                source: stamp.source_generation,
                login: stamp.login_generation,
            },
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Receipt {
    schema: u32,
    operation: String,
    boundary: Option<String>,
    thread: String,
    mode: String,
    pool_revision: String,
    target: Option<String>,
    source: Stamp,
    destination: Option<Stamp>,
    acknowledged: bool,
}
impl From<&core::Receipt> for Receipt {
    fn from(receipt: &core::Receipt) -> Self {
        let (mode, pool_revision, target) = match &receipt.selection {
            core::Selection::Manual { account, revision } => {
                ("manual", revision.clone(), Some(account.clone()))
            }
            core::Selection::Auto { revision } => ("auto", revision.clone(), None),
        };
        Self {
            schema: 1,
            operation: receipt.operation.clone(),
            boundary: receipt.boundary.clone(),
            thread: receipt.thread.clone(),
            mode: mode.into(),
            pool_revision,
            target,
            source: (&receipt.source).into(),
            destination: receipt.destination.as_ref().map(Into::into),
            acknowledged: receipt.acknowledged,
        }
    }
}
impl Receipt {
    pub fn into_core(self) -> Option<core::Receipt> {
        if self.schema != 1 {
            return None;
        }
        let selection = match (self.mode.as_str(), self.target) {
            ("manual", Some(account)) => core::Selection::Manual {
                account,
                revision: self.pool_revision,
            },
            ("auto", None) => core::Selection::Auto {
                revision: self.pool_revision,
            },
            _ => return None,
        };
        let receipt = core::Receipt {
            operation: self.operation,
            boundary: self.boundary,
            thread: self.thread,
            selection,
            source: self.source.into(),
            destination: self.destination.map(Into::into),
            acknowledged: self.acknowledged,
        };
        receipt.valid().then_some(receipt)
    }
}

pub fn read<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Receipt>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    let receipt = serde_json::from_value::<Receipt>(value).ok();
    Ok(receipt.filter(|receipt| receipt.clone().into_core().is_some()))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn receipt() -> core::Receipt {
        core::Receipt {
            operation: "a".repeat(32),
            boundary: Some("b".repeat(32)),
            thread: "00000000-0000-0000-0000-000000000000".into(),
            selection: core::Selection::Manual {
                account: "c".repeat(64),
                revision: "d".repeat(64),
            },
            source: core::ReceiptStamp {
                credential_generation: 14,
                assignment_generation: 0,
                generations: core::Generations {
                    auth: 2,
                    owner: 0,
                    source: 0,
                    login: 0,
                },
            },
            destination: None,
            acknowledged: false,
        }
    }
    #[test]
    fn assignment_locator_roundtrips_without_private_material_or_owed_turn() {
        let original = receipt();
        let json = serde_json::to_string(&Receipt::from(&original)).unwrap();
        for forbidden in ["path", "digest", "token", "renewal", "purpose", "turn"] {
            assert!(!json.contains(forbidden));
        }
        assert_eq!(
            serde_json::from_str::<Receipt>(&json).unwrap().into_core(),
            Some(original)
        );
    }
    #[test]
    fn assignment_invalid_locator_is_discarded_without_losing_resume_history() {
        #[derive(Deserialize)]
        struct Envelope {
            history: String,
            #[serde(deserialize_with = "read")]
            assignment: Option<Receipt>,
        }
        let original = serde_json::to_value(Receipt::from(&receipt())).unwrap();
        for (field, value) in [
            ("schema", serde_json::json!(2)),
            ("operation", serde_json::json!("wrong")),
            ("acknowledged", serde_json::json!(true)),
            (
                "privatePath",
                serde_json::json!("/private/credential-canary"),
            ),
        ] {
            let mut malformed = original.clone();
            malformed[field] = value;
            let envelope: Envelope = serde_json::from_value(
                serde_json::json!({"history":"original work","assignment":malformed}),
            )
            .unwrap();
            assert_eq!(envelope.history, "original work");
            assert!(envelope.assignment.is_none());
        }
    }
}
