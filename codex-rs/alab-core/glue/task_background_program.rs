// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Original actual account/catalog Daybreak policy shared by CREATE and SEND.
use crate::request_processors::AccountRequestProcessor;
use crate::request_processors::CatalogRequestProcessor;
use codex_app_server_protocol::Account;
use codex_app_server_protocol::ClientResponsePayload;
use codex_app_server_protocol::ExperimentalFeatureListParams;
use codex_app_server_protocol::GetAccountParams;
use codex_app_server_protocol::ModelListParams;
use codex_features::Feature;
use codex_features::Features;
use codex_protocol::ThreadId;
use std::collections::HashSet;
#[path = "task_daybreak_selection.rs"]
mod daybreak;

async fn api_key_programs_enabled(catalog: &CatalogRequestProcessor, child: ThreadId) -> bool {
    let mut cursor = None;
    let mut cursors = HashSet::new();
    let mut names = HashSet::new();
    let mut enabled = false;
    let discovery = async {
        for _ in 0..10 {
            let response = catalog
                .experimental_feature_list(ExperimentalFeatureListParams {
                    cursor: cursor.take(),
                    limit: Some(100),
                    thread_id: Some(child.to_string()),
                })
                .await
                .ok()?;
            let Some(ClientResponsePayload::ExperimentalFeatureList(response)) = response else {
                return None;
            };
            if response.data.len() > 100 {
                return None;
            }
            for feature in response.data {
                if names.insert(feature.name.clone())
                    && feature.name == Feature::ApiKeyCyberAccessPrograms.key()
                    && feature.enabled
                {
                    enabled = true;
                }
            }
            cursor = response.next_cursor;
            let Some(next) = cursor.as_ref() else {
                return Some(enabled);
            };
            if !cursors.insert(next.clone()) {
                return None;
            }
        }
        None
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), discovery)
        .await
        .ok()
        .flatten()
        .unwrap_or(false)
}

pub(crate) async fn program(
    accounts: &AccountRequestProcessor,
    catalog: &CatalogRequestProcessor,
    features: &Features,
    child: ThreadId,
    model: &str,
    provider: &str,
    enabled: bool,
) -> Result<Option<codex_app_server_protocol::CyberAccessProgram>, String> {
    if !features.enabled(Feature::CliDaybreak) {
        return Ok(None);
    }
    let (account, models) = tokio::join!(
        accounts.get_account(GetAccountParams {
            refresh_token: false
        }),
        catalog.model_list(ModelListParams {
            cursor: None,
            limit: None,
            include_hidden: Some(true),
            model_provider: None,
        }),
    );
    let account = match account {
        Ok(Some(ClientResponsePayload::GetAccount(response))) => response.account,
        Err(error) if enabled && provider == "openai" => {
            return Err(format!(
                "Could not determine Daybreak account eligibility: {}",
                error.message
            ));
        }
        _ => None,
    };
    let eligible = provider == "openai"
        && match account {
            Some(Account::Chatgpt { .. }) => true,
            Some(Account::ApiKey { .. }) => api_key_programs_enabled(catalog, child).await,
            Some(Account::AmazonBedrock { .. } | Account::Gcp {}) | None => false,
        };
    let programs: Option<codex_protocol::openai_models::ModelAccessPrograms> = match models {
        Ok(Some(ClientResponsePayload::ModelList(response))) => response
            .data
            .into_iter()
            .find(|entry| entry.model == model)
            .and_then(|entry| entry.available_access_programs)
            .map(Into::into),
        _ => None,
    };
    daybreak::program_for_turn(programs.as_ref(), model, eligible, enabled && eligible)
        .map(|program| program.map(Into::into))
}
