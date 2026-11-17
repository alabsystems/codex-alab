// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Namespaced result metadata beside the actual typed payload. This is Source
//! correlation DATA only. It grants no local/Native/model authority.
use crate::ClientResponse;
use crate::ClientResponsePayload;
use crate::RequestId;
use serde::Serialize;
use serde::ser::Error as _;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone)]
pub struct Payload {
    inner: Option<Box<ClientResponsePayload>>,
    entries: BTreeMap<String, Value>,
}
impl std::fmt::Debug for Payload {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("AlabResponseMetadata(<typed payload; Source data only>)")
    }
}
impl Payload {
    pub(crate) fn into_client_response(self, id: RequestId) -> Option<ClientResponse> {
        self.inner
            .and_then(|inner| (*inner).into_client_response(id))
    }
}
impl Serialize for Payload {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let value = match &self.inner {
            Some(inner) => serde_json::to_value(inner).map_err(S::Error::custom)?,
            None => Value::Object(Default::default()),
        };
        let value = merge_metadata(value, &self.entries).map_err(S::Error::custom)?;
        value.serialize(serializer)
    }
}
fn merge_metadata(
    mut value: Value,
    entries: &BTreeMap<String, Value>,
) -> Result<Value, &'static str> {
    let body = value
        .as_object_mut()
        .ok_or("ALab response metadata requires an object")?;
    let meta = body
        .entry("_meta")
        .or_insert_with(|| Value::Object(Default::default()))
        .as_object_mut()
        .ok_or("response metadata is not an object")?;
    for (key, entry) in entries {
        if meta.contains_key(key) {
            return Err("response metadata namespace occupied");
        }
        meta.insert(key.clone(), entry.clone());
    }
    Ok(value)
}
fn insert(
    mut payload: Payload,
    key: &str,
    value: Value,
) -> Result<ClientResponsePayload, serde_json::Error> {
    if !key.starts_with("alab.") || key.len() <= 5 || payload.entries.contains_key(key) {
        return Err(<serde_json::Error as serde::ser::Error>::custom(
            "response metadata namespace occupied or invalid",
        ));
    }
    payload.entries.insert(key.into(), value);
    Ok(ClientResponsePayload::AlabResponseMetadata(payload))
}
/// Preserve the actual typed response, including typed analytics. This cannot
/// construct an arbitrary result body or overwrite another metadata owner.
pub fn decorate(
    payload: ClientResponsePayload,
    key: &str,
    value: Value,
) -> Result<ClientResponsePayload, serde_json::Error> {
    let payload = match payload {
        ClientResponsePayload::AlabResponseMetadata(payload) => payload,
        actual => Payload {
            inner: Some(Box::new(actual)),
            entries: BTreeMap::new(),
        },
    };
    insert(payload, key, value)
}
/// The retained frontier's acknowledgment carries metadata only and never
/// pretends to be a newly reconstructed thread/resume or a fresh Core result.
pub fn acknowledgment(key: &str, value: Value) -> Result<ClientResponsePayload, serde_json::Error> {
    insert(
        Payload {
            inner: None,
            entries: BTreeMap::new(),
        },
        key,
        value,
    )
}

#[cfg(test)]
#[path = "response_metadata_payload_tests.rs"]
mod tests;
