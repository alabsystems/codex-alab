// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::token_data::AccountOAuthProvenance;
use crate::token_data::parse_chatgpt_jwt_claims;
use base64::Engine as _;
use codex_alab_core::native_source as native;
use sha2::Digest;
use sha2::Sha256;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

const CHILD: &str = "ALAB_NATIVE_MEMBER_CASE";
fn child(case: &str) {
    let root = tempfile::tempdir().unwrap();
    let root = root.path().canonicalize().unwrap();
    let name = concat!(module_path!(), "::constructor_child")
        .split_once("::")
        .unwrap()
        .1;
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", name])
        .env(CHILD, case)
        .env("CODEX_HOME", &root)
        .env(
            codex_alab_core::home::AUTH_PATH_ENV,
            root.join("startup.json"),
        )
        .env(native::LAUNCH_ENV, "fixture-native-constructor")
        .env_remove(codex_alab_core::managed_auth::SLOT_ENV)
        .env_remove(CODEX_API_KEY_ENV_VAR);
    if case.starts_with("api") || case == "refused" {
        command.env(CODEX_API_KEY_ENV_VAR, "fixture-api-key");
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
}
#[test]
fn native_constructor_refusal_precedes_api_and_file_fallback() {
    child("refused");
}
#[test]
fn native_api_manager_has_real_unrelated_completion_and_snapshot_lifetime() {
    child("api_manager");
}
#[test]
fn native_api_direct_reader_has_real_unrelated_completion_and_snapshot_lifetime() {
    child("api_direct");
}
#[test]
fn native_manager_preloads_verified_private_source_before_loading_and_records_actual_state() {
    child("managed");
}
#[test]
fn native_manager_policy_refusal_clears_cache_before_unavailable_completion() {
    child("policy");
}
#[test]
fn native_manager_eof_fences_retained_snapshot_and_last_snapshot_retires() {
    child("eof");
}

#[test]
fn native_api_reload_never_falls_through_to_startup_file() {
    child("api_reload");
}
#[test]
fn native_api_logout_seals_snapshots_without_deleting_startup_file() {
    child("api_logout");
}
#[test]
fn native_api_pinned_reload_preserves_restricted_modes() {
    child("api_policy");
}

#[test]
fn native_scope_never_inherits_an_ack_from_a_different_member() {
    child("member_replacement");
}

#[test]
fn native_source_tampering_never_claims_policy_unavailable() {
    child("tampered");
}
#[test]
fn native_mode_policy_refusal_does_not_read_credentials() {
    child("mode");
}
#[test]
fn native_ready_refusal_never_claims_local_policy_unavailable() {
    child("ready_refused");
}

#[test]
fn native_managed_direct_reader_owns_real_reader_lifetime() {
    child("managed_direct");
}

#[test]
fn native_refresh_registers_new_revision_before_read_and_actual_ready_after_reload() {
    child("managed_refresh");
}

#[test]
fn native_registered_refresh_calls_contextual_owner_before_flock_without_legacy_host() {
    child("managed_owner_refresh");
}

#[test]
fn native_network_permit_keeps_last_use_and_revokes_on_transport_eof() {
    child("network");
}
#[test]
fn native_network_factory_cannot_take_another_members_credentials() {
    child("network_cross");
}

#[test]
fn native_http_body_blocks_assignment_until_body_drop_without_counting_idle_client() {
    child("network_body");
}

#[test]
fn native_managed_policy_refusal_revokes_retained_credential_and_request_scope() {
    child("managed_policy");
}

#[test]
fn native_ack_scope_cannot_transfer_to_a_new_lifetime_with_same_server_member_id() {
    child("member_reincarnation");
}

fn document() -> AuthDotJson {
    let b64 = |bytes: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let payload = serde_json::json!({"sub":"user-b","https://api.openai.com/auth":{"chatgpt_user_id":"user-b","chatgpt_account_id":"account-b"}});
    let jwt = format!(
        "{}.{}.fixture",
        b64(br#"{"alg":"none"}"#),
        b64(payload.to_string().as_bytes())
    );
    AuthDotJson {
        auth_mode: Some(AuthMode::Chatgpt),
        openai_api_key: None,
        tokens: Some(TokenData {
            id_token: parse_chatgpt_jwt_claims(&jwt).unwrap(),
            access_token: "fixture-access".into(),
            refresh_token: "fixture-refresh".into(),
            account_id: Some("account-b".into()),
            login_owner_id: Some("fixture-login".into()),
            oauth_provenance: Some(AccountOAuthProvenance {
                issuer: "https://auth.openai.com".into(),
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
fn principal(domain: &[u8], value: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(domain);
    hash.update(value);
    format!("{:x}", hash.finalize())
}
struct Peer {
    source: native::Source,
    case: String,
    live: AtomicBool,
    opened: AtomicUsize,
    preloaded: AtomicUsize,
    ready: Mutex<Vec<Option<codex_alab_core::assignment::Generations>>>,
    unavailable: AtomicUsize,
    retired: AtomicUsize,
    changed: tokio::sync::Notify,
    owner_refreshed: AtomicBool,
    owner_requests: Mutex<Vec<codex_alab_core::managed_auth::Request>>,
}
struct Host(Arc<Peer>);
impl native::Host for Host {
    fn source<'a>(&'a self, operation: &'a str) -> native::SourceFuture<'a> {
        Box::pin(async move {
            let mut source = self.0.source.clone();
            source.operation = operation.into();
            Ok(source)
        })
    }
    fn open_member<'a>(&'a self, _: &'a str) -> native::MemberFuture<'a> {
        Box::pin(async move {
            let ordinal = self.0.opened.fetch_add(1, Ordering::Relaxed) + 1;
            if self.0.case == "refused" {
                return Err(codex_alab_core::managed_auth::unavailable());
            }
            Ok(Arc::new(MemberHandle {
                peer: Arc::clone(&self.0),
                use_serial: Arc::new(tokio::sync::Semaphore::new(1)),
                use_next: Default::default(),
                id: format!(
                    "{:032x}",
                    if self.0.case == "member_reincarnation" {
                        1
                    } else {
                        ordinal
                    }
                ),
            }) as Arc<dyn native::Member>)
        })
    }
}
struct MemberHandle {
    peer: Arc<Peer>,
    use_serial: Arc<tokio::sync::Semaphore>,
    use_next: std::sync::atomic::AtomicU64,
    id: String,
}
impl native::Member for MemberHandle {
    fn id(&self) -> &str {
        &self.id
    }
    fn live(&self) -> bool {
        self.peer.live.load(Ordering::Acquire)
    }
    fn preload<'a>(
        &'a self,
        operation: &'a str,
        borrow: Option<&'a native::Borrow>,
    ) -> native::PreloadFuture<'a> {
        <Peer as native::Member>::preload(self.peer.as_ref(), operation, borrow)
    }
    fn ready<'a>(
        &'a self,
        operation: &'a str,
        registration: Option<&'a str>,
        loaded: &'a native::Loaded,
    ) -> native::ReadyFuture<'a> {
        <Peer as native::Member>::ready(self.peer.as_ref(), operation, registration, loaded)
    }
    fn begin_use<'a>(
        &'a self,
        attempt: &'a codex_alab_core::native_source_use::UseAttempt,
        registration: &'a str,
    ) -> codex_alab_core::native_source_use::UseFuture<'a> {
        Box::pin(async move {
            use codex_alab_core::native_source_use::UseBody;
            use codex_alab_core::native_source_use::UseOutcome;
            use codex_alab_core::native_source_use::UseReceipt;
            if !attempt.hold_admission(Arc::clone(&self.use_serial)).await? {
                return Ok(UseOutcome::Unsent);
            }
            let body = match attempt.body()? {
                Some(body) => body,
                None => attempt.assign(UseBody {
                    member: self.id.clone(),
                    operation: format!(
                        "{:016x}{:016x}",
                        1,
                        self.use_next.fetch_add(1, Ordering::Relaxed) + 1
                    ),
                    registration: registration.into(),
                })?,
            };
            Ok(UseOutcome::Active(UseReceipt::from_native(
                body.member,
                body.operation.clone(),
                body.operation.repeat(2),
            )?))
        })
    }
    fn close_use<'a>(
        &'a self,
        body: &'a codex_alab_core::native_source_use::UseBody,
    ) -> codex_alab_core::native_source_use::UseFuture<'a> {
        Box::pin(async move {
            assert_eq!(body.member, self.id);
            Ok(codex_alab_core::native_source_use::UseOutcome::Closed)
        })
    }
    fn end_use<'a>(
        &'a self,
        receipt: &'a codex_alab_core::native_source_use::UseReceipt,
    ) -> native::UnitFuture<'a> {
        Box::pin(async move {
            assert_eq!(receipt.member(), self.id);
            Ok(())
        })
    }
    fn preload_use<'a>(
        &'a self,
        operation: &'a str,
        receipt: &'a codex_alab_core::native_source_use::UseReceipt,
    ) -> native::PreloadFuture<'a> {
        assert_eq!(receipt.member(), self.id);
        self.preload(operation, None)
    }
    fn ready_use<'a>(
        &'a self,
        operation: &'a str,
        registration: &'a str,
        loaded: &'a native::Loaded,
        receipt: &'a codex_alab_core::native_source_use::UseReceipt,
    ) -> native::ReadyFuture<'a> {
        assert_eq!(receipt.member(), self.id);
        self.ready(operation, Some(registration), loaded)
    }
    fn refresh_use<'a>(
        &'a self,
        request: &'a codex_alab_core::managed_auth::Request,
        receipt: &'a codex_alab_core::native_source_use::UseReceipt,
    ) -> codex_alab_core::managed_auth::RefreshFuture<'a> {
        Box::pin(async move {
            assert_eq!(self.peer.case, "managed_owner_refresh");
            assert_eq!(receipt.member(), self.id);
            assert_eq!(request.slot, self.peer.source.slot);
            assert_eq!(
                request.expected_digest,
                format!(
                    "{:x}",
                    Sha256::digest(std::fs::read(&self.peer.source.path).unwrap())
                )
            );
            let _free = codex_alab_core::auth_lock::AuthLock::try_take(&self.peer.source.path)
                .unwrap()
                .expect(
                    "contextual native owner must run BEFORE the caller's materialization flock",
                );
            self.peer
                .owner_requests
                .lock()
                .unwrap()
                .push(request.clone());
            let mut next = document();
            let tokens = next.tokens.as_mut().unwrap();
            tokens.access_token = "fixture-owner-access".into();
            tokens.refresh_token = "fixture-owner-refresh".into();
            let bytes = serde_json::to_vec(&next).unwrap();
            std::fs::write(&self.peer.source.path, &bytes).unwrap();
            self.peer.owner_refreshed.store(true, Ordering::Release);
            Ok(codex_alab_core::managed_auth::Revision {
                generation: 43,
                digest: format!("{:x}", Sha256::digest(bytes)),
            })
        })
    }
    fn retire_captured<'a>(&'a self, request: &'a native::Retirement) -> native::UnitFuture<'a> {
        Box::pin(async move {
            request.validate()?;
            self.retire(request.registration.as_deref()).await
        })
    }
    fn retire<'a>(&'a self, registration: Option<&'a str>) -> native::UnitFuture<'a> {
        <Peer as native::Member>::retire(self.peer.as_ref(), registration)
    }
}
impl native::Member for Peer {
    fn id(&self) -> &str {
        "11111111111111111111111111111111"
    }
    fn live(&self) -> bool {
        self.live.load(Ordering::Acquire)
    }
    fn preload<'a>(
        &'a self,
        operation: &'a str,
        _: Option<&'a native::Borrow>,
    ) -> native::PreloadFuture<'a> {
        Box::pin(async move {
            assert!(!self.case.starts_with("api"));
            let preload = self.preloaded.fetch_add(1, Ordering::Relaxed);
            let _free = codex_alab_core::auth_lock::AuthLock::try_take(&self.source.path)
                .unwrap()
                .expect("native preload must precede local file lock");
            if self.case == "tampered" {
                std::fs::write(&self.source.path, b"{}").unwrap();
            }
            if self.case == "mode" {
                std::fs::remove_file(&self.source.path).unwrap();
            }
            let mut source = self.source.clone();
            source.operation = operation.into();
            if self.case == "managed_refresh" && preload > 0 {
                let mut next = document();
                let tokens = next.tokens.as_mut().unwrap();
                tokens.access_token = "fixture-refreshed-access".into();
                tokens.refresh_token = "fixture-refreshed-refresh".into();
                let bytes = serde_json::to_vec(&next).unwrap();
                std::fs::write(&source.path, &bytes).unwrap();
                source.revision.generation += 1;
                source.revision.digest = format!("{:x}", Sha256::digest(bytes));
            }
            let owner_ready = self.case == "managed_owner_refresh"
                && self.owner_refreshed.load(Ordering::Acquire);
            if owner_ready {
                source.revision.generation = 43;
                source.revision.digest =
                    format!("{:x}", Sha256::digest(std::fs::read(&source.path).unwrap()));
            }
            Ok(native::Preload {
                source,
                registration: if (self.case == "managed_refresh" && preload > 0) || owner_ready {
                    "3".repeat(32)
                } else {
                    "2".repeat(32)
                },
            })
        })
    }
    fn ready<'a>(
        &'a self,
        operation: &'a str,
        registration: Option<&'a str>,
        loaded: &'a native::Loaded,
    ) -> native::ReadyFuture<'a> {
        Box::pin(async move {
            let _free = codex_alab_core::auth_lock::AuthLock::try_take(&self.source.path)
                .unwrap()
                .expect("native Ready must follow local file unlock");
            match loaded {
                native::Loaded::Source {
                    source,
                    generations,
                } => {
                    assert_eq!(source.path, self.source.path);
                    if matches!(
                        self.case.as_str(),
                        "managed_refresh" | "managed_owner_refresh"
                    ) && source.revision.generation == 43
                    {
                        assert_eq!(source.operation, operation);
                        assert_eq!(registration, Some("33333333333333333333333333333333"));
                        assert_eq!(
                            source.revision.digest,
                            format!("{:x}", Sha256::digest(std::fs::read(&source.path).unwrap()))
                        );
                    } else {
                        assert_eq!(source.revision, self.source.revision);
                    }
                    self.ready.lock().unwrap().push(Some(generations.clone()));
                    if self.case == "ready_refused" {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::PermissionDenied,
                            "native-refused",
                        ));
                    }
                }
                native::Loaded::NoNativeReader(kind) => {
                    assert_eq!(*kind, NoNativeReader::ApiKey);
                    self.ready.lock().unwrap().push(None);
                }
                native::Loaded::ReaderSource { source } => {
                    assert_eq!(self.case, "managed_direct");
                    assert_eq!(source.revision, self.source.revision);
                    self.ready.lock().unwrap().push(None);
                }
                native::Loaded::PolicyUnavailable => {
                    self.unavailable.fetch_add(1, Ordering::Relaxed);
                }
            }
            Ok(native::ReadyReceipt {
                registration: registration
                    .map(str::to_owned)
                    .unwrap_or_else(|| "2".repeat(32)),
            })
        })
    }
    fn retire<'a>(&'a self, _: Option<&'a str>) -> native::UnitFuture<'a> {
        Box::pin(async move {
            self.retired.fetch_add(1, Ordering::Relaxed);
            self.changed.notify_one();
            Ok(())
        })
    }
}
fn config(home: PathBuf, case: &str) -> AuthConfig {
    AuthConfig {
        codex_home: home,
        auth_credentials_store_mode: AuthCredentialsStoreMode::File,
        keyring_backend_kind: AuthKeyringBackendKind::default(),
        forced_login_method: None,
        chatgpt_base_url: None,
        forced_chatgpt_workspace_id: if case == "policy" {
            Some(vec!["excluded-account".into()])
        } else {
            None
        },
        managed_auth_policy: ManagedAuthPolicy::default(),
        auth_route_config: crate::test_support::transport_default_auth_route_config(),
    }
}
#[tokio::test]
async fn constructor_child() {
    let Ok(case) = std::env::var(CHILD) else {
        return;
    };
    codex_alab_core::activate();
    let home = PathBuf::from(std::env::var_os("CODEX_HOME").unwrap())
        .canonicalize()
        .unwrap();
    let original = b"must-not-read-or-replace-startup-file";
    let startup = home.join("startup.json");
    std::fs::write(&startup, original).unwrap();
    let selected = home.join("native.json");
    let bytes = serde_json::to_vec(&document()).unwrap();
    codex_alab_core::home::save_auth_bytes(&selected, &bytes).unwrap();
    codex_alab_core::home::save_auth_bytes(
        &codex_alab_core::managed_auth::marker_path(&selected),
        b"ATERM-CODEX-AUTH-SLOT 1 736c6f74\n",
    )
    .unwrap();
    let peer = Arc::new(Peer {
        source: native::Source {
            operation: "b".repeat(32),
            slot: "736c6f74".into(),
            path: selected.clone(),
            account: principal(b"aterm/codex-renewal/account/v1\0", "account-b"),
            user: principal(b"aterm/codex-renewal/user/v1\0", "user-b"),
            revision: codex_alab_core::managed_auth::Revision {
                generation: 42,
                digest: format!("{:x}", Sha256::digest(&bytes)),
            },
            assignment_generation: 3,
            native_intent: Some("f".repeat(32)),
        },
        case: case.clone(),
        live: AtomicBool::new(true),
        opened: AtomicUsize::new(0),
        preloaded: AtomicUsize::new(0),
        ready: Mutex::default(),
        unavailable: AtomicUsize::new(0),
        retired: AtomicUsize::new(0),
        changed: Default::default(),
        owner_refreshed: AtomicBool::new(false),
        owner_requests: Mutex::default(),
    });
    native::install(Arc::new(Host(Arc::clone(&peer))));
    let config = config(home.clone(), &case);
    if case == "api_direct" || case == "managed_direct" {
        let snapshot = config.load_auth(true).await.unwrap().unwrap();
        if case == "api_direct" {
            assert_eq!(snapshot.api_key(), Some("fixture-api-key"));
        } else {
            assert_eq!(snapshot.get_account_id().as_deref(), Some("account-b"));
        }
        assert_eq!(*peer.ready.lock().unwrap(), vec![None]);
        peer.live.store(false, Ordering::Release);
        assert!(snapshot.api_key().is_none());
        assert!(snapshot.get_token().is_err());
        drop(snapshot);
        peer.changed.notified().await;
    } else if case == "network_body" {
        let manager = AuthManager::new_from_auth_config(config, true, None).await;
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string("held-body"))
            .expect(1)
            .mount(&server)
            .await;
        let factory = manager.http_client_factory();
        let client = factory
            .build_client(&server.uri(), codex_http_client::ClientRouteClass::Other)
            .unwrap();
        let response = client.get(server.uri()).send().await.unwrap();
        let body = response.bytes_stream();
        let domain = manager.alab_assignment_domain();
        let operation = "a".repeat(32);
        domain
            .request(codex_alab_core::assignment::Intent {
                operation: operation.clone(),
                thread: "00000000-0000-0000-0000-000000000001".into(),
                selection: codex_alab_core::assignment::Selection::Auto {
                    revision: "a".repeat(64),
                },
            })
            .unwrap();
        let retiring = domain.try_retire(&operation).unwrap();
        let retiring = match retiring.freeze() {
            Err(codex_alab_core::assignment::FreezeError::Pending(retiring)) => retiring,
            _ => panic!("actual HTTP response body must hold source activity"),
        };
        drop(body);
        tokio::time::timeout(std::time::Duration::from_secs(10), retiring.drained())
            .await
            .unwrap()
            .unwrap();

        assert!(client.get(server.uri()).send().await.is_err());
        server.verify().await;
    } else if case == "network" || case == "network_cross" {
        let manager = AuthManager::new_from_auth_config(config.clone(), true, None).await;
        let snapshot = manager.auth_cached().unwrap();
        let factory = manager.http_client_factory();
        let permit = factory
            .network_policy()
            .acquire(&"https://example.com/".parse().unwrap())
            .unwrap();
        if case == "network_cross" {
            let other = AuthManager::new_from_auth_config(config, true, None).await;
            assert!(
                other
                    .http_client_factory()
                    .network_policy()
                    .clone()
                    .with_account_binding(factory.network_policy())
                    .is_none()
            );
        } else {
            peer.live.store(false, Ordering::Release);
            assert!(permit.check().is_err());
            tokio::time::timeout(std::time::Duration::from_secs(1), permit.revoked())
                .await
                .unwrap();
            drop(manager);
            drop(snapshot);
            assert_eq!(peer.retired.load(Ordering::Relaxed), 0);
            drop(factory);
            assert_eq!(peer.retired.load(Ordering::Relaxed), 0);
            drop(permit);
            peer.changed.notified().await;
            assert_eq!(peer.retired.load(Ordering::Relaxed), 1);
        }
    } else {
        let mut manager = AuthManager::new_from_auth_config(
            config,
            true,
            if case == "mode" {
                Some(vec![AuthMode::ApiKey])
            } else {
                None
            },
        )
        .await;
        if matches!(
            case.as_str(),
            "refused" | "policy" | "mode" | "tampered" | "ready_refused"
        ) {
            assert!(manager.auth_cached().is_none());
            assert_eq!(
                peer.ready.lock().unwrap().len(),
                usize::from(case == "ready_refused")
            );
            assert_eq!(
                peer.unavailable.load(Ordering::Relaxed),
                usize::from(case == "policy" || case == "mode")
            );
        } else {
            let snapshot = manager.auth_cached().unwrap();
            if case.starts_with("api") {
                assert_eq!(snapshot.api_key(), Some("fixture-api-key"));
                assert_eq!(*peer.ready.lock().unwrap(), vec![None]);
            } else {
                assert_eq!(snapshot.get_account_id().as_deref(), Some("account-b"));
                assert_eq!(manager.alab_login_file(), Some(selected.clone()));
                let actual = *manager.auth_change_state_tx.borrow();
                assert_eq!(
                    *peer.ready.lock().unwrap(),
                    vec![Some(codex_alab_core::assignment::Generations {
                        auth: actual.generation,
                        owner: actual.owner_generation,
                        source: actual.source_generation,
                        login: actual.login_generation
                    })]
                );
            }
            if case == "member_replacement" || case == "member_reincarnation" {
                let fence = manager.alab_native_source_fence().await.unwrap();
                let acknowledged = fence.scope();
                drop(fence);
                let allocation = codex_alab_core::native_source_launch::allocate_manager()
                    .await
                    .unwrap()
                    .unwrap();
                let replacement = Lifetime::new(allocation).unwrap();
                let preload = replacement.preload(&"c".repeat(32), None).await.unwrap();
                *manager.alab_source_member.write().unwrap() = Some(replacement.clone());
                manager
                    .alab_initialize_member_source(preload.source, replacement)
                    .await
                    .unwrap();
                let current = manager.alab_native_source_fence().await.unwrap();
                assert!(!current.continues_scope(&acknowledged));
            }
            if case == "managed_refresh" || case == "managed_owner_refresh" {
                let old = *manager.auth_change_state_receiver().borrow();
                manager.refresh_token_from_authority().await.unwrap();
                let auth = manager.auth_cached().expect("new revision must be Ready");
                assert_eq!(
                    auth.get_token().unwrap(),
                    if case == "managed_owner_refresh" {
                        "fixture-owner-access"
                    } else {
                        "fixture-refreshed-access"
                    }
                );
                if case == "managed_owner_refresh" {
                    assert_eq!(
                        peer.owner_requests.lock().unwrap().len(),
                        1,
                        "registered refresh never uses an absent legacy host"
                    );
                }
                let current = *manager.auth_change_state_receiver().borrow();
                assert!(current.generation > old.generation);
                let ready = peer.ready.lock().unwrap();
                assert_eq!(ready.len(), 2);
                let ready = ready[1].as_ref().unwrap();
                assert_eq!(
                    (ready.auth, ready.owner, ready.source, ready.login),
                    (
                        current.generation,
                        current.owner_generation,
                        current.source_generation,
                        current.login_generation
                    )
                );
                assert_eq!(
                    peer.preloaded.load(Ordering::Relaxed),
                    if case == "managed_owner_refresh" {
                        3
                    } else {
                        2
                    },
                    "native owner publication requires a fresh registered post-refresh read"
                );
            }
            if case == "managed_policy" {
                let factory = manager.http_client_factory();
                let permit = factory
                    .network_policy()
                    .acquire(&"https://example.com/".parse().unwrap())
                    .unwrap();
                manager.restrict_auth_modes(vec![AuthMode::ApiKey]).unwrap();
                manager.reload().await;
                assert!(manager.auth_cached().is_none());
                assert!(snapshot.get_token().is_err());
                assert!(permit.check().is_err());
            }
            if case == "api_reload" {
                manager.enable_codex_api_key_env = false;
                assert!(!manager.reload().await);
                assert_eq!(
                    manager.auth_cached().unwrap().api_key(),
                    Some("fixture-api-key")
                );
            }
            if case == "api_logout" {
                assert!(manager.logout().await.unwrap());
                assert!(manager.auth_cached().is_none());
                assert!(snapshot.get_token().is_err());
                assert!(!manager.reload().await);
                assert!(manager.auth_cached().is_none());
            }
            if case == "api_policy" {
                manager
                    .restrict_auth_modes(vec![AuthMode::Chatgpt])
                    .unwrap();
                manager.reload().await;
                assert!(manager.auth_cached().is_none());
                assert!(snapshot.get_token().is_err());
            }
            if case == "eof" || case == "api_manager" {
                peer.live.store(false, Ordering::Release);
                assert!(manager.auth_cached().is_none());
                assert!(snapshot.get_token().is_err());
                drop(manager);
                tokio::task::yield_now().await;
                assert_eq!(peer.retired.load(Ordering::Relaxed), 0);
                drop(snapshot);
                peer.changed.notified().await;
                assert_eq!(peer.retired.load(Ordering::Relaxed), 1);
            }
        }
    }
    assert_eq!(std::fs::read(startup).unwrap(), original);
    if case == "mode" {
        assert!(!selected.exists());
    } else {
        assert_eq!(
            std::fs::read(&selected).unwrap(),
            if case == "tampered" {
                b"{}".to_vec()
            } else if case == "managed_refresh" || case == "managed_owner_refresh" {
                let mut next = document();
                next.last_refresh =
                    serde_json::from_slice::<AuthDotJson>(&std::fs::read(&selected).unwrap())
                        .unwrap()
                        .last_refresh;
                let tokens = next.tokens.as_mut().unwrap();
                tokens.access_token = if case == "managed_owner_refresh" {
                    "fixture-owner-access"
                } else {
                    "fixture-refreshed-access"
                }
                .into();
                tokens.refresh_token = if case == "managed_owner_refresh" {
                    "fixture-owner-refresh"
                } else {
                    "fixture-refreshed-refresh"
                }
                .into();
                serde_json::to_vec(&next).unwrap()
            } else {
                bytes
            }
        );
    }
    assert_eq!(
        peer.opened.load(Ordering::Relaxed),
        if case == "member_replacement" || case == "member_reincarnation" || case == "network_cross"
        {
            2
        } else {
            1
        }
    );
}
