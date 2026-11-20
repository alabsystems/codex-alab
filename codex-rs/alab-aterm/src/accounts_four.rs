// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The identity-bound snapshot. Every object has named fields, including explicit nulls.

use super::*;
use serde::de::DeserializeOwned;

pub(super) fn decode(body: &str) -> serde_json::Result<Snapshot> {
    let WireObject(wire) = serde_json::from_str::<WireObject<Envelope>>(body)?;
    Ok(Snapshot {
        schema: wire.schema,
        observed_at: wire.observed_at,
        request: wire.request,
        session: wire.session.0,
        revision: wire.revision,
        assignment: Assignment {
            account_id: wire.assignment.0.account_id,
        },
        pool: Some(Pool {
            choice_id: wire.pool.0.choice_id,
            pinned: wire.pool.0.pinned,
            rotation: wire.pool.0.rotation,
        }),
        accounts: wire.accounts.into_iter().map(Into::into).collect(),
        capabilities: wire.capabilities.into(),
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    schema: u32,
    observed_at: i64,
    request: String,
    session: WireObject<Session>,
    revision: String,
    assignment: WireObject<Assigned>,
    pool: WireObject<Choice>,
    accounts: Vec<WireObject<WireAccountFour>>,
    capabilities: WireObject<WireCapabilitiesFour>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Assigned {
    #[serde(deserialize_with = "Deserialize::deserialize")]
    account_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Choice {
    #[serde(deserialize_with = "Deserialize::deserialize")]
    choice_id: Option<String>,
    pinned: bool,
    #[serde(deserialize_with = "word")]
    rotation: Rotation,
}

// Decode the finite native enum from a string, never an externally tagged object.
fn word<'de, D: serde::Deserializer<'de>, T: DeserializeOwned>(
    deserializer: D,
) -> Result<T, D::Error> {
    T::deserialize(serde::de::value::StringDeserializer::new(
        String::deserialize(deserializer)?,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireCapabilitiesFour {
    add: bool,
    select: bool,
    auto: bool,
    select_bound: bool,
}

impl From<WireObject<WireCapabilitiesFour>> for Capabilities {
    fn from(WireObject(caps): WireObject<WireCapabilitiesFour>) -> Self {
        Self {
            add: caps.add,
            select: caps.select,
            auto: caps.auto,
            select_bound: caps.select_bound,
        }
    }
}

/// Schema 4 requires both nullable fields even when a row has no bound destination.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireAccountFour {
    id: String,
    label: String,
    #[serde(deserialize_with = "Deserialize::deserialize")]
    masked_email: Option<String>,
    #[serde(deserialize_with = "word")]
    condition: Condition,
    #[serde(deserialize_with = "read_signed_out_reason")]
    signed_out_reason: Option<SignedOutReason>,
    #[serde(deserialize_with = "Deserialize::deserialize")]
    selection_ref: Option<String>,
    enrolled: bool,
    held: bool,
    eligible: bool,
    quota: WireObject<RequiredQuota>,
}

impl From<WireObject<WireAccountFour>> for Account {
    fn from(WireObject(row): WireObject<WireAccountFour>) -> Self {
        Self {
            id: row.id,
            label: row.label,
            masked_email: row.masked_email,
            condition: row.condition,
            signed_out_reason: row.signed_out_reason,
            selection_ref: row.selection_ref,
            enrolled: row.enrolled,
            held: row.held,
            eligible: row.eligible,
            quota: row.quota.0.into(),
        }
    }
}

// All five measurements may be null, but their keys are part of the published schema.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequiredQuota {
    stale: bool,
    #[serde(deserialize_with = "Deserialize::deserialize")]
    observed_at: Option<i64>,
    #[serde(deserialize_with = "Deserialize::deserialize")]
    five_hour_used_percent: Option<f64>,
    #[serde(deserialize_with = "Deserialize::deserialize")]
    five_hour_reset_at: Option<i64>,
    #[serde(deserialize_with = "Deserialize::deserialize")]
    weekly_used_percent: Option<f64>,
    #[serde(deserialize_with = "Deserialize::deserialize")]
    weekly_reset_at: Option<i64>,
}
impl From<RequiredQuota> for Quota {
    fn from(wire: RequiredQuota) -> Self {
        Self {
            stale: wire.stale,
            observed_at: wire.observed_at,
            five_hour_used_percent: wire.five_hour_used_percent,
            five_hour_reset_at: wire.five_hour_reset_at,
            weekly_used_percent: wire.weekly_used_percent,
            weekly_reset_at: wire.weekly_reset_at,
        }
    }
}
