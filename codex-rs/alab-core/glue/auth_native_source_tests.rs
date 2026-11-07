// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::token_data::AccountOAuthProvenance;
use crate::token_data::TokenData;
use crate::token_data::parse_chatgpt_jwt_claims;
use base64::Engine as _;
use serde_json::json;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

const CHILD: &str = "ALAB_NATIVE_SOURCE_CASE";

fn child(case: &str) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let name = concat!(module_path!(), "::native_source_child")
        .split_once("::")
        .unwrap()
        .1;
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name])
        .env(CHILD, case)
        .env("CODEX_HOME", &root)
        .env(
            codex_alab_core::home::AUTH_PATH_ENV,
            root.join("startup.json"),
        )
        .env(managed_auth::SLOT_ENV, "6f6c64")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
}

#[test]
fn native_source_binding_preserves_policy_and_actual_owner_generations() {
    child("binding");
}

#[test]
fn native_source_first_switch_requires_real_unassigned_startup_proof_and_committed_destination() {
    child("bootstrap");
}

#[test]
fn native_source_cancelled_before_install_restores_only_its_unchanged_source() {
    child("restore");
}

#[test]
fn acknowledged_scope_allows_verified_same_grant_refresh_but_rejects_changed_authority() {
    child("scope");
}

#[test]
fn acknowledged_scope_preserves_identifiable_oauth_session_counters() {
    child("scope-jwt");
}

#[test]
fn native_source_refresh_and_logout_do_not_touch_startup_source() {
    child("refresh");
}

#[test]
fn native_source_invalid_unreadable_and_replaced_files_keep_the_cached_owner() {
    child("replaced");
}

fn document(user: &str, account: &str, refresh: &str) -> AuthDotJson {
    let b64 = |bytes: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let payload = json!({"sub":user,"email":"fixture@example.test","https://api.openai.com/auth":{"chatgpt_user_id":user,"chatgpt_account_id":account,"chatgpt_plan_type":"pro"}});
    let jwt = format!(
        "{}.{}.sig",
        b64(br#"{"alg":"none"}"#),
        b64(payload.to_string().as_bytes())
    );
    AuthDotJson {
        auth_mode: Some(AuthMode::Chatgpt),
        openai_api_key: None,
        tokens: Some(TokenData {
            id_token: parse_chatgpt_jwt_claims(&jwt).unwrap(),
            access_token: format!("fixture-access-{refresh}"),
            refresh_token: refresh.into(),
            account_id: Some(account.into()),
            login_owner_id: Some(format!("fixture-login-{user}")),
            oauth_provenance: Some(AccountOAuthProvenance {
                issuer: "https://issuer.example.test".into(),
                client_id: "fixture-client".into(),
            }),
        }),
        last_refresh: Some(chrono::Utc::now()),
        agent_identity: None,
        personal_access_token: None,
        bedrock_api_key: None,
        bedrock_access_keys: None,
    }
}

fn access_session(document: &mut AuthDotJson, nonce: &str) {
    let tokens = document.tokens.as_mut().unwrap();
    let payload = json!({
        "iss":"https://issuer.example.test", "session_id":"fixture-managed-session",
        "sub":tokens.id_token.chatgpt_user_id, "client_id":"fixture-client", "jti":nonce,
        "https://api.openai.com/auth": {
            "chatgpt_user_id":tokens.id_token.chatgpt_user_id,
            "chatgpt_account_id":tokens.account_id,
        }
    });
    let b64 = |bytes: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    tokens.access_token = format!(
        "{}.{}.sig",
        b64(br#"{"alg":"none"}"#),
        b64(payload.to_string().as_bytes())
    );
}

fn write(path: &Path, document: &AuthDotJson) {
    codex_alab_core::home::save_auth_bytes(path, &serde_json::to_vec(document).unwrap()).unwrap();
}

fn marker(path: &Path, slot: &str) {
    codex_alab_core::home::save_auth_bytes(
        &managed_auth::marker_path(path),
        format!("{}{slot}\n", managed_auth::SLOT_MARKER_PREFIX).as_bytes(),
    )
    .unwrap();
}

fn source(path: PathBuf, document: &AuthDotJson) -> native_source::Source {
    let tokens = document.tokens.as_ref().unwrap();
    native_source::Source {
        operation: "a".repeat(32),
        slot: "6e6577".into(),
        path,
        account: principal(
            b"aterm/codex-renewal/account/v1\0",
            tokens.account_id.as_deref().unwrap(),
        ),
        user: principal(
            b"aterm/codex-renewal/user/v1\0",
            tokens.id_token.chatgpt_user_id.as_deref().unwrap(),
        ),
        revision: managed_auth::Revision {
            generation: 1,
            digest: format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(document).unwrap())
            ),
        },
        assignment_generation: 1,
        native_intent: Some("a".repeat(32)),
    }
}

