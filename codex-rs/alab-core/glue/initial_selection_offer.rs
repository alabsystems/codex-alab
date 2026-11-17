// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Client-only closed carrier decoder. Restore the original error before UI policy.
use super::metadata::NEXT_KEY;
use super::metadata::NextSelection;
use serde::Deserialize;

/// Closed client carrier. Preserve the original data until every field is valid.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SelectionOffer {
    schema: u32,
    kind: SelectionOfferKind,
    selection: NextSelection,
    had_data: bool,
    original_value: serde_json::Value,
}
#[derive(Deserialize)]
enum SelectionOfferKind {
    #[serde(rename = "nextInitialSelection")]
    Next,
}

pub(crate) fn take_offer(
    error: &mut codex_app_server_protocol::JSONRPCErrorError,
    expected: &str,
) -> std::io::Result<Option<NextSelection>> {
    let invalid = || {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid next initial selection",
        )
    };
    let Some(data) = error.data.as_ref().and_then(serde_json::Value::as_object) else {
        return Ok(None);
    };
    let Some(value) = data.get(NEXT_KEY) else {
        return Ok(None);
    };
    if data.len() != 1 {
        return Err(invalid());
    }
    let offer: SelectionOffer = serde_json::from_value(value.clone()).map_err(|_| invalid())?;
    if offer.schema != 1
        || !offer.selection.valid()
        || offer.selection.previous_claim != expected
        || (!offer.had_data && !offer.original_value.is_null())
    {
        return Err(invalid());
    }
    // Inspect the discriminator through its closed enum, not a truthy label.
    match offer.kind {
        SelectionOfferKind::Next => {}
    }
    error.data = offer.had_data.then_some(offer.original_value);
    Ok(Some(offer.selection))
}
