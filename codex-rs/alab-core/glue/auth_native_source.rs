// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Per-manager native source selection. Native files are read-only to Codex.

use super::*;
use crate::auth::storage::AuthStorageBackend;
use crate::auth::storage::auth_storage_commit_guard;
use codex_alab_core::managed_auth;
use codex_alab_core::native_source;
use sha2::Digest;
use sha2::Sha256;

#[path = "auth_target_policy.rs"]
pub(super) mod target_policy;
/// Keep native source I/O opaque to downstream request futures. This retains
/// in-task cancellation and ownership while bounding exported async type depth.
pub(super) fn boxed_native<'a, T: Send + 'a>(
    future: impl std::future::Future<Output = T> + Send + 'a,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>> {
    Box::pin(future)
}

#[derive(Default)]
pub(super) enum State {
    #[default]
    Startup,
    Bound(Arc<Bound>),
    PendingInitial(Arc<PendingInitial>),
    Detached,
}
/// The credential was cloned under coherent Source/cache/member guards. Keep
/// the original cache reader through account-bound factory construction.
pub(super) struct NativeCacheSnapshot<'a> {
    pub(super) auth: CodexAuth,
    _cache: std::sync::RwLockReadGuard<'a, CachedAuth>,
}

pub(super) struct PendingInitial {
    pub(super) member: codex_alab_core::native_source_lifetime::Lifetime,
    pub(super) selected: Option<native_source::NoNativeReader>,
    bound: Option<Arc<Bound>>,
    reported: Option<AuthChangeState>,
    recovery: Option<Arc<ReboundOrigin>>,
}
/// Inert original custody for a constructor-owned rebound. No credential read
/// or new source is granted until the actual native PRELOAD returns.
pub(super) struct ReboundOrigin {
    bound: Arc<Bound>,
    source: native_source::Source,
    expected: AuthChangeState,
    epoch: codex_alab_core::manager_source_epoch::Epoch,
    tokens: crate::token_data::TokenData,
}

pub(super) struct Bound {
    source: native_source::Source,
    revision: Mutex<managed_auth::Revision>,
    member: Option<codex_alab_core::native_source_lifetime::Lifetime>,
    reader: std::sync::OnceLock<Box<codex_alab_core::native_source_lifetime::ReaderLoaded>>,
}

impl std::fmt::Debug for Bound {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("NativeCredentialStorage(<private>)")
    }
}

/// Direct readers use the same verified storage authority without inventing a manager.
pub(super) async fn direct_reader(
    config: &AuthConfig,
    allowed_modes: Option<&[AuthMode]>,
    source: native_source::Source,
    member: codex_alab_core::native_source_lifetime::Lifetime,
) -> std::io::Result<CodexAuth> {
    if allowed_modes.is_some_and(|modes| !modes.contains(&AuthMode::Chatgpt))
        || !config.is_login_method_allowed(ForcedLoginMethod::Chatgpt)
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "native account is excluded by authentication policy",
        ));
    }
    let bound = Arc::new(Bound {
        reader: std::sync::OnceLock::new(),
        revision: Mutex::new(source.revision.clone()),
        source: source.clone(),
        member: Some(member.clone()),
    });
    let reader = Arc::clone(&bound);
    let document = tokio::task::spawn_blocking(move || reader.read())
        .await
        .map_err(|_| managed_auth::unavailable())??;
    let agent_api = agent_identity_authapi_base_url(config.chatgpt_base_url.as_deref()).ok();
    let route = config
        .auth_route_config
        .alab_with_native_source(member.network_scope().map_err(|_| ()));
    let mut auth = CodexAuth::from_auth_dot_json(
        &config.codex_home,
        document,
        AuthCredentialsStoreMode::File,
        config.chatgpt_base_url.as_deref(),
        config.keyring_backend_kind,
        agent_api.as_deref(),
        &route,
        None,
    )
    .await?;
    if !config.allows_auth(&auth) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "native account is excluded by authentication policy",
        ));
    }
    let CodexAuth::Chatgpt(chatgpt) = &mut auth else {
        return Err(managed_auth::unavailable());
    };
    chatgpt.storage = bound.clone();
    member
        .reader_ready(&source)
        .await
        .map_err(|_| managed_auth::unavailable())?;
    bound
        .reader
        .set(Box::new(member.reader_loaded()?))
        .map_err(|_| managed_auth::unavailable())?;
    Ok(auth)
}

pub(super) fn principal(domain: &[u8], value: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(domain);
    hash.update(value.as_bytes());
    format!("{:x}", hash.finalize())
}

fn binding(
    source: &native_source::Source,
    state: AuthChangeState,
) -> codex_alab_core::assignment::Binding {
    codex_alab_core::assignment::Binding {
        account: source.account.clone(),
        user: source.user.clone(),
        digest: source.revision.digest.clone(),
        credential_generation: source.revision.generation,
        assignment_generation: source.assignment_generation,
        generations: codex_alab_core::assignment::Generations {
            auth: state.generation,
            owner: state.owner_generation,
            source: state.source_generation,
            login: state.login_generation,
        },
    }
}

impl Bound {
    fn same_scope(&self, source: &native_source::Source) -> bool {
        self.source.slot == source.slot
            && self.source.path == source.path
            && self.source.account == source.account
            && self.source.user == source.user
            && self.source.assignment_generation == source.assignment_generation
            && self.source.native_intent == source.native_intent
    }
    fn read(&self) -> std::io::Result<AuthDotJson> {
        let revision = self
            .revision
            .lock()
            .map_err(|_| managed_auth::unavailable())?
            .clone();
        self.read_revision(&revision)
    }

    fn read_revision(&self, revision: &managed_auth::Revision) -> std::io::Result<AuthDotJson> {
        self.source.validate()?;
        if let Some(member) = &self.member {
            member.permits_storage(&self.source)?;
            member.permits_revision(revision)?;
        }
        if self
            .source
            .path
            .canonicalize()
            .map_err(|_| managed_auth::unavailable())?
            != self.source.path
        {
            return Err(managed_auth::unavailable());
        }
        if managed_auth::binding_for_path(true, None, Some(&self.source.path))?.as_deref()
            != Some(self.source.slot.as_str())
        {
            return Err(managed_auth::unavailable());
        }
        let bytes = managed_auth::read_document(&self.source.path)?;
        if format!("{:x}", Sha256::digest(bytes.as_slice())) != revision.digest {
            return Err(managed_auth::unavailable());
        }
        let document: AuthDotJson =
            serde_json::from_slice(&bytes).map_err(|_| managed_auth::unavailable())?;
        let tokens = document
            .tokens
            .as_ref()
            .ok_or_else(managed_auth::unavailable)?;
        let account = tokens
            .account_id
            .as_deref()
            .filter(|value| !value.is_empty())
            .ok_or_else(managed_auth::unavailable)?;
        let user = tokens
            .id_token
            .chatgpt_user_id
            .as_deref()
            .filter(|value| !value.is_empty())
            .ok_or_else(managed_auth::unavailable)?;
        if document.resolved_mode() != AuthMode::Chatgpt
            || principal(b"aterm/codex-renewal/account/v1\0", account) != self.source.account
            || principal(b"aterm/codex-renewal/user/v1\0", user) != self.source.user
        {
            return Err(managed_auth::unavailable());
        }
        Ok(document)
    }

    fn acknowledge(&self, revision: &managed_auth::Revision) -> std::io::Result<()> {
        let mut current = self
            .revision
            .lock()
            .map_err(|_| managed_auth::unavailable())?;
        if revision.generation < current.generation
            || (revision.generation == current.generation && revision.digest != current.digest)
            || !managed_auth::valid_hex(&revision.digest, 64)
        {
            return Err(managed_auth::unavailable());
        }
        self.read_revision(revision)?;
        *current = revision.clone();
        Ok(())
    }
}

impl AuthStorageBackend for Bound {
    fn alab_source_scope(&self) -> Option<native_source::Source> {
        Some(self.source.clone())
    }
    fn alab_source_access(&self) -> std::io::Result<()> {
        if let Some(member) = &self.member {
            if !member.ready()
                || self
                    .reader
                    .get()
                    .is_some_and(|loaded| !member.reader_still_loaded(loaded))
            {
                return Err(managed_auth::unavailable());
            }
            if self.reader.get().is_some() {
                // New extraction must still name the exact loaded document. Existing
                // request/body leases may finish across a legitimate grant refresh.
                self.read()?;
            }
        }
        Ok(())
    }
    fn alab_source_lifetime(&self) -> Option<codex_alab_core::native_source_lifetime::Lifetime> {
        self.member.clone()
    }
    fn lock_for_write(
        &self,
    ) -> std::io::Result<(std::sync::MutexGuard<'static, ()>, Option<std::fs::File>)> {
        Ok((auth_storage_commit_guard()?, None))
    }
    fn load(&self) -> std::io::Result<Option<AuthDotJson>> {
        self.read().map(Some)
    }
    fn save(&self, _auth: &AuthDotJson) -> std::io::Result<()> {
        Err(managed_auth::unavailable())
    }
    fn delete(&self) -> std::io::Result<bool> {
        Err(managed_auth::unavailable())
    }
}

