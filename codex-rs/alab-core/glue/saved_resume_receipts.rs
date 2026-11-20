// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Bounded, process-local DATA for exact original saved resumes after normal retirement.
use crate::outgoing_message::alab_initial_claim::ClientPresentation;
use crate::outgoing_message::alab_initial_claim::PreparationOffer;
use crate::outgoing_message::alab_initial_claim::PreparedArguments;
use crate::outgoing_message::alab_initial_claim::SavedResumeOutcome;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::ThreadStartParams;
use serde_json::Number;
use serde_json::Value;
use sha2::Digest;
use sha2::Sha256;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::io;
use std::io::Write;
use std::sync::Arc;

const MAX_ENTRIES: usize = 4096;
const MAX_ERROR_BYTES: usize = 64 * 1024;
const MAX_TOTAL_BYTES: usize = 4 * 1024 * 1024;

pub(super) type Key = [u8; 32];

pub(super) fn key(original: &PreparedArguments, offer: &PreparationOffer) -> Option<Key> {
    original.saved_resume_child(offer)?;
    key_for_fields(
        offer,
        original.saved_opening.as_ref() == Some(&offer.preparation),
        original.method.name(),
        &original.parameters,
        &original.selection.prospective_create,
        original.selection.presentation,
    )
}

pub(super) fn key_for_fields(
    offer: &PreparationOffer,
    saved_opening: bool,
    method: &str,
    parameters: &Value,
    prospective_create: &ThreadStartParams,
    presentation: ClientPresentation,
) -> Option<Key> {
    if !saved_opening
        || !offer.valid()
        || method != "thread/resume"
        || presentation != ClientPresentation::SideConversation
    {
        return None;
    }
    let child = parameters.get("threadId")?.as_str()?;
    let mut value = serde_json::to_value((
        &offer.preparation,
        offer,
        method,
        parameters,
        prospective_create,
        presentation,
        child,
    ))
    .ok()?;
    value.sort_all_objects();
    let zero = Number::from_f64(0.0)?;
    canonicalize_zero(&mut value, &zero);
    let mut digest = Sha256::new();
    digest.update(b"codex-alab/saved-resume-receipt/v1\0");
    serde_json::to_writer(&mut digest, &value).ok()?;
    Some(digest.finalize().into())
}

// The default JSON Number compares floating +0 and -0 equal. With arbitrary
// precision it compares the original spelling instead. Use its actual equality
// in either build, which also keeps integer zero distinct from floating zero.
fn canonicalize_zero(value: &mut Value, zero: &Number) {
    match value {
        Value::Number(number) if number == zero => number.clone_from(zero),
        Value::Array(items) => items
            .iter_mut()
            .for_each(|item| canonicalize_zero(item, zero)),
        Value::Object(fields) => fields
            .values_mut()
            .for_each(|item| canonicalize_zero(item, zero)),
        _ => {}
    }
}

#[derive(Default)]
struct BoundedError(Vec<u8>);

impl Write for BoundedError {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_ERROR_BYTES - self.0.len() {
            return Err(io::Error::other(
                "saved resume rejection exceeds receipt capacity",
            ));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Clone)]
pub(super) struct Receipt {
    // No Core, original request, response object, Native transition, or connection survives here.
    rejection: Option<Arc<[u8]>>,
}

impl Receipt {
    pub(super) fn resumed() -> Self {
        Self { rejection: None }
    }

    pub(super) fn rejected(error: &JSONRPCErrorError) -> Option<Self> {
        let mut encoded = BoundedError::default();
        serde_json::to_writer(
            &mut encoded,
            &(
                error.code,
                &error.message,
                error.data.is_some(),
                &error.data,
            ),
        )
        .ok()?;
        // Raw JSON may use a reserved serde number representation or exceed the
        // decoder's nesting limit. Never publish an altered or unreadable error.
        let (code, message, had_data, value) =
            serde_json::from_slice::<(i64, String, bool, Value)>(&encoded.0).ok()?;
        if code != error.code
            || message != error.message
            || had_data.then_some(&value) != error.data.as_ref()
        {
            return None;
        }
        Some(Self {
            rejection: Some(encoded.0.into()),
        })
    }

    fn size(&self) -> usize {
        self.rejection.as_ref().map_or(0, |bytes| bytes.len())
    }

    pub(super) fn outcome(&self) -> SavedResumeOutcome {
        let Some(bytes) = &self.rejection else {
            return SavedResumeOutcome::Resumed {};
        };
        match serde_json::from_slice::<(i64, String, bool, Value)>(bytes) {
            Ok((code, message, had_data, value)) => SavedResumeOutcome::Rejected {
                code,
                message,
                had_data,
                value,
            },
            Err(_) => SavedResumeOutcome::Unavailable {},
        }
    }
}

#[derive(Default)]
pub(super) struct Receipts {
    entries: HashMap<Key, Receipt>,
    order: VecDeque<Key>,
    bytes: usize,
}

impl Receipts {
    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(super) fn get(&self, key: &Key) -> Option<Receipt> {
        self.entries.get(key).cloned()
    }

    pub(super) fn insert(&mut self, key: Key, receipt: Receipt) {
        if self.entries.contains_key(&key) {
            return;
        }
        let size = receipt.size();
        while self.order.len() >= MAX_ENTRIES || self.bytes + size > MAX_TOTAL_BYTES {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some(retired) = self.entries.remove(&oldest) {
                self.bytes -= retired.size();
            }
        }
        self.bytes += size;
        self.order.push_back(key);
        self.entries.insert(key, receipt);
    }
}

#[cfg(test)]
#[path = "saved_resume_receipts_tests.rs"]
mod tests;