struct Owner {
    source: Mutex<native_source::Source>,
    next: Mutex<Option<AuthDotJson>>,
    refreshes: AtomicUsize,
}

impl native_source::Host for Owner {
    fn source<'a>(&'a self, operation: &'a str) -> native_source::SourceFuture<'a> {
        Box::pin(async move {
            let mut source = self.source.lock().unwrap().clone();
            source.operation = operation.into();
            Ok(source)
        })
    }
}

impl managed_auth::Host for Owner {
    fn refresh<'a>(
        &'a self,
        request: &'a managed_auth::Request,
    ) -> managed_auth::RefreshFuture<'a> {
        Box::pin(async move {
            let mut source = self.source.lock().unwrap();
            if request.slot != source.slot || request.expected_digest != source.revision.digest {
                return Err(managed_auth::unavailable());
            }
            let document = self
                .next
                .lock()
                .unwrap()
                .take()
                .ok_or_else(managed_auth::unavailable)?;
            write(&source.path, &document);
            source.revision = managed_auth::Revision {
                generation: source.revision.generation + 1,
                digest: format!(
                    "{:x}",
                    Sha256::digest(serde_json::to_vec(&document).unwrap())
                ),
            };
            self.refreshes.fetch_add(1, Ordering::SeqCst);
            Ok(source.revision.clone())
        })
    }
}

async fn manager(home: &Path, workspaces: Option<Vec<String>>) -> AuthManager {
    AuthManager::new(
        home.to_owned(),
        false,
        AuthCredentialsStoreMode::File,
        workspaces,
        None,
        AuthKeyringBackendKind::default(),
        crate::test_support::transport_default_auth_route_config(),
    )
    .await
}

fn boundary(manager: &AuthManager, operation: &str) -> codex_alab_core::assignment::BoundaryPermit {
    use codex_alab_core::assignment::Intent;
    use codex_alab_core::assignment::Selection;
    let domain = manager.alab_assignment_domain();
    domain
        .request(Intent {
            operation: operation.into(),
            thread: "00000000-0000-0000-0000-000000000000".into(),
            selection: Selection::Manual {
                account: "c".repeat(64),
                revision: "d".repeat(64),
            },
        })
        .unwrap();
    let retirement = domain.try_retire(operation).unwrap();
    let mut boundary = retirement.freeze().ok().unwrap();
    boundary.set_native_intent(operation.into()).unwrap();
    boundary
}

struct NoExternal;
impl ExternalAuth for NoExternal {
    fn resolve(&self) -> ExternalAuthFuture<'_, CodexAuth> {
        panic!("selected native source must reject an external provider before resolving it")
    }
    fn refresh(&self, _context: ExternalAuthRefreshContext) -> ExternalAuthFuture<'_, CodexAuth> {
        panic!("no external refresh")
    }
}