/// Holds the real refresh exclusion across native commit and local source publication.
/// It contains no file lock: native IPC must precede local materialization locking.
pub struct NativeSourceFence {
    domain: Arc<codex_alab_core::assignment::Domain>,
    _refresh: tokio::sync::OwnedSemaphorePermit,
    member: Option<codex_alab_core::native_source_lifetime::Lifetime>,
    expected: AuthChangeState,
    source: native_source::Source,
    grant: crate::token_data::TokenData,
}
/// An observation of the exact managed scope used by a sent ACK. This is not
/// a semaphore, startup-adoption permit, or authorization to install a source.
pub struct NativeSourceScope {
    domain: Arc<codex_alab_core::assignment::Domain>,
    source: native_source::Source,
    expected: AuthChangeState,
    grant: crate::token_data::TokenData,
    member: Option<codex_alab_core::native_source_lifetime::MemberIncarnation>,
}

// Keep only the noncredential fields the existing native lineage comparison
// uses. This projection grants nothing without the verified private scope.
fn grant_projection(tokens: &crate::token_data::TokenData) -> crate::token_data::TokenData {
    crate::token_data::TokenData {
        id_token: crate::token_data::IdTokenInfo {
            chatgpt_user_id: tokens.id_token.chatgpt_user_id.clone(),
            ..Default::default()
        },
        access_token: String::new(),
        refresh_token: String::new(),
        account_id: tokens.account_id.clone(),
        login_owner_id: tokens.login_owner_id.clone(),
        oauth_provenance: tokens.oauth_provenance.clone(),
    }
}

impl NativeSourceFence {
    pub fn scope(&self) -> NativeSourceScope {
        NativeSourceScope {
            domain: Arc::clone(&self.domain),
            source: self.source.clone(),
            expected: self.expected,
            grant: self.grant.clone(),
            member: self
                .member
                .as_ref()
                .map(codex_alab_core::native_source_lifetime::Lifetime::incarnation),
        }
    }
    /// Both observations came through private Source and verified documents.
    /// Numeric generation advancement alone never establishes grant lineage.
    pub fn continues_scope(&self, previous: &NativeSourceScope) -> bool {
        self.scope().continues_scope(previous)
    }
    pub fn receipt_stamp(&self) -> codex_alab_core::assignment::ReceiptStamp {
        codex_alab_core::assignment::ReceiptStamp {
            credential_generation: self.source.revision.generation,
            assignment_generation: self.source.assignment_generation,
            generations: binding(&self.source, self.expected).generations,
        }
    }
    pub fn auth_state(&self) -> AuthChangeState {
        self.expected
    }
    pub fn credential_generation(&self) -> u64 {
        self.source.revision.generation
    }
    pub fn assignment_generation(&self) -> u64 {
        self.source.assignment_generation
    }
}
impl NativeSourceScope {
    pub(super) fn binding(&self) -> codex_alab_core::assignment::Binding {
        binding(&self.source, self.expected)
    }
    pub(super) fn continues_scope(&self, previous: &NativeSourceScope) -> bool {
        Arc::ptr_eq(&self.domain, &previous.domain)
            && self
                .member
                .as_ref()
                .is_none_or(codex_alab_core::native_source_lifetime::MemberIncarnation::live)
            && self.member == previous.member
            && self.source.slot == previous.source.slot
            && self.source.path == previous.source.path
            && self.source.account == previous.source.account
            && self.source.user == previous.source.user
            && self.source.assignment_generation == previous.source.assignment_generation
            && self.source.native_intent == previous.source.native_intent
            && self.expected.owner_generation == previous.expected.owner_generation
            && self.expected.source_generation >= previous.expected.source_generation
            && self.expected.login_generation == previous.expected.login_generation
            && self.expected.generation >= previous.expected.generation
            && super::alab_auth_refresh_lock::managed_grant_relation(&previous.grant, &self.grant)
                == super::alab_auth_refresh_lock::ManagedGrantRelation::Same
            && self.source.revision.generation >= previous.source.revision.generation
            && (self.source.revision.generation != previous.source.revision.generation
                || self.source.revision.digest == previous.source.revision.digest)
    }
}

