//! Keep optional security reminders on the app-server's existing runtime identity.
//! Only saved ChatGPT credentials matching the active login may fetch a notice.

use super::*;
use codex_app_server_protocol::AccountSecuritySetupAction;
use codex_app_server_protocol::GetAccountSecuritySetupResponse;
use codex_http_client::ClientRouteClass;
use codex_http_client::RouteAwareClientPool;
use serde::Deserialize;

#[derive(Deserialize)]
struct Response {
    notice: Option<Notice>,
}

#[derive(Deserialize)]
struct Notice {
    title: String,
    description: String,
    action: AccountSecuritySetupAction,
}

impl AccountRequestProcessor {
    pub(crate) async fn get_account_security_setup(&self) -> GetAccountSecuritySetupResponse {
        let mut verified_identity = None; // ALAB(notices): failure never manufactures an absence receipt.
        let notice = tokio::time::timeout(Duration::from_secs(/*secs*/ 3), async {
            let _assignment_resource = self.auth_manager.alab_account_resource().ok()?; // ALAB(accounts): admit the security read before any credential/configuration access.
            let config = self.load_latest_config().await;
            let (auth, factory) = self.auth_manager.auth_with_http_client_factory().await?;
            let source_generation = self
                .auth_manager
                .auth_change_state_for(&auth)?
                .source_generation;
            let saved = config
                .auth_config()
                .load_auth(/*enable_codex_api_key_env*/ false)
                .await
                .ok()
                .flatten()?;
            if !matches!(saved, CodexAuth::Chatgpt(_)) || saved.is_fedramp_account() {
                return None;
            }
            if auth.auth_mode() != codex_protocol::auth::AuthMode::Chatgpt
                || saved.get_token().ok()? != auth.get_token().ok()?
            {
                return None;
            }
            let account_id = auth.get_account_id()?;
            let user_id = auth.get_chatgpt_user_id()?;
            let url = format!(
                "{}/wham/security-setup",
                config.chatgpt_base_url.trim_end_matches('/')
            );
            let runtime = self
                .thread_manager
                .runtime_auth_for_config(&config)
                .ok()?
                .filter(|_| codex_runtime_agent_instance::is_openai_api_url(&url));
            let resolved = codex_model_provider::auth_provider_for_request(
                &auth,
                runtime.as_ref(),
                /*headers*/ &Default::default(),
            )
            .await
            .ok()?;
            let factory = resolved.bind_http_client_factory(factory).ok()?;
            let client =
                RouteAwareClientPool::new_without_redirects(factory, ClientRouteClass::Api);
            let mut headers = resolved
                .auth
                .apply_auth_headers(Default::default())
                .await
                .ok()?;
            headers.insert("chatgpt-account-id", account_id.parse().ok()?);
            resolved.validate_headers(&headers).ok()?;
            let response = client
                .get(&url)
                .headers(headers)
                .header(
                    "User-Agent",
                    codex_login::default_client::get_codex_user_agent(),
                )
                .send()
                .await
                .ok()?;
            if !response.status().is_success() {
                return None;
            }
            let notice = response.json::<Response>().await.ok()?.notice; // ALAB(notices): successful empty reads retain their authenticated identity.
            let current = self.auth_manager.auth().await?;
            if self
                .auth_manager
                .auth_change_state_for(&auth)?
                .source_generation
                != source_generation
                || current.auth_mode() != codex_protocol::auth::AuthMode::Chatgpt
                || current.get_account_id().as_ref() != Some(&account_id)
                || current.get_chatgpt_user_id().as_ref() != Some(&user_id)
                || current.is_fedramp_account()
            {
                return None;
            }
            alab_observation::verified(&mut verified_identity, account_id, user_id, notice) // ALAB(notices): only the validated completion can certify an empty reminder.
        })
        .await
        .ok()
        .flatten();
        GetAccountSecuritySetupResponse { notice, alab_successful_empty: verified_identity } // ALAB(notices): expose receipt without changing the upstream notice payload.
    }
}

#[path = "../../../../alab-core/glue/security_setup_observation.rs"]
mod alab_observation; // ALAB(notices): classify successful empty reads in ALab-owned glue.