#[tokio::test]
async fn native_source_child() {
    let Ok(case) = std::env::var(CHILD) else {
        return;
    };
    codex_alab_core::activate();
    let home = PathBuf::from(std::env::var_os("CODEX_HOME").unwrap())
        .canonicalize()
        .unwrap();
    let startup = home.join("startup.json");
    let selected = home.join("selected.json");
    let old = document("user-old", "account-old", "old");
    let mut new = document("user-new", "account-new", "new");
    let mut rotated = document("user-new", "account-new", "rotated");
    if case == "scope-jwt" {
        access_session(&mut new, "before");
        access_session(&mut rotated, "after");
    }
    write(&startup, &old);
    marker(&startup, "6f6c64");
    write(&selected, &new);
    marker(&selected, "6e6577");
    let original = std::fs::read(&startup).unwrap();
    let mut proof = source(selected.clone(), &new);
    if case == "bootstrap" || case == "restore" {
        proof = source(startup.clone(), &old);
        proof.slot = "6f6c64".into();
        proof.assignment_generation = 0;
        proof.native_intent = None;
    }
    let owner = Arc::new(Owner {
        source: Mutex::new(proof.clone()),
        next: Mutex::new(Some(rotated)),
        refreshes: AtomicUsize::new(0),
    });
    native_source::install(owner.clone());
    managed_auth::install(owner.clone());
    let selected_manager = manager(&home, None).await;
    assert_eq!(
        selected_manager
            .auth_cached()
            .unwrap()
            .get_account_id()
            .as_deref(),
        Some("account-old")
    );

    if case == "bootstrap" || case == "restore" {
        let before = *selected_manager.auth_change_state_tx.borrow();
        let mut fence = selected_manager.alab_native_source_fence().await.unwrap();
        assert_eq!(fence.assignment_generation(), 0);
        assert_eq!(fence.credential_generation(), 1);
        let permit = boundary(&selected_manager, &"a".repeat(32));
        let pinned = selected_manager
            .alab_pin_current_native_source(&mut fence, &permit)
            .await
            .unwrap();
        assert!(pinned.generation > before.generation);
        assert_eq!(pinned.owner_generation, before.owner_generation);
        assert_eq!(pinned.source_generation, before.source_generation);
        assert_eq!(pinned.login_generation, before.login_generation);
        assert!(
            selected_manager
                .alab_rebind_native_source_fenced(&"a".repeat(32), &fence, &permit)
                .await
                .is_err(),
            "unassigned startup proof cannot acknowledge a destination"
        );
        assert_eq!(*selected_manager.auth_change_state_tx.borrow(), pinned);
        assert_eq!(selected_manager.alab_login_file(), Some(startup.clone()));
        let terminal = codex_alab_core::assignment_service::Status {
            operation: "a".repeat(32),
            phase: codex_alab_core::assignment_service::Phase::Cancelled,
            boundary: None,
            assignment_generation: None,
            credential_generation: None,
            target: None,
            mode: None,
            reason: Some(codex_alab_core::assignment_service::Reason::Cancelled),
        };
        if case == "restore" {
            let restored = selected_manager
                .alab_restore_unchanged_native_source(&mut fence, &permit, &terminal)
                .await
                .unwrap();
            assert_eq!(restored.owner_generation, before.owner_generation);
            assert_eq!(
                selected_manager
                    .auth_cached()
                    .unwrap()
                    .get_account_id()
                    .as_deref(),
                Some("account-old")
            );
            let binding = permit.bound().unwrap();
            permit.record_resources_ready(&binding).unwrap();
            let completion = codex_alab_core::assignment::AbortCompletion {
                operation: permit.operation().into(),
                boundary: permit.boundary().into(),
                binding,
            };
            permit.abort(completion).unwrap();
            assert_eq!(
                selected_manager.alab_assignment_domain().phase(),
                codex_alab_core::assignment::Phase::Open
            );
            assert_eq!(std::fs::read(&startup).unwrap(), original);
            return;
        }
        *owner.source.lock().unwrap() = source(selected.clone(), &new);
        let after = selected_manager
            .alab_rebind_native_source_fenced(&"a".repeat(32), &fence, &permit)
            .await
            .unwrap();
        assert!(after.owner_generation > before.owner_generation);
        assert!(after.source_generation > before.source_generation);
        assert!(after.login_generation > before.login_generation);
        assert_eq!(selected_manager.alab_login_file(), Some(selected.clone()));
        assert_eq!(
            selected_manager
                .auth_cached()
                .unwrap()
                .get_account_id()
                .as_deref(),
            Some("account-new")
        );
        assert!(
            selected_manager
                .alab_restore_unchanged_native_source(&mut fence, &permit, &terminal)
                .await
                .is_err(),
            "an installed B cannot restore original A as unchanged"
        );
        assert_eq!(*selected_manager.auth_change_state_tx.borrow(), after);
        assert_eq!(std::fs::read(&startup).unwrap(), original);
        return;
    }

    if case == "binding" {
        let mut false_bootstrap = proof.clone();
        false_bootstrap.assignment_generation = 0;
        false_bootstrap.native_intent = None;
        *owner.source.lock().unwrap() = false_bootstrap;
        assert!(
            selected_manager.alab_native_source_fence().await.is_err(),
            "bootstrap does not permit a different grant"
        );
        *owner.source.lock().unwrap() = proof.clone();
        assert!(
            selected_manager.alab_native_source_fence().await.is_err(),
            "a different native principal cannot prove the source being retired"
        );
        let api = AuthManager::from_auth_for_testing_with_home(
            CodexAuth::from_api_key("fixture-api-key"),
            home.clone(),
        );
        let api_before = *api.auth_change_state_tx.borrow();
        assert!(
            api.alab_rebind_native_source(
                &proof.operation,
                api_before,
                &boundary(&api, &proof.operation)
            )
            .await
            .is_err(),
            "an unrelated native file cannot bootstrap an API-key consumer"
        );
        let restricted = manager(&home, Some(vec!["account-old".into()])).await;
        let before = *restricted.auth_change_state_tx.borrow();
        let restricted_boundary = boundary(&restricted, &proof.operation);
        let original_state = *selected_manager.auth_change_state_tx.borrow();
        assert!(
            selected_manager
                .alab_rebind_native_source(&proof.operation, original_state, &restricted_boundary)
                .await
                .is_err(),
            "a different manager's boundary cannot authorize source publication"
        );
        assert!(
            restricted
                .alab_rebind_native_source(&proof.operation, before, &restricted_boundary)
                .await
                .is_err()
        );
        assert_eq!(*restricted.auth_change_state_tx.borrow(), before);
        assert_eq!(
            restricted
                .auth_cached()
                .unwrap()
                .get_account_id()
                .as_deref(),
            Some("account-old")
        );
        let modes = manager(&home, None).await;
        modes.restrict_auth_modes(vec![AuthMode::ApiKey]).unwrap();
        let modes_before = *modes.auth_change_state_tx.borrow();
        assert!(
            modes
                .alab_rebind_native_source(
                    &proof.operation,
                    modes_before,
                    &boundary(&modes, &proof.operation)
                )
                .await
                .is_err()
        );
    }

    let before = *selected_manager.auth_change_state_tx.borrow();
    let permit = boundary(&selected_manager, &proof.operation);
    let after = selected_manager
        .alab_rebind_native_source(&proof.operation, before, &permit)
        .await
        .unwrap();
    assert!(
        after.generation > before.generation
            && after.owner_generation > before.owner_generation
            && after.source_generation > before.source_generation
            && after.login_generation > before.login_generation
    );
    assert_eq!(
        selected_manager
            .auth_cached()
            .unwrap()
            .get_account_id()
            .as_deref(),
        Some("account-new")
    );
    assert_eq!(selected_manager.alab_login_file(), Some(selected.clone()));
    assert_eq!(
        selected_manager
            .alab_managed_binding(new.tokens.as_ref().unwrap())
            .unwrap()
            .as_deref(),
        Some("6e6577")
    );
    assert_eq!(crate::auth::storage::get_auth_file(&home), startup);
    assert_eq!(std::env::var(managed_auth::SLOT_ENV).unwrap(), "6f6c64");
    assert!(
        selected_manager
            .set_external_auth(Arc::new(NoExternal))
            .await
            .is_err()
    );

    match case.as_str() {
        "binding" => {
            let fence = selected_manager.alab_native_source_fence().await.unwrap();
            assert_eq!(fence.auth_state(), after);
            assert_eq!(fence.credential_generation(), proof.revision.generation);
            assert_eq!(fence.assignment_generation(), proof.assignment_generation);
            assert!(
                selected_manager.refresh_lock.try_acquire().is_err(),
                "refresh cannot cross native commit and source publication"
            );
            assert_eq!(
                selected_manager
                    .alab_rebind_native_source_fenced(&proof.operation, &fence, &permit)
                    .await
                    .unwrap(),
                after
            );
            let foreign = manager(&home, None).await;
            assert!(
                foreign
                    .alab_rebind_native_source_fenced(&proof.operation, &fence, &permit)
                    .await
                    .is_err()
            );
            drop(fence);
            assert!(selected_manager.refresh_lock.try_acquire().is_ok());
            assert_eq!(
                selected_manager
                    .alab_rebind_native_source(&proof.operation, before, &permit)
                    .await
                    .unwrap(),
                after,
                "a repeated receipt must not advance auth generations again"
            );
            let mut invalid = proof.clone();
            invalid.revision.digest = "0".repeat(64);
            *owner.source.lock().unwrap() = invalid;
            assert!(
                selected_manager
                    .alab_rebind_native_source(&proof.operation, after, &permit)
                    .await
                    .is_err()
            );
            assert_eq!(*selected_manager.auth_change_state_tx.borrow(), after);
            assert_eq!(
                selected_manager
                    .auth_cached()
                    .unwrap()
                    .get_account_id()
                    .as_deref(),
                Some("account-new")
            );
        }
        "scope" | "scope-jwt" => {
            let fence = selected_manager.alab_native_source_fence().await.unwrap();
            let scope = fence.scope();
            drop(fence);
            let binding = permit.bound().unwrap();
            permit
                .destination(&binding, async { selected_manager.refresh_token().await })
                .await
                .unwrap()
                .unwrap();
            let fresh = selected_manager.alab_native_source_fence().await.unwrap();
            assert!(fresh.credential_generation() > scope.source.revision.generation);
            assert!(
                fresh.continues_scope(&scope),
                "actual verified same-grant refresh may advance C after ACK"
            );
            if case == "scope-jwt" {
                assert_eq!(
                    fresh.expected.source_generation, scope.expected.source_generation,
                    "an identifiable OAuth session preserves the actual source counter"
                );
            } else {
                assert!(
                    fresh.expected.source_generation > scope.expected.source_generation,
                    "opaque access-token refresh preserves the truthful upstream source-counter advance"
                );
            }
            for field in 0..12 {
                let mut changed = fresh.scope();
                match field {
                    0 => changed.domain = Arc::new(codex_alab_core::assignment::Domain::default()),
                    1 => changed.source.slot = "00".into(),
                    2 => changed.source.path = startup.clone(),
                    3 => changed.source.account = "0".repeat(64),
                    4 => changed.source.user = "0".repeat(64),
                    5 => changed.source.assignment_generation += 1,
                    6 => changed.source.native_intent = Some("0".repeat(32)),
                    7 => changed.expected.owner_generation += 1,
                    8 => changed.expected.source_generation = fresh.expected.source_generation + 1,
                    9 => changed.expected.login_generation += 1,
                    10 => changed.grant.login_owner_id = Some("different-login".into()),
                    11 => {
                        changed.grant.oauth_provenance.as_mut().unwrap().client_id =
                            "different-client".into()
                    }
                    _ => unreachable!(),
                }
                assert!(
                    !fresh.continues_scope(&changed),
                    "changed authority field {field}"
                );
            }
            let mut changed = fresh.scope();
            changed.source.revision.digest = "0".repeat(64);
            assert!(
                !fresh.continues_scope(&changed),
                "same generation cannot change digest"
            );
            let mut changed = fresh.scope();
            changed.source.revision.generation += 1;
            assert!(
                !fresh.continues_scope(&changed),
                "rollback cannot inherit an ACK"
            );
        }
        "refresh" => {
            let binding = permit.bound().unwrap();
            permit.record_resources_ready(&binding).unwrap();
            let completion = codex_alab_core::assignment::Completion {
                operation: permit.operation().into(),
                native_intent: permit.native_intent().unwrap().into(),
                boundary: permit.boundary().into(),
                binding,
            };
            permit.finish(completion).unwrap();
            selected_manager.refresh_token().await.unwrap();
            assert_eq!(owner.refreshes.load(Ordering::SeqCst), 1);
            let refreshed = *selected_manager.auth_change_state_tx.borrow();
            assert_eq!(refreshed.owner_generation, after.owner_generation);
            assert_eq!(
                selected_manager
                    .auth_cached()
                    .unwrap()
                    .get_token_data()
                    .unwrap()
                    .refresh_token,
                "rotated"
            );
            assert!(selected_manager.logout_with_revoke().await.unwrap());
            assert!(selected_manager.auth_cached().is_none());
            selected_manager.reload().await;
            assert!(selected_manager.auth_cached().is_none());
            assert!(selected.exists() && managed_auth::marker_path(&selected).exists());
        }
        "replaced" => {
            std::fs::remove_file(&selected).unwrap();
            assert!(!selected_manager.reload().await);
            assert!(selected_manager.reload_with_cached_auth().await.is_err());
            assert_eq!(
                selected_manager
                    .auth_cached()
                    .unwrap()
                    .get_account_id()
                    .as_deref(),
                Some("account-new")
            );
            write(
                &selected,
                &document("user-other", "account-other", "replacement"),
            );
            assert!(!selected_manager.reload().await);
            assert_eq!(
                selected_manager
                    .auth_cached()
                    .unwrap()
                    .get_account_id()
                    .as_deref(),
                Some("account-new")
            );
            std::fs::remove_file(&selected).unwrap();
            #[cfg(unix)]
            std::os::unix::fs::symlink(&startup, &selected).unwrap();
            assert!(!selected_manager.reload().await);
            assert_eq!(*selected_manager.auth_change_state_tx.borrow(), after);
        }
        _ => panic!("unknown fixture case"),
    }
    assert_eq!(std::fs::read(&startup).unwrap(), original);
    let untouched = manager(&home, None).await;
    assert_eq!(
        untouched.auth_cached().unwrap().get_account_id().as_deref(),
        Some("account-old")
    );
}