impl AuthManager {
    pub(super) fn alab_begin_initial_source(
        &self,
        member: codex_alab_core::native_source_lifetime::Lifetime,
        selected: Option<native_source::NoNativeReader>,
    ) -> std::io::Result<()> {
        self.alab_real_epoch()?;
        *self
            .alab_native_source
            .write()
            .map_err(|_| managed_auth::unavailable())? =
            State::PendingInitial(Arc::new(PendingInitial {
                member,
                selected,
                bound: None,
                reported: None,
                recovery: None,
            }));
        Ok(())
    }
    pub(super) async fn alab_resume_initial_source(&self) -> std::io::Result<()> {
        self.alab_real_epoch()?;
        if !matches!(
            &*self
                .alab_native_source
                .read()
                .map_err(|_| managed_auth::unavailable())?,
            State::PendingInitial(_)
        ) {
            return Ok(());
        }
        let _refresh = self
            .refresh_lock
            .acquire()
            .await
            .map_err(|_| managed_auth::unavailable())?;
        let pending = match &*self
            .alab_native_source
            .read()
            .map_err(|_| managed_auth::unavailable())?
        {
            State::PendingInitial(pending) => pending.clone(),
            _ => return Ok(()),
        };
        if let Some(kind) = pending.selected {
            if let Err(error) = pending.member.unrelated_ready(kind).await {
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::PermissionDenied
                        | std::io::ErrorKind::Unsupported
                        | std::io::ErrorKind::InvalidInput
                        | std::io::ErrorKind::InvalidData
                ) {
                    let mut source = self
                        .alab_native_source
                        .write()
                        .map_err(|_| managed_auth::unavailable())?;
                    if matches!(&*source, State::PendingInitial(current) if Arc::ptr_eq(current, &pending))
                    {
                        *source = State::Detached;
                    }
                }
                return Err(error);
            }
            let mut source = self
                .alab_native_source
                .write()
                .map_err(|_| managed_auth::unavailable())?;
            if !matches!(&*source, State::PendingInitial(current) if Arc::ptr_eq(current, &pending))
            {
                return Err(managed_auth::unavailable());
            }
            let mut cached = self
                .inner
                .write()
                .map_err(|_| managed_auth::unavailable())?;
            let members = self
                .alab_source_member
                .read()
                .map_err(|_| managed_auth::unavailable())?;
            if !members
                .as_ref()
                .is_some_and(|actual| actual.same(&pending.member))
            {
                return Err(managed_auth::unavailable());
            }
            if let Some(CodexAuth::ApiKey(api)) = &mut cached.auth {
                api.alab_source = Some(pending.member.clone());
            }
            // Initial cache publication preceded member installation. Capture
            // this same epoch's actually acknowledged unrelated occurrence;
            // no credential or auth-change counter is changed here.
            self.alab_bind_cached_epoch_from_member(
                &mut cached.auth,
                Ok(Some(pending.member.clone())),
            );
            *source = State::Startup; // Actual acknowledged unrelated member still pins the source kind.
            return Ok(());
        }
        if pending.recovery.is_some() {
            return self.alab_resume_rebound_source(pending).await;
        }
        if let (Some(bound), Some(expected)) = (&pending.bound, pending.reported) {
            // No source RPC/file load/cache publication may replace this report.
            let actual = {
                let cached = self.inner.read().map_err(|_| managed_auth::unavailable())?;
                if cached.auth.is_none() || *self.auth_change_state_tx.borrow() != expected {
                    return Err(managed_auth::unavailable());
                }
                binding(&bound.source, expected)
            };
            pending
                .member
                .resume_source_ready(&bound.source, &actual.generations)
                .await?;
            let mut source = self
                .alab_native_source
                .write()
                .map_err(|_| managed_auth::unavailable())?;
            if !matches!(&*source, State::PendingInitial(current) if Arc::ptr_eq(current, &pending))
                || *self.auth_change_state_tx.borrow() != expected
            {
                return Err(managed_auth::unavailable());
            }
            *source = State::Bound(bound.clone());
            return Ok(());
        }
        if let Some(report) = pending.member.retained_preload_completion()? {
            if !matches!(report, native_source::Loaded::PolicyUnavailable) {
                return Err(managed_auth::unavailable());
            }
            pending.member.resume_preload_completion().await?;
            let mut source = self
                .alab_native_source
                .write()
                .map_err(|_| managed_auth::unavailable())?;
            if matches!(&*source, State::PendingInitial(current) if Arc::ptr_eq(current, &pending))
            {
                *source = State::Detached;
            }
            return Err(managed_auth::unavailable());
        }
        let operation = format!("{:032x}", rand::random::<u128>());
        // Lifetime retains the original attempt/op/sequence. This preferred nonce
        // cannot create another scope while its first preload remains pending.
        let packet = pending.member.preload(&operation, None).await?;
        self.alab_initialize_member_source(packet.source, pending.member.clone())
            .await?;
        Ok(())
    }
    pub(super) async fn alab_destination_report(
        &self,
        boundary: &str,
    ) -> std::io::Result<codex_alab_core::native_target::ResourceReport> {
        let scope = self.alab_destination_scope(None).await?;
        let member = self
            .alab_native_member()?
            .ok_or_else(managed_auth::unavailable)?;
        member.target_report(&scope.source, scope.binding().generations, boundary)
    }
    pub(super) async fn alab_destination_binding(
        &self,
    ) -> std::io::Result<codex_alab_core::assignment::Binding> {
        self.alab_destination_scope(None)
            .await
            .map(|scope| scope.binding())
    }
    pub(super) async fn alab_destination_scope(
        &self,
        report: Option<&codex_alab_core::native_target::ResourceReport>,
    ) -> std::io::Result<NativeSourceScope> {
        self.alab_real_epoch()?;
        let _refresh = self
            .refresh_lock
            .acquire()
            .await
            .map_err(|_| managed_auth::unavailable())?;
        let bound = self.alab_bound()?.ok_or_else(managed_auth::unavailable)?;
        let cached = self.inner.read().map_err(|_| managed_auth::unavailable())?;
        let Some(CodexAuth::Chatgpt(auth)) = cached.auth.as_ref() else {
            return Err(managed_auth::unavailable());
        };
        let epoch = self
            .alab_assignment_domain
            .capture()
            .map_err(|_| managed_auth::unavailable())?;
        if !auth
            .state
            .alab_epoch
            .epoch()
            .is_some_and(|captured| captured.same(&epoch))
        {
            return Err(managed_auth::unavailable());
        }
        let mut source = bound.source.clone();
        source.revision = bound
            .revision
            .lock()
            .map_err(|_| managed_auth::unavailable())?
            .clone();
        let actual = binding(&source, *self.auth_change_state_tx.borrow());
        let member = bound
            .member
            .as_ref()
            .ok_or_else(managed_auth::unavailable)?;
        let _loaded = member.source_use(&source, &actual.generations)?;
        if let Some(report) = report {
            let expected =
                member.target_report(&source, actual.generations, report.boundary())?;
            if report.binding() != expected.binding()
                || report.member() != expected.member()
                || report.registration() != expected.registration()
                || report.source().slot != source.slot
                || report.source().path != source.path
                || report.source().native_intent != source.native_intent
            {
                return Err(managed_auth::unavailable());
            }
        }
        let tokens = auth
            .current_token_data()
            .ok_or_else(managed_auth::unavailable)?;
        Ok(NativeSourceScope {
            domain: epoch.domain(),
            source,
            expected: *self.auth_change_state_tx.borrow(),
            grant: grant_projection(&tokens),
            member: Some(member.incarnation()),
        })
    }

    pub(super) async fn alab_initialize_member_source(
        &self,
        source: native_source::Source,
        member: codex_alab_core::native_source_lifetime::Lifetime,
    ) -> std::io::Result<AuthChangeState> {
        self.alab_real_epoch()?;
        member.permits_storage(&source)?;
        let bound = Arc::new(Bound {
            reader: std::sync::OnceLock::new(),
            revision: Mutex::new(source.revision.clone()),
            source: source.clone(),
            member: Some(member.clone()),
        });
        let candidate = self.alab_load_bound(Arc::clone(&bound), None).await?;
        let (actual, reported) = {
            let mut selected = self
                .alab_native_source
                .write()
                .map_err(|_| managed_auth::unavailable())?;
            let mut cached = self
                .inner
                .write()
                .map_err(|_| managed_auth::unavailable())?;
            bound.read()?;
            self.set_cached_auth_locked(&mut cached, candidate);
            let actual = *self.auth_change_state_tx.borrow();
            let reported = Arc::new(PendingInitial {
                member: member.clone(),
                selected: None,
                bound: Some(bound),
                reported: Some(actual),
                recovery: None,
            });
            *selected = State::PendingInitial(reported.clone());
            (actual, reported)
        };
        member
            .source_ready(&source, binding(&source, actual).generations)
            .await
            .map_err(|_| managed_auth::unavailable())?;
        let mut selected = self
            .alab_native_source
            .write()
            .map_err(|_| managed_auth::unavailable())?;
        let State::PendingInitial(pending) = &*selected else {
            return Err(managed_auth::unavailable());
        };
        if !Arc::ptr_eq(pending, &reported)
            || !pending.member.same(&member)
            || pending.reported != Some(actual)
            || *self.auth_change_state_tx.borrow() != actual
        {
            return Err(managed_auth::unavailable());
        }
        *selected = State::Bound(
            pending
                .bound
                .clone()
                .ok_or_else(managed_auth::unavailable)?,
        );
        Ok(actual)
    }

    pub(super) async fn alab_native_account_tokens(
        &self,
    ) -> Option<std::io::Result<Option<TokenData>>> {
        let bound = match self.alab_bound() {
            Ok(Some(bound)) => bound,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        Some(
            tokio::task::spawn_blocking(move || bound.read().map(|document| document.tokens))
                .await
                .map_err(|_| managed_auth::unavailable())
                .and_then(std::convert::identity),
        )
    }

    /// Wake dispatch on actual epoch publication without fabricating auth counters.
    pub async fn alab_assignment_domain_changed(
        &self,
        previous: &Arc<codex_alab_core::assignment::Domain>,
    ) -> std::io::Result<()> {
        if let Some(host) = &self.alab_context_host {
            loop {
                let current = host.capture()?;
                if !Arc::ptr_eq(&current.alab_assignment_domain(), previous) {
                    return Ok(());
                }
                tokio::select! {
                    changed = host.changed(&current) => changed?,
                    changed = current.alab_assignment_domain.domain_changed(previous) => changed.map_err(|_| managed_auth::unavailable())?,
                }
            }
        }
        self.alab_assignment_domain
            .domain_changed(previous)
            .await
            .map_err(|_| managed_auth::unavailable())
    }

    /// Checks this manager's own policy; a process coordinator must visit every registered manager.
    pub(super) fn alab_destination_constraints(
        &self,
        target: &codex_alab_core::assignment_service::Target,
    ) -> std::io::Result<()> {
        self.alab_real_epoch()?;
        target.validate(&target.operation)?;
        if self.workload_identity_selected
            || self.has_external_auth()
            || !self
                .allowed_login_methods()
                .contains(&ForcedLoginMethod::Chatgpt)
            || self
                .allowed_auth_modes
                .read()
                .map_err(|_| managed_auth::unavailable())?
                .as_ref()
                .is_some_and(|modes| !modes.contains(&AuthMode::Chatgpt))
            || self
                .effective_chatgpt_workspaces()
                .is_some_and(|workspaces| {
                    !workspaces.iter().any(|workspace| {
                        principal(b"aterm/codex-renewal/account/v1\0", workspace) == target.account
                    })
                })
        {
            return Err(managed_auth::unavailable());
        }
        Ok(())
    }

    pub fn alab_check_native_target(
        &self,
        target: &codex_alab_core::assignment_service::Target,
    ) -> std::io::Result<()> {
        self.alab_destination_constraints(target)?;
        let Some(CodexAuth::Chatgpt(auth)) = self.auth_cached() else {
            return Err(managed_auth::unavailable());
        };
        let epoch = self
            .alab_assignment_domain
            .capture()
            .map_err(|_| managed_auth::unavailable())?;
        if !auth
            .state
            .alab_epoch
            .epoch()
            .is_some_and(|captured| captured.same(&epoch))
        {
            return Err(managed_auth::unavailable());
        }
        let bound = self.alab_bound()?.ok_or_else(managed_auth::unavailable)?;
        if !auth
            .storage
            .alab_source_scope()
            .is_some_and(|source| bound.same_scope(&source))
        {
            return Err(managed_auth::unavailable());
        }
        let tokens = auth
            .current_token_data()
            .ok_or_else(managed_auth::unavailable)?;
        target_policy::document(&tokens, target, target_policy::Side::Source)
    }

    pub fn alab_verify_native_target(
        &self,
        target: &codex_alab_core::assignment_service::Target,
    ) -> std::io::Result<()> {
        self.alab_real_epoch()?;
        let Some(CodexAuth::Chatgpt(auth)) = self.auth_cached() else {
            return Err(managed_auth::unavailable());
        };
        let epoch = self
            .alab_assignment_domain
            .capture()
            .map_err(|_| managed_auth::unavailable())?;
        if !auth
            .state
            .alab_epoch
            .epoch()
            .is_some_and(|captured| captured.same(&epoch))
        {
            return Err(managed_auth::unavailable());
        }
        let bound = self.alab_bound()?.ok_or_else(managed_auth::unavailable)?;
        if bound.source.native_intent.as_deref() != Some(target.operation.as_str())
            || bound.source.account != target.account
            || bound.source.user != target.user
            || bound.source.assignment_generation <= target.expected_assignment
            || !auth
                .storage
                .alab_source_scope()
                .is_some_and(|source| bound.same_scope(&source))
        {
            return Err(managed_auth::unavailable());
        }
        let tokens = auth
            .current_token_data()
            .ok_or_else(managed_auth::unavailable)?;
        target_policy::document(&tokens, target, target_policy::Side::Destination)
    }

    /// Proves the current managed grant before any healthy assignment commit.
    pub async fn alab_native_source_fence(&self) -> std::io::Result<NativeSourceFence> {
        self.alab_real_epoch()?;
        let refresh = Arc::clone(&self.refresh_lock)
            .acquire_owned()
            .await
            .map_err(|_| managed_auth::unavailable())?;
        if !codex_alab_core::is_alab_binary()
            || self.workload_identity_selected
            || self.has_external_auth()
        {
            return Err(managed_auth::unavailable());
        }
        let expected = *self.auth_change_state_tx.borrow();
        let Some(CodexAuth::Chatgpt(current)) = self.auth_cached() else {
            return Err(managed_auth::unavailable());
        };
        let tokens = current
            .current_token_data()
            .ok_or_else(managed_auth::unavailable)?;
        let slot = self
            .alab_managed_binding(&tokens)?
            .ok_or_else(managed_auth::unavailable)?;
        // No local file lock is held across this authenticated host request.
        let source = native_source::source(&format!("{:032x}", rand::random::<u128>())).await?;
        if source.slot != slot || Some(source.path.as_path()) != self.alab_login_file().as_deref() {
            return Err(managed_auth::unavailable());
        }
        let bound = Bound {
            reader: std::sync::OnceLock::new(),
            revision: Mutex::new(source.revision.clone()),
            source: source.clone(),
            member: self.alab_member_for_source(&source)?,
        };
        let document = bound.read()?;
        if super::alab_auth_refresh_lock::managed_grant_relation(
            &tokens,
            document
                .tokens
                .as_ref()
                .ok_or_else(managed_auth::unavailable)?,
        ) != super::alab_auth_refresh_lock::ManagedGrantRelation::Same
            || *self.auth_change_state_tx.borrow() != expected
        {
            return Err(managed_auth::unavailable());
        }
        let grant = grant_projection(
            document
                .tokens
                .as_ref()
                .ok_or_else(managed_auth::unavailable)?,
        );
        Ok(NativeSourceFence {
            domain: self.alab_assignment_domain.domain(),
            _refresh: refresh,
            member: self.alab_member_for_source(&source)?,
            expected,
            source,
            grant,
        })
    }

    /// Rebinds with refresh still excluded by the exact manager's precommit fence.
    pub async fn alab_rebind_native_source_fenced(
        &self,
        operation: &str,
        fence: &NativeSourceFence,
        boundary: &codex_alab_core::assignment::BoundaryPermit,
    ) -> std::io::Result<AuthChangeState> {
        self.alab_real_epoch()?;
        if !Arc::ptr_eq(&fence.domain, &self.alab_assignment_domain()) {
            return Err(managed_auth::unavailable());
        }
        if !boundary.protects(&self.alab_assignment_domain(), operation, Some(operation)) {
            // An A boundary is not B read/factory authority. Only acknowledge the
            // exact B state already installed and recorded by this same operation.
            if boundary.operation() == operation
                && boundary.native_intent() == Some(operation)
                && fence.source.native_intent.as_deref() == Some(operation)
                && *self.auth_change_state_tx.borrow() == fence.expected
                && boundary
                    .bound()
                    .is_ok_and(|actual| actual == binding(&fence.source, fence.expected))
                && self.alab_bound()?.is_some_and(|bound| {
                    bound.same_scope(&fence.source)
                        && bound
                            .revision
                            .lock()
                            .is_ok_and(|revision| *revision == fence.source.revision)
                })
            {
                return Ok(fence.expected);
            }
            return Err(managed_auth::unavailable());
        }
        let source = native_source::source(&format!("{:032x}", rand::random::<u128>())).await?;
        self.alab_install_native_source_locked(source, fence.expected, boundary)
            .await
    }

    /// Pins the already-selected source without writing a file or authorizing another grant.
    pub async fn alab_pin_current_native_source(
        &self,
        fence: &mut NativeSourceFence,
        boundary: &codex_alab_core::assignment::BoundaryPermit,
    ) -> std::io::Result<AuthChangeState> {
        self.alab_real_epoch()?;
        if !Arc::ptr_eq(&fence.domain, &self.alab_assignment_domain())
            || !boundary.protects(
                &self.alab_assignment_domain(),
                boundary.operation(),
                boundary.native_intent(),
            )
            || *self.auth_change_state_tx.borrow() != fence.expected
        {
            return Err(managed_auth::unavailable());
        }
        if let Some(current) = self.alab_bound()?
            && current.same_scope(&fence.source)
            && *current
                .revision
                .lock()
                .map_err(|_| managed_auth::unavailable())?
                == fence.source.revision
        {
            current.read()?;
            return Ok(fence.expected);
        }
        let Some(CodexAuth::Chatgpt(mut current)) = self.auth_cached() else {
            return Err(managed_auth::unavailable());
        };
        let bound = Arc::new(Bound {
            reader: std::sync::OnceLock::new(),
            source: fence.source.clone(),
            revision: Mutex::new(fence.source.revision.clone()),
            member: self.alab_member_for_source(&fence.source)?,
        });
        bound.read()?;
        current.storage = bound.clone();
        let mut source = self
            .alab_native_source
            .write()
            .map_err(|_| managed_auth::unavailable())?;
        let mut cached = self
            .inner
            .write()
            .map_err(|_| managed_auth::unavailable())?;
        if matches!(*source, State::Detached)
            || *self.auth_change_state_tx.borrow() != fence.expected
        {
            return Err(managed_auth::unavailable());
        }
        *source = State::Bound(bound);
        self.auth_change_state_tx
            .send_modify(|state| state.generation += 1);
        // The fence proves this exact existing A document/source. Adopting its
        // storage adapter is not a B publication or a new epoch.
        cached.auth = Some(CodexAuth::Chatgpt(current));
        self.alab_bind_cached_epoch(&mut cached.auth);
        fence.expected = *self.auth_change_state_tx.borrow();
        Ok(fence.expected)
    }

    /// A definite native no-install result may restore only the original source scope.
    pub async fn alab_restore_unchanged_native_source(
        &self,
        fence: &mut NativeSourceFence,
        boundary: &codex_alab_core::assignment::BoundaryPermit,
        terminal: &codex_alab_core::assignment_service::Status,
    ) -> std::io::Result<AuthChangeState> {
        self.alab_real_epoch()?;
        use codex_alab_core::assignment_service::Phase;
        terminal.validate(boundary.operation())?;
        if !boundary.restoration_allowed()
            || !matches!(terminal.phase, Phase::Cancelled | Phase::Refused)
            || terminal.assignment_generation.is_some()
            || terminal.credential_generation.is_some()
            || terminal
                .boundary
                .as_deref()
                .is_some_and(|value| value != boundary.boundary())
            || !Arc::ptr_eq(&fence.domain, &self.alab_assignment_domain())
            || !boundary.protects(
                &self.alab_assignment_domain(),
                boundary.operation(),
                boundary.native_intent(),
            )
        {
            return Err(managed_auth::unavailable());
        }
        let fresh = native_source::source(&format!("{:032x}", rand::random::<u128>())).await?;
        let original = Bound {
            reader: std::sync::OnceLock::new(),
            source: fence.source.clone(),
            revision: Mutex::new(fence.source.revision.clone()),
            member: self.alab_member_for_source(&fence.source)?,
        };
        if !original.same_scope(&fresh) {
            return Err(managed_auth::unavailable());
        }
        let expected = *self.auth_change_state_tx.borrow();
        if expected.owner_generation != fence.expected.owner_generation
            || expected.source_generation != fence.expected.source_generation
            || expected.login_generation != fence.expected.login_generation
        {
            return Err(managed_auth::unavailable());
        }
        let Some(CodexAuth::Chatgpt(current)) = self.auth_cached() else {
            return Err(managed_auth::unavailable());
        };
        let tokens = current
            .current_token_data()
            .ok_or_else(managed_auth::unavailable)?;
        let bound = Arc::new(Bound {
            reader: std::sync::OnceLock::new(),
            source: fresh.clone(),
            revision: Mutex::new(fresh.revision.clone()),
            member: self.alab_member_for_source(&fresh)?,
        });
        let document = bound.read()?;
        if super::alab_auth_refresh_lock::managed_grant_relation(
            &tokens,
            document
                .tokens
                .as_ref()
                .ok_or_else(managed_auth::unavailable)?,
        ) != super::alab_auth_refresh_lock::ManagedGrantRelation::Same
        {
            return Err(managed_auth::unavailable());
        }
        let _file = match codex_alab_core::auth_lock::admit(
            Some(&fresh.path),
            codex_alab_core::auth_lock::WAIT,
            || async { false },
        )
        .await
        {
            codex_alab_core::auth_lock::Admission::Send(hold) => hold,
            _ => return Err(managed_auth::unavailable()),
        };
        let candidate = self.alab_load_bound(Arc::clone(&bound), None).await?;
        let mut source = self
            .alab_native_source
            .write()
            .map_err(|_| managed_auth::unavailable())?;
        let mut cached = self
            .inner
            .write()
            .map_err(|_| managed_auth::unavailable())?;
        if *self.auth_change_state_tx.borrow() != expected || matches!(*source, State::Detached) {
            return Err(managed_auth::unavailable());
        }
        bound.read()?;
        *source = State::Bound(bound);
        self.auth_change_state_tx
            .send_modify(|state| state.generation += 1);
        self.set_cached_auth_locked(&mut cached, candidate);
        let state = *self.auth_change_state_tx.borrow();
        drop(cached);
        drop(source);
        boundary
            .record_restored(binding(&fresh, state))
            .map_err(|_| managed_auth::unavailable())?;
        fence.source = fresh;
        fence.expected = state;
        Ok(state)
    }

    pub(super) fn alab_admit_source_mutation(
        &self,
    ) -> std::io::Result<codex_alab_core::assignment::Activity> {
        self.alab_real_epoch()?;
        if matches!(
            &*self
                .alab_native_source
                .read()
                .map_err(|_| managed_auth::unavailable())?,
            State::PendingInitial(_)
        ) {
            return Err(managed_auth::unavailable());
        }
        self.alab_assignment_domain()
            .admit_mutation()
            .map_err(|_| managed_auth::unavailable())
    }
    pub fn alab_account_resource(&self) -> std::io::Result<codex_alab_core::assignment::Activity> {
        if let Some(actual) = self.alab_host_capture() {
            return actual?.alab_account_resource();
        }
        self.alab_assignment_domain()
            .admit_current_resource()
            .map_err(|_| managed_auth::unavailable())
    }

    pub fn alab_assignment_domain(&self) -> Arc<codex_alab_core::assignment::Domain> {
        if let Some(actual) = self.alab_host_metadata() {
            return actual.alab_assignment_domain();
        }
        self.alab_assignment_domain.domain()
    }
    pub(super) fn alab_native_workspace_switch(&self) -> std::io::Result<()> {
        if self.alab_native_overridden() {
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "Native account selection requires native owner custody.",
            ))
        } else {
            Ok(())
        }
    }
    pub(super) fn alab_native_overridden(&self) -> bool {
        self.alab_native_source
            .read()
            .map(|state| !matches!(*state, State::Startup))
            .unwrap_or(true)
    }

    fn alab_bound(&self) -> std::io::Result<Option<Arc<Bound>>> {
        self.alab_real_epoch()?;
        match &*self
            .alab_native_source
            .read()
            .map_err(|_| managed_auth::unavailable())?
        {
            State::Startup => Ok(None),
            State::Bound(bound) => Ok(Some(Arc::clone(bound))),
            State::PendingInitial(_) => Err(managed_auth::unavailable()),
            State::Detached => Err(managed_auth::unavailable()),
        }
    }

    pub(super) fn alab_native_path(&self) -> std::io::Result<Option<PathBuf>> {
        self.alab_bound()
            .map(|bound| bound.map(|bound| bound.source.path.clone()))
    }

    pub(super) fn alab_native_binding(&self) -> Option<std::io::Result<Option<String>>> {
        if !self.alab_native_overridden() {
            return None;
        }
        Some(self.alab_bound().and_then(|bound| {
            let bound = bound.ok_or_else(managed_auth::unavailable)?;
            if managed_auth::binding_for_path(true, None, Some(&bound.source.path))?.as_deref()
                != Some(bound.source.slot.as_str())
            {
                return Err(managed_auth::unavailable());
            }
            Ok(Some(bound.source.slot.clone()))
        }))
    }

    async fn alab_load_bound(
        &self,
        bound: Arc<Bound>,
        cached: Option<&CodexAuth>,
    ) -> std::io::Result<Option<CodexAuth>> {
        let methods = self.allowed_login_methods();
        if self
            .allowed_auth_modes
            .read()
            .map_err(|_| managed_auth::unavailable())?
            .as_ref()
            .is_some_and(|modes| !modes.contains(&AuthMode::Chatgpt))
            || !auth_mode_is_allowed(Some(&methods), AuthMode::Chatgpt)
        {
            if let Some(member) = &bound.member {
                member.seal();
            }
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "native account is excluded by authentication policy",
            ));
        }
        let reader = Arc::clone(&bound);
        let document = tokio::task::spawn_blocking(move || reader.read())
            .await
            .map_err(|_| managed_auth::unavailable())??;
        let route = match &bound.member {
            Some(member) => self
                .auth_route_config
                .alab_with_native_source(member.network_scope().map_err(|_| ())),
            None => self.auth_route_config.clone(),
        };
        let cached = if bound.member.is_some() { None } else { cached };
        let mut auth = CodexAuth::from_auth_dot_json(
            &self.codex_home,
            document,
            AuthCredentialsStoreMode::File,
            self.chatgpt_base_url.as_deref(),
            self.keyring_backend_kind,
            self.agent_identity_authapi_base_url.as_deref(),
            &route,
            cached,
        )
        .await?;
        validate_auth_restrictions(
            Some(&methods),
            self.effective_chatgpt_workspaces().as_deref(),
            &auth,
        )
        .map_err(|_| {
            if let Some(member) = &bound.member {
                member.seal();
            }
            std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "native account is excluded by authentication policy",
            )
        })?;
        let CodexAuth::Chatgpt(chatgpt) = &mut auth else {
            return Err(managed_auth::unavailable());
        };
        chatgpt.storage = bound;
        Ok(Some(auth))
    }

    pub(super) async fn alab_native_load(
        &self,
        cached: Option<&CodexAuth>,
    ) -> Option<std::io::Result<Option<CodexAuth>>> {
        self.alab_native_load_kind(cached, true).await
    }
    pub(super) async fn alab_native_storage_load(
        &self,
        cached: Option<&CodexAuth>,
    ) -> Option<std::io::Result<Option<CodexAuth>>> {
        self.alab_native_load_kind(cached, false).await
    }
    async fn alab_native_load_kind(
        &self,
        cached: Option<&CodexAuth>,
        allow_external: bool,
    ) -> Option<std::io::Result<Option<CodexAuth>>> {
        let bound = {
            let state = match self.alab_native_source.read() {
                Ok(state) => state,
                Err(_) => return Some(Err(managed_auth::unavailable())),
            };
            match &*state {
                State::Startup => return self.alab_unrelated_load(allow_external),
                State::Detached => return Some(Ok(None)),
                State::PendingInitial(_) => return Some(Err(managed_auth::unavailable())),
                State::Bound(bound) => Arc::clone(bound),
            }
        };
        Some(self.alab_load_bound(bound, cached).await)
    }

    /// Observe only the exact receipt already installed by this manager's
    /// predecessor boundary. This performs no source RPC or credential file I/O.
    fn alab_observe_recorded_binding(
        &self,
        operation: &str,
        boundary: &codex_alab_core::assignment::BoundaryPermit,
    ) -> Option<AuthChangeState> {
        let epoch = self.alab_source_epoch().ok()?;
        let predecessor = epoch.predecessor_domain()?;
        if !boundary.protects(&predecessor, operation, Some(operation)) {
            return None;
        }
        let source = self.alab_native_source.read().ok()?;
        let State::Bound(bound) = &*source else {
            return None;
        };
        if bound.source.native_intent.as_deref() != Some(operation) {
            return None;
        }
        let cached = self.inner.read().ok()?;
        let Some(CodexAuth::Chatgpt(auth)) = &cached.auth else {
            return None;
        };
        if !auth.state.alab_epoch.epoch()?.same(&epoch) {
            return None;
        }
        if !bound.same_scope(&auth.storage.alab_source_scope()?) {
            return None;
        }
        let mut proof = bound.source.clone();
        proof.revision = bound.revision.lock().ok()?.clone();
        let actual = *self.auth_change_state_tx.borrow();
        if boundary.bound().ok()? != binding(&proof, actual) {
            return None;
        }
        Some(actual)
    }

    /// Called by the assignment consumer only with its process barrier retained.
    pub async fn alab_rebind_native_source(
        &self,
        operation: &str,
        expected: AuthChangeState,
        boundary: &codex_alab_core::assignment::BoundaryPermit,
    ) -> std::io::Result<AuthChangeState> {
        self.alab_real_epoch()?;
        if expected != *self.auth_change_state_tx.borrow()
            && let Some(actual) = self.alab_observe_recorded_binding(operation, boundary)
        {
            return Ok(actual);
        }
        if !boundary.protects(
            &self.alab_assignment_domain(),
            operation,
            boundary.native_intent(),
        ) {
            return Err(managed_auth::unavailable());
        }
        let source = native_source::source(&format!("{:032x}", rand::random::<u128>())).await?;
        self.alab_install_native_source(source, expected, boundary)
            .await
    }

    async fn alab_install_native_source(
        &self,
        source: native_source::Source,
        expected: AuthChangeState,
        boundary: &codex_alab_core::assignment::BoundaryPermit,
    ) -> std::io::Result<AuthChangeState> {
        self.alab_real_epoch()?;
        let _refresh = self
            .refresh_lock
            .acquire()
            .await
            .map_err(|_| managed_auth::unavailable())?;
        self.alab_install_native_source_locked(source, expected, boundary)
            .await
    }

    async fn alab_install_native_source_locked(
        &self,
        source: native_source::Source,
        expected: AuthChangeState,
        boundary: &codex_alab_core::assignment::BoundaryPermit,
    ) -> std::io::Result<AuthChangeState> {
        self.alab_real_epoch()?;
        source.validate()?;
        if !boundary.protects(
            &self.alab_assignment_domain(),
            boundary.operation(),
            source.native_intent.as_deref(),
        ) {
            return Err(managed_auth::unavailable());
        }
        if source.assignment_generation == 0 {
            return Err(managed_auth::unavailable());
        }
        if !codex_alab_core::is_alab_binary()
            || self.workload_identity_selected
            || self.has_external_auth()
        {
            return Err(managed_auth::unavailable());
        }
        if let Some(current) = self.alab_bound()? {
            let same_revision = current
                .revision
                .lock()
                .map_err(|_| managed_auth::unavailable())?
                .eq(&source.revision);
            if current.same_scope(&source) && same_revision {
                let verified = self.alab_load_bound(current, None).await?;
                let cached = self.inner.read().map_err(|_| managed_auth::unavailable())?;
                if Self::auths_equal(cached.auth.as_ref(), verified.as_ref()) {
                    let published = *self.auth_change_state_tx.borrow();
                    drop(cached);
                    boundary
                        .record_bound(binding(&source, published))
                        .map_err(|_| managed_auth::unavailable())?;
                    return Ok(published);
                }
            }
        }
        if *self.auth_change_state_tx.borrow() != expected {
            return Err(managed_auth::unavailable());
        }
        let Some(CodexAuth::Chatgpt(current)) = self.auth_cached() else {
            return Err(managed_auth::unavailable());
        };
        let tokens = current
            .current_token_data()
            .ok_or_else(managed_auth::unavailable)?;
        if self.alab_managed_binding(&tokens)?.is_none() {
            return Err(managed_auth::unavailable());
        }
        let bound = Arc::new(Bound {
            reader: std::sync::OnceLock::new(),
            revision: Mutex::new(source.revision.clone()),
            member: self.alab_member_for_source(&source)?,
            source,
        });
        let _file = match codex_alab_core::auth_lock::admit(
            Some(&bound.source.path),
            codex_alab_core::auth_lock::WAIT,
            || async { false },
        )
        .await
        {
            codex_alab_core::auth_lock::Admission::Send(hold) => hold,
            _ => return Err(managed_auth::unavailable()),
        };
        let candidate = self.alab_load_bound(Arc::clone(&bound), None).await?;
        let mut source = self
            .alab_native_source
            .write()
            .map_err(|_| managed_auth::unavailable())?;
        let mut cached = self
            .inner
            .write()
            .map_err(|_| managed_auth::unavailable())?;
        if *self.auth_change_state_tx.borrow() != expected {
            return Err(managed_auth::unavailable());
        }
        bound.read()?;
        let proof = bound.source.clone();
        *source = State::Bound(bound);
        self.auth_change_state_tx
            .send_modify(|state| state.generation += 1);
        self.set_cached_auth_locked(&mut cached, candidate);
        let published = *self.auth_change_state_tx.borrow();
        drop(cached);
        drop(source);
        boundary
            .record_bound(binding(&proof, published))
            .map_err(|_| managed_auth::unavailable())?;
        Ok(published)
    }

    /// Clone only while source, cache and native occurrence agree. A required
    /// member disappearing cannot turn that source into legacy cache access.
    pub(super) fn alab_member_cache_snapshot(&self) -> Option<NativeCacheSnapshot<'_>> {
        let source = self.alab_native_source.read().ok()?;
        let cached = self.inner.read().ok()?;
        let members = self.alab_source_member.read().ok()?;
        let auth = cached.auth.as_ref()?;
        let origin = auth.alab_credential_epoch();
        let legacy =
            || !codex_alab_core::native_source_launch::requested() && !origin.has_native_member();
        let allowed = match &*source {
            State::PendingInitial(_) | State::Detached => false,
            State::Bound(bound) => match bound.member.as_ref() {
                Some(expected) => members.as_ref().is_some_and(|actual| {
                    actual.same(expected) && actual.ready() && origin.same_native_member(actual)
                }),
                None => members.is_none() && legacy(),
            },
            State::Startup => match members.as_ref() {
                Some(actual) => actual.ready() && origin.same_native_member(actual),
                None => legacy(),
            },
        };
        if !allowed {
            return None;
        }
        let mut auth = Some(auth.clone());
        // A fresh read from the actual cache may capture the application's new
        // policy generation. Retained clones keep their old policy, and the
        // existing epoch/member checks forbid relabelling another source.
        self.alab_bind_cached_epoch_from_member(&mut auth, Ok(members.clone()));
        let auth = auth?;
        // Factory construction may acquire member metadata again; retain only
        // the cache reader, preserving the existing inner->factory lock path.
        drop(members);
        drop(source);
        Some(NativeCacheSnapshot {
            auth,
            _cache: cached,
        })
    }

    /// Recover only the exact already-published source after transport loss.
    /// Native ACK is retained separately; cache/source/policy validation owns
    /// the final synchronous publication, in Source -> cache -> member order.
    pub(super) async fn alab_reconcile_native_work(&self) -> std::io::Result<()> {
        let Some(member) = self
            .alab_source_member
            .read()
            .map_err(|_| managed_auth::unavailable())?
            .clone()
        else {
            return Ok(());
        };
        if member.ready() {
            return Ok(());
        }
        let bound = self.alab_bound()?.ok_or_else(managed_auth::unavailable)?;
        if !bound
            .member
            .as_ref()
            .is_some_and(|actual| actual.same(&member))
        {
            return Err(managed_auth::unavailable());
        }
        let Some(native_source::Loaded::Source {
            source,
            generations,
        }) = member.retained_preload_completion()?
        else {
            return Err(managed_auth::unavailable());
        };
        if binding(&source, *self.auth_change_state_tx.borrow()).generations != generations {
            return Err(managed_auth::unavailable());
        }
        let expected = *self.auth_change_state_tx.borrow();
        let epoch = self
            .alab_source_epoch()
            .map_err(|_| managed_auth::unavailable())?;
        {
            let cached = self.inner.read().map_err(|_| managed_auth::unavailable())?;
            self.alab_validate_rebound_cache(&cached, &bound, &source, expected, &epoch)?;
        }
        // Only the native physical-source observation may select changed
        // custody. EOF itself grants no local credential-file read.
        let prepared = member.prepare_rebound_ready(&source, &generations).await?;
        let recovered = match prepared {
            codex_alab_core::native_source_lifetime::ReboundPreparation::Ready(ticket) => ticket,
            codex_alab_core::native_source_lifetime::ReboundPreparation::SourceChanged => {
                self.alab_begin_rebound_source(bound, member, source.as_ref(), expected, epoch)?;
                return self.alab_resume_initial_source().await;
            }
        };
        let result = self
            .alab_commit_rebound_ready(
                &bound,
                &member,
                &source,
                (expected, &epoch),
                *recovered,
                None,
            )
            .await;
        if result.is_err() {
            member.seal();
        }
        result
    }
    fn alab_begin_rebound_source(
        &self,
        bound: Arc<Bound>,
        member: codex_alab_core::native_source_lifetime::Lifetime,
        source: &native_source::Source,
        expected: AuthChangeState,
        epoch: codex_alab_core::manager_source_epoch::Epoch,
    ) -> std::io::Result<()> {
        let mut current = self
            .alab_native_source
            .write()
            .map_err(|_| managed_auth::unavailable())?;
        let cached = self.inner.read().map_err(|_| managed_auth::unavailable())?;
        self.alab_validate_rebound_cache(&cached, &bound, source, expected, &epoch)?;
        let members = self
            .alab_source_member
            .read()
            .map_err(|_| managed_auth::unavailable())?;
        if !members.as_ref().is_some_and(|actual| actual.same(&member))
            || !matches!(&*current, State::Bound(actual) if Arc::ptr_eq(actual, &bound))
        {
            return Err(managed_auth::unavailable());
        }
        let Some(CodexAuth::Chatgpt(auth)) = cached.auth.as_ref() else {
            return Err(managed_auth::unavailable());
        };
        let tokens = auth
            .current_token_data()
            .ok_or_else(managed_auth::unavailable)?;
        *current = State::PendingInitial(Arc::new(PendingInitial {
            member,
            selected: None,
            bound: None,
            reported: None,
            recovery: Some(Arc::new(ReboundOrigin {
                bound,
                source: source.clone(),
                expected,
                epoch,
                tokens,
            })),
        }));
        Ok(())
    }
    async fn alab_resume_rebound_source(
        &self,
        mut pending: Arc<PendingInitial>,
    ) -> std::io::Result<()> {
        loop {
            let origin = pending
                .recovery
                .clone()
                .ok_or_else(managed_auth::unavailable)?;
            if let (Some(bound), Some(expected)) = (&pending.bound, pending.reported) {
                let ticket = match pending
                    .member
                    .prepare_loaded_ready(
                        &bound.source,
                        &binding(&bound.source, expected).generations,
                    )
                    .await?
                {
                    codex_alab_core::native_source_lifetime::ReboundPreparation::Ready(ticket) => {
                        ticket
                    }
                    codex_alab_core::native_source_lifetime::ReboundPreparation::SourceChanged => {
                        pending = self.alab_advance_rebound_pending(
                            &pending,
                            bound,
                            expected,
                            &origin.epoch,
                        )?;
                        continue;
                    }
                };
                let result = self
                    .alab_commit_rebound_ready(
                        bound,
                        &pending.member,
                        &bound.source,
                        (expected, &origin.epoch),
                        *ticket,
                        Some(&pending),
                    )
                    .await;
                if result.is_ok() {
                    return result;
                }
                self.alab_check_rebound_pending(&pending, bound, expected, &origin.epoch)?;
                match pending
                    .member
                    .prepare_rebound_ready(
                        &bound.source,
                        &binding(&bound.source, expected).generations,
                    )
                    .await?
                {
                    codex_alab_core::native_source_lifetime::ReboundPreparation::Ready(ticket) => {
                        return self
                            .alab_commit_rebound_ready(
                                bound,
                                &pending.member,
                                &bound.source,
                                (expected, &origin.epoch),
                                *ticket,
                                Some(&pending),
                            )
                            .await;
                    }
                    codex_alab_core::native_source_lifetime::ReboundPreparation::SourceChanged => {
                        pending = self.alab_advance_rebound_pending(
                            &pending,
                            bound,
                            expected,
                            &origin.epoch,
                        )?;
                        continue;
                    }
                }
            }
            {
                let current = self
                    .alab_native_source
                    .read()
                    .map_err(|_| managed_auth::unavailable())?;
                let cached = self.inner.read().map_err(|_| managed_auth::unavailable())?;
                if !matches!(&*current, State::PendingInitial(actual) if Arc::ptr_eq(actual, &pending))
                {
                    return Err(managed_auth::unavailable());
                }
                self.alab_validate_rebound_cache(
                    &cached,
                    &origin.bound,
                    &origin.source,
                    origin.expected,
                    &origin.epoch,
                )?;
                let Some(CodexAuth::Chatgpt(auth)) = cached.auth.as_ref() else {
                    return Err(managed_auth::unavailable());
                };
                if auth.current_token_data().as_ref() != Some(&origin.tokens) {
                    return Err(managed_auth::unavailable());
                }
            }
            let operation = format!("{:032x}", rand::random::<u128>());
            // This is an actual retained constructor read, never a fabricated
            // Activity/Begin or permission inferred from the newer physical file.
            let packet = pending.member.rebound_preload(&operation).await?;
            if !origin.bound.same_scope(&packet.source)
                || packet.source.revision.generation <= origin.source.revision.generation
            {
                return Err(managed_auth::unavailable());
            }
            let next = Arc::new(Bound {
                source: packet.source.clone(),
                revision: Mutex::new(packet.source.revision.clone()),
                member: Some(pending.member.clone()),
                reader: std::sync::OnceLock::new(),
            });
            let hold = match codex_alab_core::auth_lock::admit(
                Some(&next.source.path),
                codex_alab_core::auth_lock::WAIT,
                || async { false },
            )
            .await
            {
                codex_alab_core::auth_lock::Admission::Send(hold) => hold,
                _ => return Err(managed_auth::unavailable()),
            };
            let candidate = self.alab_load_bound(next.clone(), None).await?;
            let Some(CodexAuth::Chatgpt(auth)) = candidate.as_ref() else {
                return Err(managed_auth::unavailable());
            };
            let tokens = auth
                .current_token_data()
                .ok_or_else(managed_auth::unavailable)?;
            if super::alab_auth_refresh_lock::managed_grant_relation(&origin.tokens, &tokens)
                != super::alab_auth_refresh_lock::ManagedGrantRelation::Same
            {
                return Err(managed_auth::unavailable());
            }
            let reported = {
                let mut selected = self
                    .alab_native_source
                    .write()
                    .map_err(|_| managed_auth::unavailable())?;
                let mut cached = self
                    .inner
                    .write()
                    .map_err(|_| managed_auth::unavailable())?;
                let members = self
                    .alab_source_member
                    .read()
                    .map_err(|_| managed_auth::unavailable())?;
                if !matches!(&*selected, State::PendingInitial(actual) if Arc::ptr_eq(actual, &pending))
                    || !members
                        .as_ref()
                        .is_some_and(|actual| actual.same(&pending.member))
                {
                    return Err(managed_auth::unavailable());
                }
                self.alab_validate_rebound_cache(
                    &cached,
                    &origin.bound,
                    &origin.source,
                    origin.expected,
                    &origin.epoch,
                )?;
                let Some(CodexAuth::Chatgpt(old)) = cached.auth.as_ref() else {
                    return Err(managed_auth::unavailable());
                };
                if old.current_token_data().as_ref() != Some(&origin.tokens)
                    || next.read()?.tokens.as_ref() != Some(&tokens)
                {
                    return Err(managed_auth::unavailable());
                }
                self.set_cached_auth_locked(&mut cached, candidate);
                let actual = *self.auth_change_state_tx.borrow();
                let reported = Arc::new(PendingInitial {
                    member: pending.member.clone(),
                    selected: None,
                    bound: Some(next),
                    reported: Some(actual),
                    recovery: Some(origin.clone()),
                });
                *selected = State::PendingInitial(reported.clone());
                reported
            };
            drop(hold);
            pending = reported;
        }
    }
    fn alab_check_rebound_pending(
        &self,
        pending: &Arc<PendingInitial>,
        bound: &Arc<Bound>,
        expected: AuthChangeState,
        epoch: &codex_alab_core::manager_source_epoch::Epoch,
    ) -> std::io::Result<()> {
        let current = self
            .alab_native_source
            .read()
            .map_err(|_| managed_auth::unavailable())?;
        let cached = self.inner.read().map_err(|_| managed_auth::unavailable())?;
        let members = self
            .alab_source_member
            .read()
            .map_err(|_| managed_auth::unavailable())?;
        if !matches!(&*current, State::PendingInitial(actual) if Arc::ptr_eq(actual, pending))
            || !members
                .as_ref()
                .is_some_and(|actual| actual.same(&pending.member))
        {
            return Err(managed_auth::unavailable());
        }
        self.alab_validate_rebound_cache(&cached, bound, &bound.source, expected, epoch)
    }
    fn alab_advance_rebound_pending(
        &self,
        pending: &Arc<PendingInitial>,
        bound: &Arc<Bound>,
        expected: AuthChangeState,
        epoch: &codex_alab_core::manager_source_epoch::Epoch,
    ) -> std::io::Result<Arc<PendingInitial>> {
        let mut current = self
            .alab_native_source
            .write()
            .map_err(|_| managed_auth::unavailable())?;
        let cached = self.inner.read().map_err(|_| managed_auth::unavailable())?;
        let members = self
            .alab_source_member
            .read()
            .map_err(|_| managed_auth::unavailable())?;
        if !matches!(&*current, State::PendingInitial(actual) if Arc::ptr_eq(actual, pending))
            || !members
                .as_ref()
                .is_some_and(|actual| actual.same(&pending.member))
        {
            return Err(managed_auth::unavailable());
        }
        self.alab_validate_rebound_cache(&cached, bound, &bound.source, expected, epoch)?;
        let Some(CodexAuth::Chatgpt(auth)) = cached.auth.as_ref() else {
            return Err(managed_auth::unavailable());
        };
        let tokens = auth
            .current_token_data()
            .ok_or_else(managed_auth::unavailable)?;
        let next = Arc::new(PendingInitial {
            member: pending.member.clone(),
            selected: None,
            bound: None,
            reported: None,
            recovery: Some(Arc::new(ReboundOrigin {
                bound: bound.clone(),
                source: bound.source.clone(),
                expected,
                epoch: epoch.clone(),
                tokens,
            })),
        });
        *current = State::PendingInitial(next.clone());
        Ok(next)
    }
    fn alab_validate_rebound_cache(
        &self,
        cached: &CachedAuth,
        bound: &Bound,
        source: &native_source::Source,
        expected: AuthChangeState,
        epoch: &codex_alab_core::manager_source_epoch::Epoch,
    ) -> std::io::Result<()> {
        if *self.auth_change_state_tx.borrow() != expected
            || !bound.same_scope(source)
            || *bound
                .revision
                .lock()
                .map_err(|_| managed_auth::unavailable())?
                != source.revision
            || !self
                .alab_source_epoch()
                .map_err(|_| managed_auth::unavailable())?
                .same(epoch)
        {
            return Err(managed_auth::unavailable());
        }
        let Some(auth @ CodexAuth::Chatgpt(_)) = cached.auth.as_ref() else {
            return Err(managed_auth::unavailable());
        };
        if !auth
            .alab_credential_epoch()
            .epoch()
            .is_some_and(|actual| actual.same(epoch))
            || !self
                .allowed_auth_modes
                .read()
                .map_err(|_| managed_auth::unavailable())?
                .as_ref()
                .is_none_or(|modes| modes.contains(&auth.auth_mode()))
            || !auth_mode_is_allowed(Some(&self.allowed_login_methods()), AuthMode::Chatgpt)
            || self
                .effective_chatgpt_workspaces()
                .is_some_and(|workspaces| {
                    !auth
                        .get_account_id()
                        .is_some_and(|account| workspaces.contains(&account))
                })
        {
            return Err(managed_auth::unavailable());
        }
        Ok(())
    }
    async fn alab_commit_rebound_ready(
        &self,
        bound: &Arc<Bound>,
        member: &codex_alab_core::native_source_lifetime::Lifetime,
        source: &native_source::Source,
        captured: (
            AuthChangeState,
            &codex_alab_core::manager_source_epoch::Epoch,
        ),
        recovered: codex_alab_core::native_source_lifetime::ReboundReady,
        pending: Option<&Arc<PendingInitial>>,
    ) -> std::io::Result<()> {
        let (expected, epoch) = captured;
        // A replay is not permission to relabel a newer physical owner C.
        // No native RPC occurs under this materialization lock.
        let hold = match codex_alab_core::auth_lock::admit(
            Some(&bound.source.path),
            codex_alab_core::auth_lock::WAIT,
            || async { false },
        )
        .await
        {
            codex_alab_core::auth_lock::Admission::Send(hold) => hold,
            _ => return Err(managed_auth::unavailable()),
        };
        let document = bound.read_revision(&source.revision)?;
        let mut current = self
            .alab_native_source
            .write()
            .map_err(|_| managed_auth::unavailable())?;
        let selected = match pending {
            Some(pending) => {
                matches!(&*current, State::PendingInitial(actual) if Arc::ptr_eq(actual, pending))
            }
            None => matches!(&*current, State::Bound(actual) if Arc::ptr_eq(actual, bound)),
        };
        if !selected {
            return Err(managed_auth::unavailable());
        }
        let cached = self.inner.read().map_err(|_| managed_auth::unavailable())?;
        self.alab_validate_rebound_cache(&cached, bound, source, expected, epoch)?;
        let members = self
            .alab_source_member
            .read()
            .map_err(|_| managed_auth::unavailable())?;
        if !members.as_ref().is_some_and(|actual| actual.same(member)) {
            return Err(managed_auth::unavailable());
        }
        let Some(CodexAuth::Chatgpt(auth)) = cached.auth.as_ref() else {
            return Err(managed_auth::unavailable());
        };
        // Retain both mutable policy guards through publication.
        let policy = self
            .allowed_auth_modes
            .read()
            .map_err(|_| managed_auth::unavailable())?;
        let workspace = self
            .forced_chatgpt_workspace_id
            .read()
            .map_err(|_| managed_auth::unavailable())?;
        let methods = self
            .managed_auth_policy
            .allowed_login_methods(self.forced_login_method, workspace.as_deref());
        let workspaces = self
            .managed_auth_policy
            .effective_chatgpt_workspaces(workspace.as_deref());
        if !policy
            .as_ref()
            .is_none_or(|modes| modes.contains(&AuthMode::Chatgpt))
            || !auth_mode_is_allowed(Some(&methods), AuthMode::Chatgpt)
            || workspaces.as_ref().is_some_and(|allowed| {
                !document
                    .tokens
                    .as_ref()
                    .and_then(|tokens| tokens.account_id.as_ref())
                    .is_some_and(|account| allowed.contains(account))
            })
        {
            return Err(managed_auth::unavailable());
        }
        // Cached ChatGPT snapshots share this actual document mutex.
        // Keep its raw metadata stable through the final publication;
        // no get_token()/credential permission is used to compare it.
        let cached_document = auth
            .state
            .auth_dot_json
            .lock()
            .map_err(|_| managed_auth::unavailable())?;
        if cached_document
            .as_ref()
            .and_then(|actual| actual.tokens.as_ref())
            != document.tokens.as_ref()
        {
            return Err(managed_auth::unavailable());
        }
        recovered.commit()?;
        if pending.is_some() {
            *current = State::Bound(bound.clone());
        }
        drop(cached_document);
        drop(workspace);
        drop(policy);
        drop(members);
        drop(cached);
        drop(current);
        drop(hold);
        Ok(())
    }

    pub(super) async fn alab_native_reload(&self) -> std::io::Result<bool> {
        self.alab_resume_initial_source().await?;
        loop {
            let generation = self.auth_change_state_tx.borrow().generation;
            let auth = self
                .alab_native_load(None)
                .await
                .ok_or_else(managed_auth::unavailable)??;
            if let Some(publication) = self.publish_reloaded_auth(generation, auth) {
                return Ok(publication.changed);
            }
        }
    }

    pub(super) fn alab_native_detach(&self) -> std::io::Result<bool> {
        self.alab_real_epoch()?;
        if matches!(
            &*self
                .alab_native_source
                .read()
                .map_err(|_| managed_auth::unavailable())?,
            State::PendingInitial(_)
        ) {
            return Err(managed_auth::unavailable());
        }
        if self.alab_assignment_domain().phase() != codex_alab_core::assignment::Phase::Open {
            return Err(managed_auth::unavailable());
        }
        let mut source = self
            .alab_native_source
            .write()
            .map_err(|_| managed_auth::unavailable())?;
        if matches!(*source, State::Startup) && !self.alab_has_native_member() {
            return Ok(false);
        }
        self.alab_seal_native_member()?;
        *source = State::Detached;
        self.invalidate_pending_auth_reads();
        self.set_cached_auth(None);
        Ok(true)
    }

    pub(super) fn alab_native_note_revision(
        &self,
        slot: &str,
        path: &Path,
        revision: &managed_auth::Revision,
    ) -> std::io::Result<()> {
        let Some(bound) = self.alab_bound()? else {
            return Ok(());
        };
        if bound.source.slot != slot || bound.source.path != path {
            return Err(managed_auth::unavailable());
        }
        bound.acknowledge(revision)
    }

    pub(super) fn alab_native_member(
        &self,
    ) -> std::io::Result<Option<codex_alab_core::native_source_lifetime::Lifetime>> {
        Ok(self.alab_bound()?.and_then(|bound| bound.member.clone()))
    }
    pub(super) async fn alab_native_refresh_owner(
        &self,
        slot: &str,
        digest: &str,
    ) -> std::io::Result<managed_auth::Revision> {
        match self.alab_native_member()? {
            Some(member) => member.refresh(slot, digest).await,
            None => managed_auth::refresh(slot, digest).await,
        }
    }

    /// This RPC is always outside the local materialization flock. Its registration
    /// precedes every read of a newer credential document.
    pub(super) async fn alab_native_refresh_preload(
        &self,
        previous: Option<codex_alab_core::native_preload::Predecessor>,
    ) -> std::io::Result<(
        Option<native_source::Source>,
        Option<codex_alab_core::native_preload::Predecessor>,
    )> {
        let Some(bound) = self.alab_bound()? else {
            return Ok((None, None));
        };
        let operation = format!("{:032x}", rand::random::<u128>());
        let (current, predecessor) = match &bound.member {
            Some(member) => {
                let packet = match previous {
                    Some(previous) => match member.resample_preload(previous).await? {
                        codex_alab_core::native_preload::Resample::Preload(packet) => packet,
                        codex_alab_core::native_preload::Resample::ReadyCommitted(_) => {
                            return Err(managed_auth::unavailable());
                        }
                    },
                    None => member.preload(&operation, None).await?,
                };
                let predecessor = codex_alab_core::native_preload::Predecessor {
                    registration: packet.registration,
                    revision: packet.source.revision.clone(),
                };
                (packet.source, Some(predecessor))
            }
            None => (native_source::source(&operation).await?, None),
        };
        if !bound.same_scope(&current) {
            return Err(managed_auth::unavailable());
        }
        Ok((Some(current), predecessor))
    }

    /// Publish actual cache counters after local reload and after dropping the file lock.
    pub(super) async fn alab_native_refresh_ready(
        &self,
        source: Option<&native_source::Source>,
    ) -> std::io::Result<()> {
        let Some(source) = source else { return Ok(()) };
        let bound = self.alab_bound()?.ok_or_else(managed_auth::unavailable)?;
        if !bound.same_scope(source) {
            return Err(managed_auth::unavailable());
        }
        let Some(member) = &bound.member else {
            return Ok(());
        };
        let document = bound.read_revision(&source.revision)?;
        let actual = {
            let cached = self.inner.read().map_err(|_| managed_auth::unavailable())?;
            let Some(CodexAuth::Chatgpt(auth)) = &cached.auth else {
                return Err(managed_auth::unavailable());
            };
            if auth.current_token_data() != document.tokens {
                return Err(managed_auth::unavailable());
            }
            *self.auth_change_state_tx.borrow()
        };
        member
            .source_ready(source, binding(source, actual).generations)
            .await?;
        if *self.auth_change_state_tx.borrow() != actual {
            member.seal();
            return Err(managed_auth::unavailable());
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "auth_native_source_tests.rs"]
mod tests;
