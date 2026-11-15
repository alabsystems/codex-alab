// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use codex_protocol::openai_models::ModelsResponse;
use std::io;

mod errors;
pub use errors::map_api_error;

pub const GLOBAL_PROVIDER_ID: &str = "kimi";
pub const CHINA_PROVIDER_ID: &str = "kimi-cn";
const CATALOG_JSON: &str = include_str!("../models.json");

pub fn is_kimi_provider(provider_id: &str) -> bool {
    matches!(provider_id, GLOBAL_PROVIDER_ID | CHINA_PROVIDER_ID)
}

pub fn model_catalog(
    active: bool,
    provider_id: &str,
    configured: Option<ModelsResponse>,
) -> io::Result<Option<ModelsResponse>> {
    if configured.is_some() || !active || !is_kimi_provider(provider_id) {
        return Ok(configured);
    }
    let mut catalog: ModelsResponse = serde_json::from_str(CATALOG_JSON).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("embedded Kimi model catalog is invalid: {err}"),
        )
    })?;
    for model in &mut catalog.models {
        let messages = model.model_messages.get_or_insert_default();
        if messages
            .instructions_template
            .as_deref()
            .unwrap_or_default()
            .is_empty()
        {
            messages.instructions_template =
                Some(codex_models_manager::model_info::BASE_INSTRUCTIONS.to_owned());
        }
    }
    Ok(Some(catalog))
}

#[cfg(test)]
mod errors_tests;
#[cfg(test)]
mod tests;
