// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

impl super::AuthOwner {
    /// Renewal records retain the generation of the request's captured owner.
    pub fn alab_generation(&self) -> u64 {
        self.generation
    }
}

/// Portable comparison data only; a match cannot admit a credential or a turn.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnerIdentity {
    mode: serde_json::Value,
    user: String,
    workspace: String,
    login: Option<String>,
}

impl super::AuthOwner {
    /// Read identity from this owner's actual current source, without bearer extraction.
    pub fn alab_identity_checkpoint(&self) -> Option<serde_json::Value> {
        let auth = self.manager.auth_cached()?;
        if !auth.is_chatgpt_auth()
            || !self.is_current()
            || self.manager.auth_owner_for(&auth).as_ref() != Some(self)
        {
            return None;
        }
        let user = auth.get_chatgpt_user_id()?;
        let workspace = auth.get_account_id()?;
        if user.trim().is_empty() || workspace.trim().is_empty() {
            return None;
        }
        let identity = OwnerIdentity {
            mode: serde_json::to_value(auth.api_auth_mode()).ok()?,
            user,
            workspace,
            login: auth.alab_cached_login_owner_id(),
        };
        if !self.is_current() || self.manager.auth_owner_for(&auth).as_ref() != Some(self) {
            return None;
        }
        serde_json::to_value(identity).ok()
    }

    /// Mirrors same_owner for ChatGPT-backed identity data; runtime admission is separate.
    pub fn alab_matches_identity_checkpoint(&self, checkpoint: &serde_json::Value) -> bool {
        let Ok(saved) = serde_json::from_value::<OwnerIdentity>(checkpoint.clone()) else {
            return false;
        };
        let Some(current) = self
            .alab_identity_checkpoint()
            .and_then(|value| serde_json::from_value::<OwnerIdentity>(value).ok())
        else {
            return false;
        };
        saved.mode == current.mode
            && saved.user == current.user
            && saved.workspace == current.workspace
            && (current.login.is_none() || saved.login == current.login)
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use base64::Engine as _;

    fn auth(user: &str, account: &str) -> CodexAuth {
        let claims = serde_json::json!({"https://api.openai.com/auth": {
            "chatgpt_user_id": user
        }});
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(claims.to_string());
        CodexAuth::from_external_chatgpt_tokens(
            &format!("header.{payload}.signature"),
            account,
            None,
        )
        .unwrap()
    }

    #[test]
    fn portable_owner_data_requires_the_actual_current_account() {
        let a = AuthManager::from_auth_for_testing(auth("user", "account"));
        let owner = a.auth_owner();
        let checkpoint = owner.alab_identity_checkpoint().unwrap();
        let fresh = AuthManager::from_auth_for_testing(auth("user", "account"));
        assert!(
            fresh
                .auth_owner()
                .alab_matches_identity_checkpoint(&checkpoint)
        );
        let other = AuthManager::from_auth_for_testing(auth("user", "other-account"));
        assert!(
            !other
                .auth_owner()
                .alab_matches_identity_checkpoint(&checkpoint)
        );
        let other = AuthManager::from_auth_for_testing(auth("other-user", "account"));
        assert!(
            !other
                .auth_owner()
                .alab_matches_identity_checkpoint(&checkpoint)
        );
    }

    #[test]
    fn unknown_or_malformed_owner_data_does_not_match() {
        let manager = AuthManager::from_auth_for_testing(CodexAuth::from_api_key("fixture"));
        assert!(manager.auth_owner().alab_identity_checkpoint().is_none());
        let manager = AuthManager::from_auth_for_testing(auth("user", "account"));
        assert!(
            !manager
                .auth_owner()
                .alab_matches_identity_checkpoint(&serde_json::json!({}))
        );
    }
}
