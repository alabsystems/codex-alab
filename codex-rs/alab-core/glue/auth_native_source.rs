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

#[derive(Default)]
pub(super) enum State {
    #[default]
    Startup,
    Bound(Arc<Bound>),
    Detached,
}

pub(super) struct Bound {
    source: native_source::Source,
    revision: Mutex<managed_auth::Revision>,
    member: Option<codex_alab_core::native_source_lifetime::Lifetime>,
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
    chatgpt.storage = bound;
    member
        .reader_ready(&source)
        .await
        .map_err(|_| managed_auth::unavailable())?;
    Ok(auth)
}

fn principal(domain: &[u8], value: &str) -> String {
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
        Arc::ptr_eq(&self.domain, &previous.domain)
            && self
                .member
                .as_ref()
                .is_none_or(codex_alab_core::native_source_lifetime::Lifetime::live)
            && self
                .member
                .as_ref()
                .map(codex_alab_core::native_source_lifetime::Lifetime::incarnation)
                == previous.member
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

impl AuthManager {
    pub(super) async fn alab_initialize_member_source(
        &self,
        source: native_source::Source,
        member: codex_alab_core::native_source_lifetime::Lifetime,
    ) -> std::io::Result<AuthChangeState> {
        member.permits_storage(&source)?;
        let bound = Arc::new(Bound {
            revision: Mutex::new(source.revision.clone()),
            source: source.clone(),
            member: Some(member.clone()),
        });
        let candidate = self.alab_load_bound(Arc::clone(&bound), None).await?;
        let actual = {
            let mut selected = self
                .alab_native_source
                .write()
                .map_err(|_| managed_auth::unavailable())?;
            let mut cached = self
                .inner
                .write()
                .map_err(|_| managed_auth::unavailable())?;
            bound.read()?;
            *selected = State::Bound(bound);
            self.set_cached_auth_locked(&mut cached, candidate);
            *self.auth_change_state_tx.borrow()
        };
        member
            .source_ready(&source, binding(&source, actual).generations)
            .await
            .map_err(|_| managed_auth::unavailable())?;
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

    /// Checks this manager's own policy; a process coordinator must visit every registered manager.
    pub fn alab_check_native_target(
        &self,
        target: &codex_alab_core::assignment_service::Target,
    ) -> std::io::Result<()> {
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

    /// Proves the current managed grant before any healthy assignment commit.
    pub async fn alab_native_source_fence(&self) -> std::io::Result<NativeSourceFence> {
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
            domain: Arc::clone(&self.alab_assignment_domain),
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
        if !Arc::ptr_eq(&fence.domain, &self.alab_assignment_domain)
            || !boundary.protects(&self.alab_assignment_domain, operation, Some(operation))
        {
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
        if !Arc::ptr_eq(&fence.domain, &self.alab_assignment_domain)
            || !boundary.protects(
                &self.alab_assignment_domain,
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
        self.set_cached_auth_locked(&mut cached, Some(CodexAuth::Chatgpt(current)));
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
            || !Arc::ptr_eq(&fence.domain, &self.alab_assignment_domain)
            || !boundary.protects(
                &self.alab_assignment_domain,
                boundary.operation(),
                boundary.native_intent(),
            )
        {
            return Err(managed_auth::unavailable());
        }
        let fresh = native_source::source(&format!("{:032x}", rand::random::<u128>())).await?;
        let original = Bound {
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
        self.alab_assignment_domain
            .admit_mutation()
            .map_err(|_| managed_auth::unavailable())
    }
    pub fn alab_account_resource(&self) -> std::io::Result<codex_alab_core::assignment::Activity> {
        self.alab_assignment_domain
            .admit_current_resource()
            .map_err(|_| managed_auth::unavailable())
    }

    pub fn alab_assignment_domain(&self) -> Arc<codex_alab_core::assignment::Domain> {
        Arc::clone(&self.alab_assignment_domain)
    }
    pub(super) fn alab_native_overridden(&self) -> bool {
        self.alab_native_source
            .read()
            .map(|state| !matches!(*state, State::Startup))
            .unwrap_or(true)
    }

    fn alab_bound(&self) -> std::io::Result<Option<Arc<Bound>>> {
        match &*self
            .alab_native_source
            .read()
            .map_err(|_| managed_auth::unavailable())?
        {
            State::Startup => Ok(None),
            State::Bound(bound) => Ok(Some(Arc::clone(bound))),
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
                State::Bound(bound) => Arc::clone(bound),
            }
        };
        Some(self.alab_load_bound(bound, cached).await)
    }

    /// Called by the assignment consumer only with its process barrier retained.
    pub async fn alab_rebind_native_source(
        &self,
        operation: &str,
        expected: AuthChangeState,
        boundary: &codex_alab_core::assignment::BoundaryPermit,
    ) -> std::io::Result<AuthChangeState> {
        if !boundary.protects(
            &self.alab_assignment_domain,
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
        source.validate()?;
        if !boundary.protects(
            &self.alab_assignment_domain,
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

    pub(super) async fn alab_native_reload(&self) -> std::io::Result<bool> {
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
        if self.alab_assignment_domain.phase() != codex_alab_core::assignment::Phase::Open {
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
    ) -> std::io::Result<Option<native_source::Source>> {
        let Some(bound) = self.alab_bound()? else {
            return Ok(None);
        };
        let operation = format!("{:032x}", rand::random::<u128>());
        let current = match &bound.member {
            Some(member) => member.preload(&operation, None).await?.source,
            None => native_source::source(&operation).await?,
        };
        if !bound.same_scope(&current) {
            return Err(managed_auth::unavailable());
        }
        Ok(Some(current))
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
