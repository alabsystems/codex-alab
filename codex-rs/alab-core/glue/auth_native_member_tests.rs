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
static READER_OBSERVER: std::sync::OnceLock<std::sync::Weak<Peer>> = std::sync::OnceLock::new();
// Observe the exact admitted reader occurrence, never a task-local guessed in
// an unrelated spawned native worker. This fixture hook grants no authority.
pub(crate) fn reader_admitted(record: &Arc<codex_alab_core::manager_catalog::Record>) {
    if let Some(peer) = READER_OBSERVER.get().and_then(std::sync::Weak::upgrade)
        && matches!(peer.case.as_str(), "direct_cancel" | "direct_policy")
    {
        assert_eq!(
            record.role().unwrap(),
            codex_alab_core::manager_catalog::Role::Reader
        );
        let mut admitted = peer.direct_record.lock().unwrap();
        assert!(
            admitted.is_none(),
            "fixture expects exactly one reader admission"
        );
        *admitted = Some(record.clone());
    }
}
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
        "native member case {case}\n{}\n{}",
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
fn native_reader_retains_both_actual_constructor_manager_and_reader_permissions() {
    child("managed_reader_parent");
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
fn native_exact_ready_rebind_revalidates_manager_before_publication() {
    for case in [
        "work_rebind_ok",
        "work_rebind_seal",
        "work_rebind_policy",
        "work_rebind_workspace",
        "work_rebind_cache",
        "work_rebind_document",
        "work_rebind_source",
        "work_rebind_member",
        "work_rebind_detach",
    ] {
        child(case);
    }
}
#[test]
fn native_unrelated_completion_never_recaptures_a_foreign_ready_member() {
    child("api_cache_ack_swap");
}
#[test]
fn native_cache_capture_retains_cache_reader_and_rejects_missing_or_foreign_ready_members() {
    child("cache_snapshot");
    child("api_cache_snapshot");
}
#[test]
fn native_changed_source_eof_requires_fresh_constructor_preload_and_actual_cache_ready() {
    child("rebound_changed_no_use");
    child("rebound_changed_prior_use");
    for case in [
        "rebound_changed_gate_cancel",
        "rebound_changed_gate_eof",
        "rebound_changed_gate_seal",
        "rebound_changed_gate_policy",
        "rebound_changed_gate_workspace",
        "rebound_changed_gate_member",
        "rebound_changed_gate_physical",
    ] {
        child(case);
    }
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
fn uncertain_initial_ready_retains_actual_cache_and_replays_without_rereading_source() {
    child("ready_lost");
}

#[test]
fn native_managed_direct_reader_owns_real_reader_lifetime() {
    child("managed_direct");
}
#[test]
fn native_cancelled_direct_reader_waiter_keeps_owned_load_and_closes_exact_reader() {
    child("direct_cancel");
}
#[test]
fn native_excluded_direct_reader_never_loads_credentials_and_still_retires() {
    child("direct_policy");
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
fn native_http_text_body_preserves_authorization_and_retires_before_next_request() {
    child("network_text");
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
    work: std::sync::atomic::AtomicU64,
    recovery_started: tokio::sync::Notify,
    recovery_release: tokio::sync::Notify,
    opened: AtomicUsize,
    preloaded: AtomicUsize,
    begins: AtomicUsize,
    ready: Mutex<Vec<Option<codex_alab_core::assignment::Generations>>>,
    unavailable: AtomicUsize,
    retired: AtomicUsize,
    changed: tokio::sync::Notify,
    owner_refreshed: AtomicBool,
    owner_requests: Mutex<Vec<codex_alab_core::managed_auth::Request>>,
    direct_started: tokio::sync::Notify,
    direct_release: tokio::sync::Notify,
    direct_record: Mutex<Option<Arc<codex_alab_core::manager_catalog::Record>>>,
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
    fn work_incarnation(&self) -> u64 {
        self.peer.work.load(Ordering::Acquire)
    }
    fn reconcile_work(&self) -> native::UnitFuture<'_> {
        Box::pin(async move {
            if (self.peer.case.starts_with("work_rebind_")
                || self.peer.case.starts_with("rebound_changed"))
                && !self.peer.live.load(Ordering::Acquire)
            {
                self.peer.work.fetch_add(1, Ordering::AcqRel);
                self.peer.live.store(true, Ordering::Release);
            }
            Ok(())
        })
    }
    fn rebound_ready_owned<'a>(
        &'a self,
        attempt: &'a codex_alab_core::native_preload::Body,
        registration: &'a str,
        loaded: &'a native::Loaded,
    ) -> native::ReboundReadyFuture<'a> {
        Box::pin(async move {
            if self.peer.case.starts_with("rebound_changed") {
                let native::Loaded::Source { source, .. } = loaded else {
                    panic!("manager report")
                };
                let actual = std::fs::read(&source.path)?;
                if format!("{:x}", Sha256::digest(actual)) != source.revision.digest {
                    return Ok(native::ReboundReadyOutcome::SourceChanged);
                }
                return Ok(native::ReboundReadyOutcome::Ready(
                    codex_alab_core::native_preload::Ready::from_native(
                        attempt.clone(),
                        registration.into(),
                    )?,
                ));
            }
            self.ready_owned(attempt, registration, loaded)
                .await
                .map(native::ReboundReadyOutcome::Ready)
        })
    }
    fn rebound_preload_owned<'a>(
        &'a self,
        attempt: &'a codex_alab_core::native_preload::Body,
    ) -> native::PreloadFuture<'a> {
        self.peer.preload_owned(attempt, None)
    }
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
    fn preload_owned<'a>(
        &'a self,
        attempt: &'a codex_alab_core::native_preload::Body,
        borrow: Option<&'a native::Borrow>,
    ) -> native::PreloadFuture<'a> {
        self.peer.preload_owned(attempt, borrow)
    }
    fn preload_owned_use<'a>(
        &'a self,
        attempt: &'a codex_alab_core::native_preload::Body,
        _: &'a codex_alab_core::native_source_use::UseReceipt,
    ) -> native::PreloadFuture<'a> {
        self.peer.preload_owned(attempt, None)
    }
    fn resample_owned_use<'a>(
        &'a self,
        attempt: &'a codex_alab_core::native_preload::Body,
        previous: &'a codex_alab_core::native_preload::Predecessor,
        receipt: &'a codex_alab_core::native_source_use::UseReceipt,
    ) -> native::ResampleFuture<'a> {
        Box::pin(async move {
            assert_eq!(self.peer.case, "managed_owner_refresh");
            assert_eq!(receipt.member(), self.id);
            assert_eq!(previous.registration, "2".repeat(32));
            assert_eq!(previous.revision, self.peer.source.revision);
            assert!(self.peer.owner_refreshed.load(Ordering::Acquire));
            Ok(codex_alab_core::native_preload::Resample::Preload(
                self.peer.preload_owned(attempt, None).await?,
            ))
        })
    }
    fn ready_owned<'a>(
        &'a self,
        attempt: &'a codex_alab_core::native_preload::Body,
        registration: &'a str,
        loaded: &'a native::Loaded,
    ) -> native::OwnedReadyFuture<'a> {
        self.peer.ready_owned(attempt, registration, loaded)
    }
    fn ready_owned_use<'a>(
        &'a self,
        attempt: &'a codex_alab_core::native_preload::Body,
        registration: &'a str,
        loaded: &'a native::Loaded,
        _: &'a codex_alab_core::native_source_use::UseReceipt,
    ) -> native::OwnedReadyFuture<'a> {
        self.peer.ready_owned(attempt, registration, loaded)
    }
    fn ready<'a>(
        &'a self,
        operation: &'a str,
        registration: Option<&'a str>,
        loaded: &'a native::Loaded,
    ) -> native::ReadyFuture<'a> {
        <Peer as native::Member>::ready(self.peer.as_ref(), operation, registration, loaded)
    }
    fn open_derived<'a>(
        &'a self,
        operation: &'a str,
        parent: &'a codex_alab_core::native_source_use::UseReceipt,
    ) -> native::MemberFuture<'a> {
        Box::pin(async move {
            assert_eq!(parent.member(), self.id());
            assert_eq!(parent.private_capability().len(), 64);
            <Host as native::Host>::open_member(&Host(self.peer.clone()), operation).await
        })
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
            self.peer.begins.fetch_add(1, Ordering::Relaxed);
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
    fn ready_owned<'a>(
        &'a self,
        attempt: &'a codex_alab_core::native_preload::Body,
        registration: &'a str,
        loaded: &'a native::Loaded,
    ) -> native::OwnedReadyFuture<'a> {
        Box::pin(async move {
            let receipt = self
                .ready(&attempt.operation, Some(registration), loaded)
                .await?;
            codex_alab_core::native_preload::Ready::from_native(
                attempt.clone(),
                receipt.registration,
            )
        })
    }
    fn ready_owned_use<'a>(
        &'a self,
        attempt: &'a codex_alab_core::native_preload::Body,
        registration: &'a str,
        loaded: &'a native::Loaded,
        receipt: &'a codex_alab_core::native_source_use::UseReceipt,
    ) -> native::OwnedReadyFuture<'a> {
        assert_eq!(receipt.member(), self.id());
        self.ready_owned(attempt, registration, loaded)
    }

    fn id(&self) -> &str {
        "11111111111111111111111111111111"
    }
    fn live(&self) -> bool {
        self.live.load(Ordering::Acquire)
    }
    fn preload_owned<'a>(
        &'a self,
        attempt: &'a codex_alab_core::native_preload::Body,
        _: Option<&'a native::Borrow>,
    ) -> native::PreloadFuture<'a> {
        Box::pin(async move {
            let operation = attempt.operation.as_str();
            assert!(!self.case.starts_with("api"));
            let preload = self.preloaded.fetch_add(1, Ordering::Relaxed);
            if self.case == "direct_cancel" || self.case == "direct_policy" {
                let record = self
                    .direct_record
                    .lock()
                    .unwrap()
                    .clone()
                    .expect("actual reader admission must precede native preload");
                assert_eq!(
                    record.role().unwrap(),
                    codex_alab_core::manager_catalog::Role::Reader
                );
                if self.case == "direct_cancel" {
                    self.direct_started.notify_one();
                    self.direct_release.notified().await;
                }
            }
            let _free = codex_alab_core::auth_lock::AuthLock::try_take(&self.source.path)
                .unwrap()
                .expect("native preload must precede local file lock");
            if self.case == "tampered" {
                std::fs::write(&self.source.path, b"{}").unwrap();
            }
            if self.case == "mode" || self.case == "direct_policy" {
                std::fs::remove_file(&self.source.path).unwrap();
            }
            let mut source = self.source.clone();
            source.operation = operation.into();
            if (self.case == "managed_refresh" || self.case.starts_with("rebound_changed"))
                && preload > 0
            {
                let mut next = document();
                let tokens = next.tokens.as_mut().unwrap();
                tokens.access_token = if preload > 1 && self.case.starts_with("rebound_changed") {
                    "fixture-next-access"
                } else {
                    "fixture-refreshed-access"
                }
                .into();
                tokens.refresh_token = if preload > 1 && self.case.starts_with("rebound_changed") {
                    "fixture-next-refresh"
                } else {
                    "fixture-refreshed-refresh"
                }
                .into();
                let bytes = serde_json::to_vec(&next).unwrap();
                std::fs::write(&source.path, &bytes).unwrap();
                source.revision.generation += if self.case.starts_with("rebound_changed") {
                    preload as u64
                } else {
                    1
                };
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
                sequence: attempt.sequence.clone(),
                source,
                registration: if ((self.case == "managed_refresh"
                    || self.case.starts_with("rebound_changed"))
                    && preload > 0)
                    || owner_ready
                {
                    if self.case.starts_with("rebound_changed") {
                        format!("{:032x}", preload + 2)
                    } else {
                        "3".repeat(32)
                    }
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
                    ) || self.case.starts_with("rebound_changed")
                    {
                        assert_eq!(source.operation, operation);
                    }
                    if (matches!(
                        self.case.as_str(),
                        "managed_refresh" | "managed_owner_refresh"
                    ) || self.case.starts_with("rebound_changed"))
                        && source.revision.generation >= 43
                    {
                        assert_eq!(source.operation, operation);
                        if self.case.starts_with("rebound_changed") {
                            assert_eq!(
                                registration,
                                Some(format!("{:032x}", source.revision.generation - 40).as_str())
                            );
                        } else {
                            assert_eq!(registration, Some("33333333333333333333333333333333"));
                        }
                        assert_eq!(
                            source.revision.digest,
                            format!("{:x}", Sha256::digest(std::fs::read(&source.path).unwrap()))
                        );
                    } else {
                        assert_eq!(source.revision, self.source.revision);
                    }
                    self.ready.lock().unwrap().push(Some(generations.clone()));
                    if (self.case.starts_with("work_rebind_")
                        || self.case.starts_with("rebound_changed_gate_"))
                        && self.ready.lock().unwrap().len() == 2
                    {
                        self.recovery_started.notify_one();
                        self.recovery_release.notified().await;
                    }
                    if self.case == "ready_lost" && self.ready.lock().unwrap().len() == 1 {
                        return Err(std::io::Error::other("fixture lost Ready response"));
                    }
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
                    if self.case == "api_cache_ack_swap"
                        && self.owner_refreshed.swap(false, Ordering::AcqRel)
                    {
                        self.recovery_started.notify_one();
                        self.recovery_release.notified().await;
                    }
                }
                native::Loaded::ReaderSource { source } => {
                    assert!(matches!(
                        self.case.as_str(),
                        "managed_direct" | "direct_cancel" | "managed_reader_parent"
                    ));
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
        forced_login_method: (case == "direct_policy").then_some(ForcedLoginMethod::Api),
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
        work: Default::default(),
        recovery_started: Default::default(),
        recovery_release: Default::default(),
        opened: AtomicUsize::new(0),
        preloaded: AtomicUsize::new(0),
        begins: AtomicUsize::new(0),
        ready: Mutex::default(),
        unavailable: AtomicUsize::new(0),
        retired: AtomicUsize::new(0),
        changed: Default::default(),
        owner_refreshed: AtomicBool::new(false),
        owner_requests: Mutex::default(),
        direct_started: Default::default(),
        direct_release: Default::default(),
        direct_record: Mutex::default(),
    });
    assert!(READER_OBSERVER.set(Arc::downgrade(&peer)).is_ok());
    native::install(Arc::new(Host(Arc::clone(&peer))));
    let config = config(home.clone(), &case);

    if case == "api_cache_ack_swap" {
        let manager = Arc::new(AuthManager::new_from_auth_config(config.clone(), true, None).await);
        let other = AuthManager::new_from_auth_config(config.clone(), true, None).await;
        let original = manager.alab_source_member.read().unwrap().clone().unwrap();
        let replacement = other.alab_source_member.read().unwrap().clone().unwrap();
        let before = *manager.auth_change_state_tx.borrow();
        manager
            .alab_begin_initial_source(original.clone(), Some(native::NoNativeReader::ApiKey))
            .unwrap();
        peer.owner_refreshed.store(true, Ordering::Release);
        let waiter = {
            let manager = manager.clone();
            tokio::spawn(async move { manager.alab_resume_initial_source().await })
        };
        peer.recovery_started.notified().await;
        *manager.alab_source_member.write().unwrap() = Some(replacement.clone());
        peer.recovery_release.notify_one();
        assert!(
            waiter.await.unwrap().is_err(),
            "an original ACK cannot publish a different member"
        );
        assert!(manager.auth_cached().is_none());
        assert!(
            !manager
                .inner
                .read()
                .unwrap()
                .auth
                .as_ref()
                .unwrap()
                .alab_credential_epoch()
                .same_native_member(&replacement)
        );
        *manager.alab_source_member.write().unwrap() = Some(original.clone());
        manager.alab_resume_initial_source().await.unwrap();
        assert!(
            manager
                .auth_cached()
                .unwrap()
                .alab_credential_epoch()
                .same_native_member(&original)
        );
        assert_eq!(
            *manager.auth_change_state_tx.borrow(),
            before,
            "provenance completion cannot invent auth-change counters"
        );
        return;
    } else if matches!(case.as_str(), "cache_snapshot" | "api_cache_snapshot") {
        let manager = AuthManager::new_from_auth_config(config.clone(), true, None).await;
        let other = AuthManager::new_from_auth_config(config.clone(), true, None).await;
        let original = manager.alab_source_member.read().unwrap().clone().unwrap();
        let replacement = other.alab_source_member.read().unwrap().clone().unwrap();
        assert!(original.ready() && replacement.ready());
        assert!(!original.same(&replacement));
        let snapshot = manager.alab_member_cache_snapshot().unwrap();
        let epoch = manager.alab_source_epoch().unwrap();
        assert!(
            snapshot
                .auth
                .alab_credential_epoch()
                .epoch()
                .unwrap()
                .same(&epoch)
        );
        assert!(
            manager.inner.try_write().is_err(),
            "paired factory capture retains the real cache reader"
        );
        let factory = snapshot
            .auth
            .alab_bind_http_client_factory(manager.http_client_factory())
            .unwrap();
        *manager.alab_source_member.write().unwrap() = None;
        assert!(
            manager.auth_cached().is_none(),
            "missing native membership cannot become legacy access"
        );
        assert!(manager.auth_with_http_client_factory().await.is_none());
        assert!(manager.alab_auth_with_http_client_factory().await.unwrap().is_none());
        *manager.alab_source_member.write().unwrap() = Some(replacement);
        assert!(
            manager.auth_cached().is_none(),
            "another ready constructor is not this cache's provenance"
        );
        assert!(manager.auth_with_http_client_factory().await.is_none());
        assert!(manager.alab_auth_with_http_client_factory().await.unwrap().is_none());
        *manager.alab_source_member.write().unwrap() = Some(original);
        assert!(
            manager
                .auth_cached()
                .unwrap()
                .alab_credential_epoch()
                .epoch()
                .unwrap()
                .same(&epoch)
        );
        assert!(manager.auth_with_http_client_factory().await.is_some());
        assert!(manager.alab_auth_with_http_client_factory().await.unwrap().is_some());
        drop(factory);
        drop(snapshot);
        assert!(
            manager.inner.try_write().is_ok(),
            "capture releases its own reader synchronously"
        );
        return;
    } else if case.starts_with("rebound_changed") {
        let manager = Arc::new(AuthManager::new_from_auth_config(config.clone(), true, None).await);
        let old = manager.auth_cached().unwrap();
        let before = *manager.auth_change_state_tx.borrow();
        let member = manager.alab_source_member.read().unwrap().clone().unwrap();
        let activity = manager.alab_account_resource().unwrap();
        if case == "rebound_changed_prior_use" {
            activity
                .clone()
                .scope(async {
                    let factory = manager.http_client_factory();
                    let permit = factory
                        .network_policy()
                        .acquire(&"http://127.0.0.1/".parse().unwrap())
                        .unwrap();
                    assert!(permit.run(async { old.get_token() }).await.unwrap().is_ok());
                })
                .await;
        }
        let begins = peer.begins.load(Ordering::Acquire);
        if case == "rebound_changed_prior_use" {
            assert!(begins > 0);
        } else {
            assert_eq!(begins, 0);
        }
        let mut changed = document();
        let tokens = changed.tokens.as_mut().unwrap();
        tokens.access_token = "fixture-refreshed-access".into();
        tokens.refresh_token = "fixture-refreshed-refresh".into();
        codex_alab_core::home::save_auth_bytes(&selected, &serde_json::to_vec(&changed).unwrap())
            .unwrap();
        peer.live.store(false, Ordering::Release);
        assert!(manager.auth_cached().is_none());
        let recovered = if case.starts_with("rebound_changed_gate_") {
            let waiter = {
                let manager = manager.clone();
                tokio::spawn(async move { manager.auth().await })
            };
            peer.recovery_started.notified().await;
            assert!(
                manager.auth_cached().is_none(),
                "new cache cannot publish localReady before ACK"
            );
            assert!(manager.auth_change_state_tx.borrow().generation > before.generation);
            match case.as_str() {
                "rebound_changed_gate_cancel" => {
                    waiter.abort();
                    let _ = waiter.await;
                    peer.recovery_release.notify_one();
                    manager.auth().await
                }
                _ => {
                    match case.as_str() {
                        "rebound_changed_gate_seal" => member.seal(),
                        "rebound_changed_gate_eof" => peer.live.store(false, Ordering::Release),
                        "rebound_changed_gate_policy" => {
                            manager.restrict_auth_modes(vec![AuthMode::ApiKey]).unwrap()
                        }
                        "rebound_changed_gate_workspace" => manager
                            .set_forced_chatgpt_workspace_id(Some(vec![
                                "foreign-workspace".into(),
                            ])),
                        "rebound_changed_gate_member" => {
                            *manager.alab_source_member.write().unwrap() = None;
                        }
                        "rebound_changed_gate_physical" => {
                            let mut next = document();
                            next.tokens.as_mut().unwrap().access_token = "C4-unloaded".into();
                            codex_alab_core::home::save_auth_bytes(
                                &selected,
                                &serde_json::to_vec(&next).unwrap(),
                            )
                            .unwrap();
                        }
                        _ => unreachable!(),
                    }
                    peer.recovery_release.notify_one();
                    let result = waiter.await.unwrap();
                    if matches!(
                        case.as_str(),
                        "rebound_changed_gate_physical" | "rebound_changed_gate_eof"
                    ) {
                        result
                    } else {
                        assert!(result.is_none());
                        assert!(!member.ready());
                        assert!(manager.auth_cached().is_none());
                        assert_eq!(peer.preloaded.load(Ordering::Acquire), 2);
                        return;
                    }
                }
            }
        } else if case == "rebound_changed_prior_use" {
            activity.scope(manager.auth()).await
        } else {
            manager.auth().await
        }
        .expect("actual new PRELOAD must precede new Ready");
        assert_eq!(
            recovered.get_current_token_data().unwrap().access_token,
            if case == "rebound_changed_gate_physical" {
                "fixture-next-access"
            } else {
                "fixture-refreshed-access"
            }
        );
        assert!(member.ready());
        if case == "rebound_changed_gate_eof" {
            assert_eq!(peer.work.load(Ordering::Acquire), 2);
        }
        let after = *manager.auth_change_state_tx.borrow();
        assert!(after.generation > before.generation);
        assert_eq!(after.owner_generation, before.owner_generation);
        assert_eq!(
            peer.preloaded.load(Ordering::Acquire),
            if case == "rebound_changed_gate_physical" {
                3
            } else {
                2
            }
        );
        assert_eq!(
            peer.begins.load(Ordering::Acquire),
            begins,
            "constructor rebound cannot mint HTTP use"
        );
        let reports = peer.ready.lock().unwrap();
        assert_eq!(
            reports.len(),
            if matches!(
                case.as_str(),
                "rebound_changed_gate_cancel" | "rebound_changed_gate_physical"
            ) {
                3
            } else {
                2
            }
        );
        if case == "rebound_changed_gate_cancel" {
            assert_eq!(
                reports[1], reports[2],
                "lost observer replays original actual report"
            );
        }
        assert_eq!(
            *reports.last().unwrap(),
            Some(codex_alab_core::assignment::Generations {
                auth: after.generation,
                owner: after.owner_generation,
                source: after.source_generation,
                login: after.login_generation
            })
        );
        return;
    } else if case.starts_with("work_rebind_") {
        let manager = Arc::new(AuthManager::new_from_auth_config(config.clone(), true, None).await);
        let original = manager.auth_cached().expect("actual initial Ready");
        let member = manager.alab_source_member.read().unwrap().clone().unwrap();
        let before = *manager.auth_change_state_tx.borrow();
        peer.live.store(false, Ordering::Release);
        assert!(manager.auth_cached().is_none());
        let waiter = {
            let manager = manager.clone();
            tokio::spawn(async move { manager.auth().await })
        };
        peer.recovery_started.notified().await;
        // Native ACK has not returned: physical reconnect cannot expose cache.
        assert!(!member.ready());
        assert!(manager.auth_cached().is_none());
        match case.as_str() {
            "work_rebind_seal" => member.seal(),
            "work_rebind_policy" => manager.restrict_auth_modes(vec![AuthMode::ApiKey]).unwrap(),
            "work_rebind_workspace" => {
                manager.set_forced_chatgpt_workspace_id(Some(vec!["foreign-workspace".into()]))
            }
            "work_rebind_cache" => {
                manager.set_cached_auth(None);
            }
            "work_rebind_document" => {
                // Actual snapshots share mutable document storage. Mode-only
                // equality and unchanged watch counters cannot authorize it.
                let cache = manager.inner.read().unwrap();
                let Some(CodexAuth::Chatgpt(auth)) = cache.auth.as_ref() else {
                    panic!("managed source")
                };
                auth.state
                    .auth_dot_json
                    .lock()
                    .unwrap()
                    .as_mut()
                    .unwrap()
                    .tokens
                    .as_mut()
                    .unwrap()
                    .access_token = "changed-cached-document".into();
                assert_eq!(*manager.auth_change_state_tx.borrow(), before);
            }
            "work_rebind_source" => {
                let mut changed = document();
                changed.tokens.as_mut().unwrap().access_token = "changed-physical-owner".into();
                codex_alab_core::home::save_auth_bytes(
                    &selected,
                    &serde_json::to_vec(&changed).unwrap(),
                )
                .unwrap();
            }
            "work_rebind_member" => {
                *manager.alab_source_member.write().unwrap() = None;
            }
            "work_rebind_detach" => {
                manager.alab_native_detach().unwrap();
            }
            "work_rebind_ok" => {}
            _ => unreachable!(),
        }
        peer.recovery_release.notify_one();
        let result = waiter.await.unwrap();
        if case == "work_rebind_ok" {
            let recovered = result.expect("unchanged exact Ready can rebind");
            assert!(member.ready());
            assert!(
                original
                    .alab_credential_epoch()
                    .epoch()
                    .unwrap()
                    .same(recovered.alab_credential_epoch().epoch().unwrap())
            );
            assert_eq!(*manager.auth_change_state_tx.borrow(), before);
        } else {
            assert!(result.is_none(), "changed authority cannot publish Ready");
            assert!(!member.ready());
        }
        assert_eq!(
            peer.preloaded.load(Ordering::Acquire),
            1,
            "rebind cannot invent a new PRELOAD"
        );
        assert_eq!(peer.work.load(Ordering::Acquire), 1);
        let reports = peer.ready.lock().unwrap();
        assert_eq!(reports.len(), 2);
        assert_eq!(
            reports[0], reports[1],
            "replay retains exact actual four revisions"
        );
        return;
    } else if case == "managed_reader_parent" {
        let manager = AuthManager::new_from_auth_config(config.clone(), true, None).await;
        let domain = manager.alab_assignment_domain();
        let root = domain.admit_work(None).unwrap();
        root.scope(async {
            let reader = config.load_auth(true).await.unwrap().unwrap();
            assert!(reader.alab_credential_epoch().epoch().is_none());
            let factory = reader
                .alab_bind_http_client_factory(manager.http_client_factory())
                .unwrap();
            let server = wiremock::MockServer::start().await;
            wiremock::Mock::given(wiremock::matchers::method("GET"))
                .respond_with(
                    wiremock::ResponseTemplate::new(200).set_body_string("reader-and-manager"),
                )
                .expect(1)
                .mount(&server)
                .await;
            let url = server.uri().parse().unwrap();
            let permit = factory.network_policy().acquire(&url).unwrap();
            let token = permit
                .run(async { reader.get_token() })
                .await
                .unwrap()
                .unwrap();
            let response = factory
                .build_client(&server.uri(), codex_http_client::ClientRouteClass::Other)
                .unwrap()
                .get(server.uri())
                .bearer_auth(token)
                .send()
                .await
                .unwrap();
            assert_eq!(response.text().await.unwrap(), "reader-and-manager");
            drop(permit);
            assert_eq!(peer.opened.load(Ordering::Acquire), 2);
            assert!(peer.ready.lock().unwrap().first().unwrap().is_some());
            assert!(peer.ready.lock().unwrap().last().unwrap().is_none());
        })
        .await;
    } else if case == "direct_cancel" {
        let config = config.clone();
        let mut waiter = tokio::spawn(async move { config.load_auth(true).await });
        tokio::select! {
            _ = peer.direct_started.notified() => {},
            result = &mut waiter => panic!("reader ended before owned preload started: {result:?}"),
            _ = tokio::time::sleep(std::time::Duration::from_secs(10)) => {
                panic!("owned reader preload did not start; opened={} preloaded={} admitted={}",
                    peer.opened.load(Ordering::Acquire), peer.preloaded.load(Ordering::Acquire),
                    peer.direct_record.lock().unwrap().is_some());
            }
        }
        let record = peer.direct_record.lock().unwrap().clone().unwrap();
        waiter.abort();
        let _ = waiter.await;
        assert_eq!(
            record.phase().unwrap(),
            codex_alab_core::manager_catalog::Phase::Loading
        );
        assert!(
            record.domain().is_some(),
            "owned loader retains admitted Domain"
        );
        assert_eq!(peer.retired.load(Ordering::Acquire), 0);
        peer.direct_release.notify_one();
        peer.changed.notified().await;
        record.reader_retired().await.unwrap();
        assert_eq!(
            peer.opened.load(Ordering::Acquire),
            1,
            "cancelled waiter cannot mint another constructor"
        );
        assert_eq!(
            *peer.ready.lock().unwrap(),
            vec![None],
            "actual reader load completes without manager counters"
        );
    } else if case == "direct_policy" {
        assert!(config.load_auth(true).await.is_err());
        let record = peer.direct_record.lock().unwrap().clone().unwrap();
        record.reader_retired().await.unwrap();
        assert_eq!(peer.unavailable.load(Ordering::Acquire), 1);
        assert!(peer.ready.lock().unwrap().is_empty());
    } else if case == "api_direct" || case == "managed_direct" {
        let snapshot = config.load_auth(true).await.unwrap().unwrap();
        if case == "api_direct" {
            assert_eq!(snapshot.api_key(), Some("fixture-api-key"));
        } else {
            assert_eq!(snapshot.get_account_id().as_deref(), Some("account-b"));
        }
        assert_eq!(*peer.ready.lock().unwrap(), vec![None]);
        let reader_member = match &snapshot {
            CodexAuth::ApiKey(api) => api.alab_source.clone().unwrap(),
            CodexAuth::Chatgpt(chatgpt) => chatgpt.storage.alab_source_lifetime().unwrap(),
            _ => panic!("actual direct source kind"),
        };
        let reader_domain = reader_member.domain().unwrap();
        let reader_record = reader_domain.catalog().unwrap().clone();
        assert_eq!(
            reader_record.role().unwrap(),
            codex_alab_core::manager_catalog::Role::Reader
        );
        assert!(
            reader_record.backend().unwrap().is_none(),
            "direct reader never fabricates manager counters"
        );
        assert!(reader_record.reader_snapshot().unwrap().is_some());
        assert!(
            snapshot.alab_credential_epoch().epoch().is_none(),
            "reader origin has no manager epoch/stamps"
        );
        assert!(snapshot.alab_credential_epoch().has_native_member());
        assert!(!snapshot.alab_credential_epoch().legacy_snapshot_export());
        assert!(
            snapshot
                .alab_credential_epoch()
                .network_policy()
                .unwrap()
                .is_some()
        );
        if case == "api_direct" {
            let receiver = AuthManager::new_from_auth_config(config.clone(), true, None).await;
            let copied = {
                let mut cached = receiver.inner.write().unwrap();
                receiver.set_cached_auth_locked(&mut cached, Some(snapshot.clone()));
                cached.auth.as_ref().unwrap().clone()
            };
            assert!(copied.alab_credential_epoch().epoch().is_none());
            assert!(copied.alab_credential_epoch().has_native_member());
            assert!(
                copied
                    .alab_bind_http_client_factory(receiver.http_client_factory())
                    .is_err(),
                "copying a reader credential into a manager cannot relabel its origin"
            );
            drop(copied);
            drop(receiver);
        }
        if case == "api_direct" {
            assert_eq!(
                reader_record
                    .reader_snapshot()
                    .unwrap()
                    .unwrap()
                    .no_native_reader(),
                Some(NoNativeReader::ApiKey)
            );
        }
        drop(reader_domain);
        if case == "managed_direct" {
            let CodexAuth::Chatgpt(chatgpt) = &snapshot else {
                panic!("actual managed direct reader")
            };
            let server = wiremock::MockServer::start().await;
            wiremock::Mock::given(wiremock::matchers::method("GET"))
                .respond_with(wiremock::ResponseTemplate::new(200).set_body_string("reader-body"))
                .expect(1)
                .mount(&server)
                .await;
            let factory = snapshot
                .alab_bind_http_client_factory(
                    config.auth_route_config.http_client_factory().clone(),
                )
                .expect("pair the actual reader with its original caller transport");
            let retained_client = factory
                .build_client(&server.uri(), codex_http_client::ClientRouteClass::Other)
                .unwrap();
            assert_eq!(
                retained_client
                    .get(server.uri())
                    .send()
                    .await
                    .unwrap()
                    .text()
                    .await
                    .unwrap(),
                "reader-body"
            );
            let member = chatgpt.storage.alab_source_lifetime().unwrap();
            let original = std::fs::read(&peer.source.path).unwrap();
            std::fs::write(&peer.source.path, b"{}").unwrap();
            assert!(
                snapshot.get_token().is_err(),
                "replacement cannot expose the old snapshot token"
            );
            std::fs::write(&peer.source.path, original).unwrap();
            assert!(snapshot.get_token().is_ok());
            let root = member.domain().unwrap().admit_work(None).unwrap();
            let next = root
                .scope(member.preload(&"8".repeat(32), None))
                .await
                .unwrap();
            assert!(
                snapshot.get_token().is_err(),
                "preload invalidates previous loaded proof"
            );
            member.reader_ready(&next.source).await.unwrap();
            assert!(member.ready());
            assert!(
                snapshot.get_token().is_err(),
                "new Ready must not revive an old direct snapshot"
            );
            assert!(!snapshot.alab_credential_epoch().native_ready());
            assert!(
                snapshot
                    .alab_credential_epoch()
                    .network_policy()
                    .unwrap()
                    .unwrap()
                    .acquire(&"https://example.com/".parse().unwrap())
                    .is_err(),
                "changed actual ReaderLoaded cannot authorize the old origin"
            );
            assert!(
                retained_client.get(server.uri()).send().await.is_err(),
                "a retained paired HTTP client cannot use the reader's replacement Ready"
            );
            server.verify().await;
            drop(retained_client);
            drop(factory);
        }
        peer.live.store(false, Ordering::Release);
        assert!(snapshot.api_key().is_none());
        assert!(snapshot.get_token().is_err());
        drop(snapshot);
        drop(reader_member);
        peer.changed.notified().await;
        reader_record.reader_retired().await.unwrap();
        assert_eq!(
            reader_record.phase().unwrap(),
            codex_alab_core::manager_catalog::Phase::Retired
        );
    } else if case == "network_body" || case == "network_text" {
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
        let body = if case == "network_text" {
            assert_eq!(response.text().await.unwrap(), "held-body");
            None
        } else {
            Some(response.bytes_stream())
        };
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
        let retiring = if case == "network_body" {
            match retiring.freeze() {
                Err(codex_alab_core::assignment::FreezeError::Pending(retiring)) => retiring,
                _ => panic!("actual HTTP response body must hold source activity"),
            }
        } else {
            retiring
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
            config.clone(),
            true,
            if case == "mode" {
                Some(vec![AuthMode::ApiKey])
            } else {
                None
            },
        )
        .await;
        if case == "ready_lost" {
            assert!(manager.auth_cached().is_none());
            assert!(
                manager.inner.read().unwrap().auth.is_some(),
                "actual candidate remains private through unknown ACK"
            );
            let actual = *manager.auth_change_state_receiver().borrow();
            assert_eq!(peer.ready.lock().unwrap().len(), 1);
            assert!(
                manager.alab_native_account_tokens().await.unwrap().is_err(),
                "pending storage accessor has no Bound extraction authority"
            );
            assert!(manager.alab_native_path().is_err());
            let original = std::fs::read(&peer.source.path).unwrap();
            std::fs::write(&peer.source.path, b"{}").unwrap();
            manager.alab_resume_initial_source().await.unwrap();
            assert_eq!(
                *manager.auth_change_state_receiver().borrow(),
                actual,
                "replay cannot invent or restore counters"
            );
            assert_eq!(
                peer.preloaded.load(Ordering::Relaxed),
                1,
                "same Ready does not create a read/scope"
            );
            let reports = peer.ready.lock().unwrap().clone();
            assert_eq!(reports.len(), 2);
            assert_eq!(
                reports[0], reports[1],
                "original actual report is immutable"
            );
            assert!(manager.auth_cached().is_some());
            std::fs::write(&peer.source.path, original).unwrap();
            manager.alab_native_detach().unwrap();
            manager.alab_resume_initial_source().await.unwrap();
            assert!(
                manager.auth_cached().is_none(),
                "terminal Detached cannot resurrect the pending source"
            );
            assert_eq!(peer.ready.lock().unwrap().len(), 2);
            drop(manager);
            while peer.retired.load(Ordering::Relaxed) == 0 {
                peer.changed.notified().await;
            }
            return;
        }
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
                // Replacing the field cannot transfer the cached credential's
                // immutable original Lifetime provenance, even after Ready.
                assert!(manager.auth_cached().is_none());
                assert!(manager.alab_native_source_fence().await.is_err());
                // A genuine constructor owns its own cache and Ready. Its scope
                // still cannot continue the old occurrence (including reused IDs).
                let successor = AuthManager::new_from_auth_config(config, true, None).await;
                assert!(successor.auth_cached().is_some());
                let current = successor.alab_native_source_fence().await.unwrap();
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
                let prepared_token = snapshot.get_token().unwrap();
                assert!(manager.alab_validate_credential_export(&snapshot, &prepared_token).is_ok());
                let factory = manager.http_client_factory();
                let permit = factory
                    .network_policy()
                    .acquire(&"https://example.com/".parse().unwrap())
                    .unwrap();
                manager.restrict_auth_modes(vec![AuthMode::ApiKey]).unwrap();
                manager.reload().await;
                assert!(manager.auth_cached().is_none());
                assert!(snapshot.get_token().is_err());
                assert!(manager.alab_validate_credential_export(&snapshot, &prepared_token).is_err());
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
                let prepared_token = snapshot.get_token().unwrap();
                assert!(manager.alab_validate_credential_export(&snapshot, &prepared_token).is_ok());
                peer.live.store(false, Ordering::Release);
                assert!(manager.auth_cached().is_none());
                assert!(snapshot.get_token().is_err());
                assert!(manager.alab_validate_credential_export(&snapshot, &prepared_token).is_err());
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
    if case == "mode" || case == "direct_policy" {
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
        if matches!(case.as_str(), "member_replacement" | "member_reincarnation") {
            // Original constructor, manual transplant, genuine successor.
            3
        } else if matches!(
            case.as_str(),
            "network_cross" | "api_direct" | "managed_reader_parent"
        ) {
            // api_direct explicitly constructs a receiver manager to prove a
            // copied reader credential cannot be relabeled as manager-owned.
            2
        } else {
            1
        }
    );
}
