// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Exact owned config/read preparation metadata. Unowned metadata is preserved
//! for the original parser; no Source selector can create Native currentness.
use super::*;
pub(crate) use metadata::preparation::Completion as PreparationCompletion;
pub(crate) use metadata::preparation::Offer as PreparationOffer;
pub(crate) use metadata::preparation::Prepare as PreparedArguments;
pub(crate) use metadata::preparation::Request as PreparationRequest;
#[derive(Clone)]
pub(crate) struct PreparationClaim(pub(crate) Arc<PreparationRequest>);
impl std::fmt::Debug for PreparationClaim {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("ClientPreparation(<frozen Source DATA; requires actual Native origin>)")
    }
}
pub(crate) fn take_preparation(
    request: &mut JSONRPCRequest,
) -> io::Result<Option<PreparationClaim>> {
    let Some(params) = request.params.as_mut().and_then(Value::as_object_mut) else {
        return Ok(None);
    };
    let Some(meta) = params.get_mut(META).and_then(Value::as_object_mut) else {
        return Ok(None);
    };
    let Some(value) = meta.remove(metadata::preparation::KEY) else {
        return Ok(None);
    };
    if request.method != "config/read"
        || [
            KEY,
            CLIENT_KEY,
            "alab.retainedFrontier",
            "alab.retainedConfigured",
            "alab.sourceSelectionFrontier",
        ]
        .into_iter()
        .any(|key| meta.contains_key(key))
    {
        return Err(invalid());
    }
    let claim: PreparationRequest = serde_json::from_value(value).map_err(|_| invalid())?;
    if !claim.valid() {
        return Err(invalid());
    }
    if meta.is_empty() {
        params.remove(META);
    }
    Ok(Some(PreparationClaim(Arc::new(claim))))
}
