// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! ALAB(auth-refresh-lock) glue for `codex-login` (`auth::manager::alab_auth_refresh_lock`): the
//! auth manager's ChatGPT refresh holds the lock aterm holds on the login's file
//! (`codex_alab_core::auth_lock`), from a read of the login under it to the write of the answer,
//! and sends only the refresh token the file still holds.
//!
//! The one hook is the first line of `AuthManager::refresh_and_persist_chatgpt_token`, the only
//! place a managed ChatGPT login's refresh token is sent and its answer written; every refresh
//! (proactive, forced, the recovery from a 401) reaches it with the manager's `refresh_lock`
//! held, so this lock nests inside that one and never waits on it.
//!
//! The module also holds the re-read of the login that codex-alab's lost-login watch makes
//! (`AuthManager::alab_reload_if`, `codex_alab_autopilot::login`). Like the refresh's re-read,
//! it takes no other login. Only codex-alab's autopilot calls it; upstream's code does not.

use super::AuthManager;
use super::ChatgptAuth;
use super::CodexAuth;
use super::RefreshTokenError;
use super::same_login_for_persistence;
use crate::auth::storage::auth_storage_commit_guard;
use crate::auth::storage::get_auth_file;
use crate::token_data::TokenData;
use codex_alab_core::auth_lock;
use codex_alab_core::managed_auth;
use codex_config::types::AuthCredentialsStoreMode;
use codex_protocol::auth::AuthMode;
use std::path::PathBuf;

/// What the login's store holds now, against the tokens a refresh was about to send.
enum OnDisk {
    /// The same login with the same refresh token: send it.
    Same,
    /// The same login with another refresh token: another process rotated it.
    Rotated,
    /// Another login, none, or a store that cannot be read: what `persist_tokens` would refuse.
    Refused(std::io::Error),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ManagedGrantRelation {
    Same,
    Distinct,
    Ambiguous,
}

/// Persistence compatibility is asymmetric and cannot prove independent grants.
/// Match the owner's copy/rotation semantics before allowing native refresh.
pub(super) fn managed_grant_relation(left: &TokenData, right: &TokenData) -> ManagedGrantRelation {
    use ManagedGrantRelation::Ambiguous;
    use ManagedGrantRelation::Distinct;
    use ManagedGrantRelation::Same;
    let authority = |tokens: &TokenData| {
        tokens.oauth_provenance.as_ref().map_or_else(
            || {
                Some((
                    "https://auth.openai.com".to_owned(),
                    super::CLIENT_ID.to_owned(),
                ))
            },
            |provenance| {
                (!provenance.issuer.trim().is_empty() && !provenance.client_id.trim().is_empty())
                    .then(|| {
                        (
                            provenance.issuer.trim_end_matches('/').to_owned(),
                            provenance.client_id.clone(),
                        )
                    })
            },
        )
    };
    let Some((left_authority, right_authority)) = authority(left).zip(authority(right)) else {
        return Ambiguous;
    };
    if left_authority != right_authority {
        return Distinct;
    }
    let principal = |tokens: &TokenData| {
        tokens
            .account_id
            .as_deref()
            .filter(|value| !value.is_empty())
            .zip(
                tokens
                    .id_token
                    .chatgpt_user_id
                    .as_deref()
                    .filter(|value| !value.is_empty()),
            )
            .map(|(account, user)| (account.to_owned(), user.to_owned()))
    };
    let Some((left_principal, right_principal)) = principal(left).zip(principal(right)) else {
        return Ambiguous;
    };
    let same_binding = if left_principal == right_principal {
        Same
    } else {
        Ambiguous
    };
    // Exact token copies take precedence over missing or conflicting owner IDs.
    if !left.refresh_token.is_empty() && left.refresh_token == right.refresh_token {
        return same_binding;
    }
    match (
        left.login_owner_id
            .as_deref()
            .filter(|value| !value.is_empty()),
        right
            .login_owner_id
            .as_deref()
            .filter(|value| !value.is_empty()),
    ) {
        (Some(left), Some(right)) if left == right => same_binding,
        (Some(_), Some(_)) => Distinct,
        _ if left_principal.1 != right_principal.1 => Distinct,
        _ => Ambiguous,
    }
}

/// `auth`'s store read now, and judged against `tokens` as `persist_tokens` judges it.
fn on_disk(auth: &ChatgptAuth, tokens: &TokenData) -> OnDisk {
    let stored = match auth_storage_commit_guard() {
        Ok(_commit) => auth.storage().load(),
        Err(err) => Err(err),
    };
    let stored = match stored {
        Ok(Some(stored)) => stored,
        Ok(None) => return OnDisk::Refused(std::io::Error::other("Token data is not available.")),
        Err(err) => return OnDisk::Refused(err),
    };
    match stored.tokens.as_ref() {
        Some(stored_tokens)
            if stored.resolved_mode() == AuthMode::Chatgpt
                && same_login_for_persistence(tokens, stored_tokens) =>
        {
            if stored_tokens.refresh_token == tokens.refresh_token {
                OnDisk::Same
            } else {
                OnDisk::Rotated
            }
        }
        _ => OnDisk::Refused(std::io::Error::other(
            "Account credentials changed during refresh.",
        )),
    }
}

pub(super) struct RefreshHold {
    _file: auth_lock::Hold,
    _activity: codex_alab_core::assignment::Activity,
}

impl AuthManager {
    /// Whether an unchanged-file reconciliation can read only the selected
    /// file store. Match storage's admitted-path transform; a leftover file
    /// alone must never cause an Auto/Keyring lookup or external-host refresh.
    pub fn alab_uses_file_auth(&self) -> bool {
        if self.alab_native_overridden() {
            return true;
        }
        !self.has_external_auth()
            && (self.auth_credentials_store_mode == AuthCredentialsStoreMode::File
                || (codex_alab_core::home::has_admitted_auth_path()
                    && matches!(
                        self.auth_credentials_store_mode,
                        AuthCredentialsStoreMode::Auto | AuthCredentialsStoreMode::Keyring
                    )))
    }

    /// The file this manager keeps its login in, where it keeps it in one and this is codex-alab:
    /// `File` storage; `Keyring` and `Auto` where an admitted `CODEX_AUTH_PATH` pins them to that
    /// file (`ALAB(auth-path)`); `Auto` also where its fallback file or unsettled refresh
    /// journal is there. Other keyring and ephemeral logins keep upstream's ordering.
    ///
    /// It asks [`codex_alab_core::is_alab_binary`], not `active`: in vanilla mode aterm's pool
    /// still hands codex its logins (`CODEX_AUTH_PATH` holds there too) and still refreshes them.
    pub(super) fn alab_login_file(&self) -> Option<PathBuf> {
        if self.alab_native_overridden() {
            return self.alab_native_path().ok().flatten();
        }
        if !codex_alab_core::is_alab_binary() {
            return None;
        }
        let file = get_auth_file(&self.codex_home);
        let pinned = codex_alab_core::home::has_admitted_auth_path();
        let kept = match self.auth_credentials_store_mode {
            AuthCredentialsStoreMode::File => true,
            AuthCredentialsStoreMode::Keyring => pinned,
            AuthCredentialsStoreMode::Auto => {
                pinned
                    || std::fs::symlink_metadata(&file).is_ok()
                    || !matches!(managed_auth::refresh_pending(&file), Ok(false))
            }
            AuthCredentialsStoreMode::Ephemeral => false,
        };
        kept.then_some(file)
    }

    /// Admits a refresh of `auth`, whose `tokens` hold the refresh token it is about to send.
    ///
    /// For a login kept in a file ([`Self::alab_login_file`]) this waits for the lock beside it
    /// ([`auth_lock::admit`]) and reads the login again under it. `Ok(None)` where the file holds
    /// another refresh token for the same login: another process rotated it, this manager has
    /// reloaded what it wrote, and nothing is sent. An error, and no request, where the store
    /// holds another login or none or cannot be read: the error `persist_tokens` gives after the
    /// request upstream sends, but without spending a token whose login may live on in another
    /// file (aterm's pool keeps its logins apart). Otherwise the hold to keep through the request
    /// and `persist_tokens`: unlocked only where no file lock applies. A busy or refused
    /// file lock returns a transient error without sending a single-use token.
    pub(super) async fn alab_admit_refresh(
        &self,
        auth: &ChatgptAuth,
        tokens: &TokenData,
    ) -> Result<Option<RefreshHold>, RefreshTokenError> {
        let activity = self.alab_account_resource()?;
        if let Some(slot) = self.alab_managed_binding(tokens)? {
            self.alab_managed_refresh(&slot, tokens).await?;
            return Ok(None);
        }
        let login_file = self.alab_login_file();
        let ordinary_file = login_file.clone();
        let mut refused = None;
        let refused_slot = &mut refused;
        let mut delegated = None;
        let delegated_slot = &mut delegated;
        let admission =
            auth_lock::admit(login_file.as_deref(), auth_lock::WAIT, move || async move {
                // A host can transfer this file to managed custody while we
                // wait for its lock. Recheck under that same lock before any
                // native send/reload; Reused drops it before owner delegation.
                match self.alab_managed_binding(tokens) {
                    Ok(Some(slot)) => {
                        *delegated_slot = Some(slot);
                        return true;
                    }
                    Err(error) => {
                        *refused_slot = Some(error);
                        return true;
                    }
                    Ok(None) => {}
                }
                match on_disk(auth, tokens) {
                    OnDisk::Same => {
                        if ordinary_file.as_deref().is_some_and(|file| {
                            !matches!(managed_auth::refresh_pending(file), Ok(false))
                        }) {
                            *refused_slot = Some(std::io::Error::other(
                                "A shared credential refresh is unsettled; no token was sent.",
                            ));
                            true
                        } else {
                            false
                        }
                    }
                    OnDisk::Rotated => {
                        tracing::info!(
                            "Skipping token refresh: another process rotated this login."
                        );
                        if let Err(error) = self.alab_reload_rotated_tokens(tokens).await {
                            *refused_slot = Some(error);
                        }
                        true
                    }
                    OnDisk::Refused(err) => {
                        *refused_slot = Some(err);
                        true
                    }
                }
            })
            .await;
        if let Some(slot) = delegated {
            self.alab_managed_refresh(&slot, tokens).await?;
            return Ok(None);
        }
        match admission {
            auth_lock::Admission::Unavailable(err) => Err(RefreshTokenError::from(err)),
            auth_lock::Admission::Send(hold) => Ok(Some(RefreshHold {
                _file: hold,
                _activity: activity,
            })),
            auth_lock::Admission::Reused => match refused {
                Some(err) => Err(RefreshTokenError::from(err)),
                None => Ok(None),
            },
        }
    }

    pub(super) fn alab_managed_binding(
        &self,
        expected: &TokenData,
    ) -> std::io::Result<Option<String>> {
        if let Some(binding) = self.alab_native_binding() {
            return binding;
        }
        let mut file = self
            .alab_uses_file_auth()
            .then(|| get_auth_file(&self.codex_home));
        if file.is_none()
            && codex_alab_core::is_alab_binary()
            && !self.has_external_auth()
            && self.auth_credentials_store_mode == AuthCredentialsStoreMode::Auto
        {
            let candidate = get_auth_file(&self.codex_home);
            if managed_auth::custody_present(&candidate)? {
                let bytes = managed_auth::read_document(&candidate)?;
                let stored: super::AuthDotJson =
                    serde_json::from_slice(&bytes).map_err(|_| managed_auth::unavailable())?;
                // Actual Auto fallback must retain managed custody. A known
                // different keyring login cannot inherit stale file authority;
                // ambiguous legacy same-login copies remain fail-closed.
                if stored.resolved_mode() == AuthMode::Chatgpt {
                    let tokens = stored
                        .tokens
                        .as_ref()
                        .ok_or_else(managed_auth::unavailable)?;
                    match managed_grant_relation(expected, tokens) {
                        ManagedGrantRelation::Same => file = Some(candidate),
                        ManagedGrantRelation::Ambiguous => return Err(managed_auth::unavailable()),
                        ManagedGrantRelation::Distinct => {}
                    }
                }
            }
        }
        managed_auth::binding_for_path(
            codex_alab_core::is_alab_binary(),
            std::env::var_os(managed_auth::SLOT_ENV).as_deref(),
            file.as_deref(),
        )
    }

    /// The managed owner must acquire its own local refresh lock, publish the
    /// encrypted replacement and materialize the native file before replying.
    /// Call it before our flock, never falling through to native token exchange.
    async fn alab_managed_refresh(
        &self,
        slot: &str,
        tokens: &TokenData,
    ) -> Result<(), RefreshTokenError> {
        if self.alab_native_member()?.is_some() {
            let activity = self
                .alab_assignment_domain()
                .admit_current_resource()
                .map_err(|_| managed_auth::unavailable())?;
            return activity
                .scope(self.alab_managed_refresh_owned(slot, tokens))
                .await;
        }
        self.alab_managed_refresh_owned(slot, tokens).await
    }
    async fn alab_managed_refresh_owned(
        &self,
        slot: &str,
        tokens: &TokenData,
    ) -> Result<(), RefreshTokenError> {
        use sha2::Digest as _;
        let (registered, predecessor) = self.alab_native_refresh_preload(None).await?;
        let file = self
            .alab_login_file()
            .ok_or_else(managed_auth::unavailable)?;
        let bytes = managed_auth::read_document(&file)?;
        let stored: super::AuthDotJson =
            serde_json::from_slice(&bytes).map_err(|_| managed_auth::unavailable())?;
        let stored_tokens = stored
            .tokens
            .as_ref()
            .ok_or_else(managed_auth::unavailable)?;
        if stored.resolved_mode() != AuthMode::Chatgpt
            || !same_login_for_persistence(tokens, stored_tokens)
        {
            return Err(managed_auth::unavailable().into());
        }
        let expected_digest = format!("{:x}", sha2::Sha256::digest(bytes.as_slice()));
        drop(bytes);
        if stored_tokens != tokens {
            if let Some(source) = &registered {
                self.alab_native_note_revision(slot, &file, &source.revision)?;
            }
            self.alab_reload_managed_tokens(stored_tokens).await?;
            self.alab_native_refresh_ready(registered.as_ref()).await?;
            return Ok(());
        }
        let revision = self
            .alab_native_refresh_owner(slot, &expected_digest)
            .await?;
        // An open owned PRELOAD replays its original packet. Resample the exact
        // predecessor after owner publication; neither a new operation string
        // nor the owner's revision alone grants permission to read newer bytes.
        let (registered, _) = self.alab_native_refresh_preload(predecessor).await?;
        let current_revision = registered
            .as_ref()
            .map_or(&revision, |source| &source.revision);
        if current_revision.generation < revision.generation
            || (current_revision.generation == revision.generation
                && current_revision.digest != revision.digest)
        {
            return Err(managed_auth::unavailable().into());
        }
        let hold = match auth_lock::admit(Some(&file), auth_lock::WAIT, || async { false }).await {
            auth_lock::Admission::Send(hold) => hold,
            _ => return Err(managed_auth::unavailable().into()),
        };
        let current = managed_auth::read_document(&file)?;
        if format!("{:x}", sha2::Sha256::digest(current.as_slice())) != current_revision.digest {
            return Err(managed_auth::unavailable().into());
        }
        let verified: super::AuthDotJson =
            serde_json::from_slice(&current).map_err(|_| managed_auth::unavailable())?;
        let verified_tokens = verified
            .tokens
            .as_ref()
            .ok_or_else(managed_auth::unavailable)?;
        if verified.resolved_mode() != AuthMode::Chatgpt
            || !same_login_for_persistence(tokens, verified_tokens)
        {
            return Err(managed_auth::unavailable().into());
        }
        drop(current);
        self.alab_native_note_revision(slot, &file, current_revision)?;
        self.alab_reload_managed_tokens(verified_tokens).await?;
        drop(hold);
        self.alab_native_refresh_ready(registered.as_ref()).await?;
        Ok(())
    }

    /// Validate the actual loaded candidate, rather than a prior file snapshot.
    /// A competing sign-in or read failure cannot switch/clear this cached login.
    async fn alab_reload_managed_tokens(
        &self,
        expected: &TokenData,
    ) -> Result<(), RefreshTokenError> {
        self.alab_reload_if(|candidate| {
            matches!(candidate, CodexAuth::Chatgpt(candidate)
                if candidate.current_token_data().as_ref() == Some(expected))
        })
        .await
        .ok_or_else(managed_auth::unavailable)?;
        Ok(())
    }

    /// Ordinary shared-file refresh also publishes only the same provider login.
    /// A newer rotation is welcome; another sign-in or failed read keeps the cache.
    async fn alab_reload_rotated_tokens(&self, expected: &TokenData) -> std::io::Result<()> {
        self.alab_reload_if(|candidate| {
            matches!(candidate, CodexAuth::Chatgpt(candidate)
                if candidate.current_token_data().as_ref().is_some_and(|loaded|
                    same_login_for_persistence(expected, loaded)))
        })
        .await
        .ok_or_else(|| {
            std::io::Error::other(
                "Account credentials changed during refresh or could not be read.",
            )
        })?;
        Ok(())
    }

    /// RELOADS THE LOGIN, TAKING ONLY THE WALLED ONE (codex-alab's lost-login watch,
    /// `codex_alab_autopilot::login`). This is [`Self::reload`], except that it publishes the
    /// login the store holds only where `walled` takes it for the login a wall refused. The 401
    /// recovery's reload works the same way: it publishes only the expected account's login
    /// (`reload_if_account_id_matches`). Another login, or none, stays in the store, and this
    /// process keeps the login it holds. A failed storage read also keeps the cached login
    /// and never reaches the caller's login predicate.
    ///
    /// Returns `Some` once the login was taken, with whether the cached login changed, and
    /// `None` where it was not taken. The log says neither login.
    pub async fn alab_reload_if(&self, walled: impl Fn(&CodexAuth) -> bool) -> Option<bool> {
        loop {
            let generation = self.auth_change_state_tx.borrow().generation;
            let new_auth = self.load_auth(/*cached_auth*/ None).await.ok()?;
            if !new_auth.as_ref().is_some_and(&walled) {
                tracing::info!("Skipping auth reload: the store holds another login, or none.");
                return None;
            }
            tracing::info!("Reloading auth for the walled login");
            if let Some(publication) = self.publish_reloaded_auth(generation, new_auth) {
                return Some(publication.changed_for_refresh);
            }
            if self.inner.is_poisoned() {
                return None;
            }
        }
    }
}

// `AuthLock::try_take`, which these tests hold the lock with, exists on unix only.
#[cfg(all(test, unix))]
mod tests {
    //! The real refresh path (`refresh_token_from_authority`) against a fake token endpoint
    //! (wiremock, named by the login's recorded issuer) while the test holds the login's lock
    //! through an open of its own, as aterm would. The endpoint judges each request by the lock
    //! as it stands when the request arrives (`LockWitness`). Only privately spawned test
    //! children activate ALab's switch or set `CODEX_AUTH_PATH`; the shared test runner keeps
    //! upstream's behavior.

    use super::super::AuthManager;
    use super::super::CodexAuth;
    use super::super::save_auth;
    use super::RefreshTokenError;
    use crate::AuthDotJson;
    use crate::AuthKeyringBackendKind;
    use crate::auth::storage::get_auth_file;
    use crate::token_data::AccountOAuthProvenance;
    use crate::token_data::TokenData;
    use crate::token_data::parse_chatgpt_jwt_claims;
    use anyhow::Context as _;
    use base64::Engine as _;
    use codex_alab_core::auth_lock;
    use codex_alab_core::auth_lock::AuthLock;
    use codex_alab_core::managed_auth;
    use codex_config::types::AuthCredentialsStoreMode;
    use codex_protocol::auth::AuthMode;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use std::path::Path;
    use std::path::PathBuf;
    use std::process::Command;
    use std::process::Stdio;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::PoisonError;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;
    use std::time::Duration;
    use tokio::sync::Notify;
    use wiremock::Mock;
    use wiremock::MockServer;
    use wiremock::Request;
    use wiremock::Respond;
    use wiremock::ResponseTemplate;
    use wiremock::matchers::method;
    use wiremock::matchers::path;

    const CHILD_CASE_ENV: &str = "ALAB_AUTH_REFRESH_LOCK_CHILD_CASE";
    const CHILD_COMPLETE: &str = "ALAB_AUTH_REFRESH_LOCK_CHILD_OK";
    /// A hang detector for what must happen: one minute.
    const HANG: Duration = Duration::from_secs(60);
    /// How long the test keeps the lock before it lets go, so that the refresh waits for it. It
    /// is no detection window: the endpoint judges each request by the lock as it stands then
    /// (`LockWitness`), whenever the request comes.
    const HELD: Duration = Duration::from_millis(500);

    /// How the login's lock stood when a token request reached the endpoint.
    #[derive(Clone, Debug, PartialEq, Eq)]
    enum AtRequest {
        /// aterm (the test) still held it: the refresh was sent past aterm's lock.
        AtermHeld,
        /// aterm had let go and another open held it: the refresh kept its own lock through its
        /// request, while the inner commit lock remained free.
        CodexHeld,
        /// The outer hold stood, but the inner commit lock was held across the request.
        CommitHeld,
        /// No one held it: the refresh was sent without the lock.
        Free,
        /// The witness could not open or lock the lock file.
        Unknown(String),
    }

    /// The token endpoint. It answers every refresh with the same new tokens, and first records
    /// how the login's lock stands, judged under the mutex aterm lets go under, so a request is
    /// judged against one state of aterm's hold.
    struct LockWitness {
        login_file: PathBuf,
        /// aterm's hold on the login's lock; `None` once aterm has let go.
        aterm: Arc<Mutex<Option<AuthLock>>>,
        seen: Arc<Mutex<Vec<AtRequest>>>,
        arrived: Arc<Notify>,
    }

    impl Respond for LockWitness {
        fn respond(&self, _request: &Request) -> ResponseTemplate {
            let at_request = {
                let aterm = self.aterm.lock().unwrap_or_else(PoisonError::into_inner);
                if aterm.is_some() {
                    AtRequest::AtermHeld
                } else {
                    match AuthLock::try_take(&self.login_file) {
                        Ok(None) => {
                            let commit = std::fs::OpenOptions::new().read(true).write(true).open(
                                codex_alab_core::home::auth_commit_lock_path(&self.login_file),
                            );
                            match commit
                                .and_then(|file| file.try_lock().map_err(std::io::Error::from))
                            {
                                Ok(()) => AtRequest::CodexHeld,
                                Err(_) => AtRequest::CommitHeld,
                            }
                        }
                        Ok(Some(_free)) => AtRequest::Free,
                        Err(err) => AtRequest::Unknown(err.to_string()),
                    }
                }
            };
            self.seen
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(at_request);
            self.arrived.notify_one();
            ResponseTemplate::new(200).set_body_json(json!({
                "access_token": "access-from-codex",
                "refresh_token": "refresh-from-codex",
            }))
        }
    }

    fn spawn_case(case: &str, with_auth_path: bool) -> std::io::Result<()> {
        let (_, local_mod) = module_path!().split_once("::").ok_or_else(|| {
            std::io::Error::other("expected nested module path for auth_refresh_lock tests")
        })?;
        let exact = format!("{local_mod}::auth_refresh_lock_child");
        let world = tempfile::Builder::new()
            .prefix("alab-auth-refresh-lock-")
            .tempdir()?;
        let codex_home = world.path().join("codex-home");
        std::fs::create_dir_all(&codex_home)?;
        let pool_auth = world.path().join("pool").join("acct-b").join("auth.json");

        let mut cmd = Command::new(std::env::current_exe()?);
        cmd.args(["--exact", &exact, "--nocapture", "--test-threads=1"])
            .env_clear()
            .env(CHILD_CASE_ENV, case)
            .env("HOME", world.path())
            .env("CODEX_HOME", &codex_home)
            .env("TMPDIR", world.path())
            .env("USER", "alab-test")
            .env("LOGNAME", "alab-test")
            .env("PATH", "/usr/bin:/bin")
            .current_dir(world.path())
            .stdin(Stdio::null());
        if with_auth_path {
            cmd.env(codex_alab_core::home::AUTH_PATH_ENV, &pool_auth);
        }
        let output = cmd.output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "auth refresh lock child ({case}) failed:\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
        assert!(
            stdout.contains(CHILD_COMPLETE),
            "auth refresh lock child ({case}) did not complete:\n{stdout}"
        );
        Ok(())
    }

    #[test]
    fn the_refresh_holds_the_login_lock_and_sends_only_the_token_the_file_holds()
    -> std::io::Result<()> {
        assert!(!codex_alab_core::is_alab_binary());
        spawn_case("held", /*with_auth_path*/ true)?;
        spawn_case("rotated", /*with_auth_path*/ false)?;
        spawn_case("replaced", /*with_auth_path*/ true)?;
        spawn_case("refused-lock", /*with_auth_path*/ true)?;
        assert!(!codex_alab_core::is_alab_binary());
        Ok(())
    }

    #[test]
    fn the_lost_login_watch_keeps_its_owner_until_the_expected_login_can_be_read()
    -> std::io::Result<()> {
        spawn_case("reload-watch", /*with_auth_path*/ true)
    }

    #[test]
    fn ordinary_rotated_reload_accepts_only_the_same_login_and_keeps_cache_on_read_failure()
    -> std::io::Result<()> {
        spawn_case("ordinary-reload-race", /*with_auth_path*/ true)
    }

    #[test]
    fn ordinary_file_refresh_keeps_an_unsettled_host_journal_without_spending_its_token()
    -> std::io::Result<()> {
        spawn_case("ordinary-journal-pending", /*with_auth_path*/ true)?;
        spawn_case("ordinary-journal-rotated", /*with_auth_path*/ true)
    }

    #[test]
    fn auto_file_fallback_keeps_journal_custody_without_reading_the_os_keychain()
    -> std::io::Result<()> {
        spawn_case("ordinary-auto-journal", /*with_auth_path*/ false)
    }

    #[test]
    fn effective_auto_file_fallback_preserves_managed_marker_and_pending_custody()
    -> std::io::Result<()> {
        spawn_case("managed-auto-effective", /*with_auth_path*/ false)?;
        spawn_case(
            "managed-auto-effective-pending",
            /*with_auth_path*/ false,
        )
    }

    #[test]
    fn auto_keyring_copies_cannot_escape_managed_or_uncertain_refresh_custody()
    -> std::io::Result<()> {
        for case in [
            "managed-auto-keyring-legacy",
            "managed-auto-keyring-conflicting-owner",
            "managed-auto-keyring-legacy-rotated",
            "managed-auto-keyring-conflicting-principal",
            "managed-auto-keyring-journal",
            "managed-auto-keyring-journal-replaced",
            "managed-auto-keyring-journal-missing-tokens",
            "managed-auto-keyring-journal-missing-file",
            "managed-auto-keyring-distinct",
        ] {
            spawn_case(case, /*with_auth_path*/ false)?;
        }
        Ok(())
    }

    #[test]
    fn a_marked_slot_uses_its_owner_even_with_the_interface_off_and_never_falls_back()
    -> std::io::Result<()> {
        for case in [
            "managed-ready",
            "managed-failed",
            "managed-missing",
            "managed-malformed",
            "managed-reload-race",
            "managed-handoff",
            "managed-handoff-malformed",
            "managed-handoff-pending",
        ] {
            spawn_case(case, /*with_auth_path*/ true)?;
        }
        Ok(())
    }

    #[test]
    fn default_file_markers_delegate_but_unpinned_auto_and_keyring_markers_do_not()
    -> std::io::Result<()> {
        for case in [
            "managed-default",
            "managed-auto-unpinned",
            "managed-keyring-unpinned",
        ] {
            spawn_case(case, /*with_auth_path*/ false)?;
        }
        Ok(())
    }

    struct ManagedOwner {
        file: PathBuf,
        fail: bool,
        calls: std::sync::atomic::AtomicUsize,
    }

    /// Deterministic keyring selection without touching the operating system's
    /// credential service. Its Debug never renders the native document.
    struct FixtureKeyring(AuthDotJson);

    impl std::fmt::Debug for FixtureKeyring {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("FixtureKeyring")
        }
    }

    impl crate::auth::storage::AuthStorageBackend for FixtureKeyring {
        fn load(&self) -> std::io::Result<Option<AuthDotJson>> {
            Ok(Some(self.0.clone()))
        }

        fn save(&self, _: &AuthDotJson) -> std::io::Result<()> {
            Err(std::io::Error::other("fixture keyring is read-only"))
        }

        fn delete(&self) -> std::io::Result<bool> {
            Err(std::io::Error::other("fixture keyring is read-only"))
        }
    }

    async fn copied_keyring_case(
        case: &str,
        file_manager: &AuthManager,
        login_file: &Path,
        server: &MockServer,
    ) -> anyhow::Result<()> {
        let mut selected = file_manager.auth_cached().context("fixture auth")?;
        let CodexAuth::Chatgpt(auth) = &mut selected else {
            anyhow::bail!("fixture must be Chatgpt")
        };
        let mut document = auth.storage.load()?.context("fixture document")?;
        let attempted = document.tokens.as_mut().context("fixture tokens")?;
        attempted.login_owner_id = if matches!(
            case,
            "managed-auto-keyring-conflicting-owner" | "managed-auto-keyring-distinct"
        ) {
            Some("different-login-owner".to_owned())
        } else {
            None
        };
        if matches!(
            case,
            "managed-auto-keyring-legacy-rotated" | "managed-auto-keyring-distinct"
        ) {
            attempted.refresh_token = "older-keyring-token".to_owned();
        }
        if case == "managed-auto-keyring-conflicting-principal" {
            attempted.account_id = Some("different-workspace".to_owned());
        }
        let expected = attempted.clone();
        auth.state.auth_dot_json = Arc::new(Mutex::new(Some(document.clone())));
        auth.storage = Arc::new(FixtureKeyring(document));
        let manager = super::super::alab_manager_catalog::fixture::configured(
            |manager| manager.auth_credentials_store_mode = AuthCredentialsStoreMode::Auto,
            || {
                AuthManager::from_auth_for_testing_with_home(
                    selected,
                    file_manager.codex_home.clone(),
                )
            },
        );
        let cached = manager.auth_cached().context("keyring-selected fixture")?;
        let CodexAuth::Chatgpt(auth) = cached else {
            anyhow::bail!("fixture must be Chatgpt")
        };
        let metadata = if case.contains("-journal") {
            managed_auth::refresh_journal_path(login_file)
        } else {
            managed_auth::marker_path(login_file)
        };
        codex_alab_core::home::save_auth_bytes(
            &metadata,
            format!(
                "{}736c6f742f66697874757265\n",
                managed_auth::SLOT_MARKER_PREFIX
            )
            .as_bytes(),
        )?;
        if case == "managed-auto-keyring-journal-replaced" {
            // Native sign-in does not take the refresh flock or erase an
            // earlier journal. Its new file cannot prove the journal's grant.
            save(
                &manager.codex_home,
                tokens(server, "other-user", "other-account", "new-native-grant")?,
            )?;
        } else if case == "managed-auto-keyring-journal-missing-tokens" {
            codex_alab_core::home::save_auth_bytes(
                login_file,
                br#"{"auth_mode":"chatgpt","tokens":null}"#,
            )?;
        } else if case == "managed-auto-keyring-journal-missing-file" {
            std::fs::remove_file(login_file)?;
        }
        if case == "managed-auto-keyring-distinct" {
            assert!(
                manager
                    .alab_admit_refresh(&auth, &expected)
                    .await?
                    .is_some()
            );
        } else {
            assert!(matches!(
                manager.refresh_token_from_authority().await,
                Err(RefreshTokenError::Transient(_))
            ));
        }
        assert_eq!(
            manager
                .auth_cached()
                .context("retained keyring fixture")?
                .get_token_data()?,
            expected
        );
        assert!(
            server
                .received_requests()
                .await
                .unwrap_or_default()
                .is_empty()
        );
        Ok(())
    }

    impl managed_auth::Host for ManagedOwner {
        fn refresh<'a>(
            &'a self,
            request: &'a managed_auth::Request,
        ) -> managed_auth::RefreshFuture<'a> {
            Box::pin(async move {
                use sha2::Digest as _;
                self.calls.fetch_add(1, Ordering::SeqCst);
                let _hold = AuthLock::try_take(&self.file)?
                    .context("Codex must not hold the refresh lock before calling its owner")
                    .map_err(std::io::Error::other)?;
                let original = managed_auth::read_document(&self.file)?;
                assert_eq!(
                    request.expected_digest,
                    format!("{:x}", sha2::Sha256::digest(original.as_slice()))
                );
                if self.fail {
                    return Err(std::io::Error::other("secret-fixture-owner-error"));
                }
                let mut document: serde_json::Value = serde_json::from_slice(&original)?;
                document["tokens"]["access_token"] = "access-from-owner".into();
                document["tokens"]["refresh_token"] = "refresh-from-owner".into();
                let replacement = serde_json::to_vec(&document)?;
                codex_alab_core::home::save_auth_bytes(&self.file, &replacement)?;
                Ok(managed_auth::Revision {
                    generation: 2,
                    digest: format!("{:x}", sha2::Sha256::digest(&replacement)),
                })
            })
        }
    }

    async fn managed_case(
        case: &str,
        manager: &AuthManager,
        login_file: &Path,
        server: &MockServer,
    ) -> anyhow::Result<()> {
        use std::future::Future as _;
        use std::io::Write as _;
        use std::os::unix::fs::OpenOptionsExt as _;
        codex_alab_core::set_enhanced(false);
        assert!(!codex_alab_core::active());
        let slot = "736c6f742f66697874757265";
        let handoff = case.starts_with("managed-handoff");
        let marker = if matches!(case, "managed-malformed" | "managed-handoff-malformed") {
            "bad marker\n".into()
        } else {
            format!("{}{}\n", managed_auth::SLOT_MARKER_PREFIX, slot)
        };
        if !handoff {
            let mut marker_file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(if case == "managed-auto-effective-pending" {
                    managed_auth::state_path(login_file)
                } else {
                    managed_auth::marker_path(login_file)
                })?;
            marker_file.write_all(marker.as_bytes())?;
        }
        let mut original: serde_json::Value = serde_json::from_slice(&std::fs::read(login_file)?)?;
        original["future_native_field"] = serde_json::json!({"preserve": true});
        let original = serde_json::to_vec(&original)?;
        codex_alab_core::home::save_auth_bytes(login_file, &original)?;
        let owner = Arc::new(ManagedOwner {
            file: login_file.to_owned(),
            fail: case == "managed-failed",
            calls: std::sync::atomic::AtomicUsize::new(0),
        });
        if case != "managed-missing" {
            managed_auth::install(owner.clone());
        }
        if case == "managed-reload-race" {
            let expected = tokens(server, "user-1", "account-1", "observed-rotation")?;
            let before = manager
                .auth_cached()
                .context("cached auth")?
                .get_token_data()?;
            let generation = manager.auth_change_state_tx.borrow().generation;
            // A second writer replaces the file after the expected document was
            // observed but before the production reload publishes its candidate.
            save(
                &manager.codex_home,
                tokens(server, "other-user", "other-account", "other-grant")?,
            )?;
            assert!(manager.alab_reload_managed_tokens(&expected).await.is_err());
            assert_eq!(
                manager
                    .auth_cached()
                    .context("original cache")?
                    .get_token_data()?,
                before
            );
            std::fs::write(login_file, b"unreadable-json")?;
            assert!(manager.alab_reload_managed_tokens(&expected).await.is_err());
            assert_eq!(
                manager
                    .auth_cached()
                    .context("retained cache")?
                    .get_token_data()?,
                before
            );
            assert_eq!(manager.auth_change_state_tx.borrow().generation, generation);
            assert!(
                server
                    .received_requests()
                    .await
                    .unwrap_or_default()
                    .is_empty()
            );
            return Ok(());
        }
        if let Some(mode) = match case {
            "managed-auto-unpinned" => Some(AuthCredentialsStoreMode::Auto),
            "managed-keyring-unpinned" => Some(AuthCredentialsStoreMode::Keyring),
            _ => None,
        } {
            // Retain the fixture's file-backed ChatgptAuth while testing the
            // manager's storage-selection guard. Never touch the OS keychain.
            let selected = manager.auth_cached().context("fixture auth")?;
            let unpinned = super::super::alab_manager_catalog::fixture::configured(
                move |manager| manager.auth_credentials_store_mode = mode,
                || {
                    AuthManager::from_auth_for_testing_with_home(
                        selected,
                        manager.codex_home.clone(),
                    )
                },
            );
            let cached = unpinned.auth_cached().context("cached fixture auth")?;
            let CodexAuth::Chatgpt(auth) = cached else {
                anyhow::bail!("fixture must be Chatgpt")
            };
            let attempted = auth.current_token_data().context("fixture tokens")?;
            if mode == AuthCredentialsStoreMode::Auto {
                // This fixture checks unrelated stale-file selection only;
                // real Auto fallback uses an isolated mock keyring below.
                save(
                    &manager.codex_home,
                    tokens(server, "unrelated-user", "unrelated-account", "stale-grant")?,
                )?;
                assert!(unpinned.alab_managed_binding(&attempted)?.is_none());
                assert_eq!(owner.calls.load(Ordering::SeqCst), 0);
                assert!(
                    server
                        .received_requests()
                        .await
                        .unwrap_or_default()
                        .is_empty()
                );
                return Ok(());
            }
            let native = unpinned.alab_admit_refresh(&auth, &attempted).await?;
            assert!(
                native.is_some(),
                "stale file marker must not select managed auth"
            );
            assert_eq!(owner.calls.load(Ordering::SeqCst), 0);
            assert!(
                server
                    .received_requests()
                    .await
                    .unwrap_or_default()
                    .is_empty()
            );
            return Ok(());
        }
        let result = if handoff {
            let hold = AuthLock::try_take(login_file)?.context("transfer lock")?;
            let mut refresh = Box::pin(manager.refresh_token_from_authority());
            std::future::poll_fn(|context| {
                assert!(refresh.as_mut().poll(context).is_pending());
                std::task::Poll::Ready(())
            })
            .await;
            let destination = if case == "managed-handoff-pending" {
                managed_auth::state_path(login_file)
            } else {
                managed_auth::marker_path(login_file)
            };
            codex_alab_core::home::save_auth_bytes(&destination, marker.as_bytes())?;
            drop(hold);
            refresh.await
        } else {
            manager.refresh_token_from_authority().await
        };
        if matches!(
            case,
            "managed-ready" | "managed-default" | "managed-handoff" | "managed-auto-effective"
        ) {
            result?;
            assert_eq!(owner.calls.load(Ordering::SeqCst), 1);
            assert_eq!(stored_refresh_token(login_file)?, "refresh-from-owner");
            assert_eq!(
                manager
                    .auth_cached()
                    .context("cached auth")?
                    .get_token_data()?
                    .refresh_token,
                "refresh-from-owner"
            );
            let current: serde_json::Value = serde_json::from_slice(&std::fs::read(login_file)?)?;
            assert_eq!(
                current["future_native_field"],
                serde_json::json!({"preserve": true})
            );
            let old: serde_json::Value = serde_json::from_slice(&original)?;
            assert_eq!(
                current["tokens"]["oauth_provenance"],
                old["tokens"]["oauth_provenance"]
            );
            assert_eq!(
                current["tokens"]["login_owner_id"],
                old["tokens"]["login_owner_id"]
            );
        } else {
            let error = result.err().context("managed refresh must fail closed")?;
            assert!(matches!(error, RefreshTokenError::Transient(_)));
            assert!(!error.to_string().contains("secret-fixture-owner-error"));
            assert_eq!(std::fs::read(login_file)?, original);
            assert_eq!(
                owner.calls.load(Ordering::SeqCst),
                usize::from(case == "managed-failed")
            );
        }
        assert!(
            server
                .received_requests()
                .await
                .unwrap_or_default()
                .is_empty(),
            "Codex sent a provider request instead of using its owner"
        );
        Ok(())
    }

    fn jwt(payload: serde_json::Value) -> String {
        let b64 = |bytes: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
        format!(
            "{}.{}.sig",
            b64(br#"{"alg":"none","typ":"JWT"}"#),
            b64(payload.to_string().as_bytes())
        )
    }

    /// A ChatGPT login of `user` in `account`, issued by `server`, holding `refresh_token`.
    fn tokens(
        server: &MockServer,
        user: &str,
        account: &str,
        refresh_token: &str,
    ) -> anyhow::Result<TokenData> {
        Ok(TokenData {
            id_token: parse_chatgpt_jwt_claims(&jwt(json!({
                "sub": user,
                "email": format!("{user}@example.com"),
                "https://api.openai.com/auth": {
                    "chatgpt_user_id": user,
                    "chatgpt_account_id": account,
                    "chatgpt_plan_type": "pro",
                },
            })))?,
            access_token: format!("access-for-{refresh_token}"),
            refresh_token: refresh_token.to_owned(),
            oauth_provenance: Some(AccountOAuthProvenance {
                issuer: server.uri(),
                client_id: "codex-client".to_owned(),
            }),
            login_owner_id: Some(format!("login-of-{user}")),
            account_id: Some(account.to_owned()),
        })
    }

    fn save(codex_home: &Path, tokens: TokenData) -> std::io::Result<()> {
        save_auth(
            codex_home,
            &AuthDotJson {
                auth_mode: Some(AuthMode::Chatgpt),
                openai_api_key: None,
                tokens: Some(tokens),
                last_refresh: Some(chrono::Utc::now()),
                agent_identity: None,
                personal_access_token: None,
                bedrock_api_key: None,
                bedrock_access_keys: None,
            },
            AuthCredentialsStoreMode::File,
            AuthKeyringBackendKind::default(),
        )
    }

    fn stored_refresh_token(login_file: &Path) -> anyhow::Result<String> {
        let doc: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(login_file)?)?;
        doc["tokens"]["refresh_token"]
            .as_str()
            .map(str::to_owned)
            .context("no refresh token stored")
    }

    async fn sent_refresh_tokens(server: &MockServer) -> anyhow::Result<Vec<String>> {
        let mut sent = Vec::new();
        for request in server.received_requests().await.unwrap_or_default() {
            let body: serde_json::Value = serde_json::from_slice(&request.body)?;
            sent.push(
                body["refresh_token"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
            );
        }
        Ok(sent)
    }

    #[tokio::test]
    #[expect(
        clippy::print_stdout,
        reason = "exact completion marker for the parent test process"
    )]
    async fn auth_refresh_lock_child() -> anyhow::Result<()> {
        let Ok(case) = std::env::var(CHILD_CASE_ENV) else {
            return Ok(());
        };
        codex_alab_core::activate();
        let codex_home = PathBuf::from(std::env::var_os("CODEX_HOME").context("CODEX_HOME")?);
        let login_file = get_auth_file(&codex_home);
        if !case.starts_with("managed-auto-keyring-")
            && !matches!(
                case.as_str(),
                "rotated"
                    | "managed-default"
                    | "managed-auto-unpinned"
                    | "managed-keyring-unpinned"
                    | "ordinary-auto-journal"
                    | "managed-auto-effective"
                    | "managed-auto-effective-pending"
            )
        {
            assert_eq!(
                std::env::var_os(codex_alab_core::home::AUTH_PATH_ENV).map(PathBuf::from),
                Some(login_file.clone())
            );
        }

        let server = MockServer::start().await;
        save(
            &codex_home,
            tokens(&server, "user-1", "account-1", "refresh-1")?,
        )?;
        let effective_auto = case.starts_with("managed-auto-effective");
        if effective_auto {
            keyring::set_default_credential_builder(keyring::mock::default_credential_builder());
        }
        let manager = AuthManager::shared(
            codex_home.clone(),
            /*enable_codex_api_key_env*/ false,
            if effective_auto {
                AuthCredentialsStoreMode::Auto
            } else {
                AuthCredentialsStoreMode::File
            },
            /*forced_chatgpt_workspace_id*/ None,
            /*chatgpt_base_url*/ None,
            if effective_auto {
                AuthKeyringBackendKind::Direct
            } else {
                AuthKeyringBackendKind::default()
            },
            crate::test_support::transport_default_auth_route_config(),
        )
        .await;

        if case.starts_with("managed-auto-keyring-") {
            copied_keyring_case(&case, &manager, &login_file, &server).await?;
            println!("{CHILD_COMPLETE}");
            return Ok(());
        }

        if case == "ordinary-auto-journal" {
            keyring::set_default_credential_builder(keyring::mock::default_credential_builder());
            let auto = AuthManager::shared(
                codex_home.clone(),
                false,
                AuthCredentialsStoreMode::Auto,
                None,
                None,
                AuthKeyringBackendKind::Direct,
                crate::test_support::transport_default_auth_route_config(),
            )
            .await;
            let expected = auto
                .auth_cached()
                .context("Auto selected the native file fallback")?
                .get_token_data()?;
            assert_eq!(expected.refresh_token, "refresh-1");
            codex_alab_core::home::save_auth_bytes(
                &managed_auth::refresh_journal_path(&login_file),
                b"pending fixture refresh",
            )?;
            assert!(matches!(
                auto.refresh_token_from_authority().await,
                Err(RefreshTokenError::Transient(_))
            ));
            assert_eq!(
                auto.auth_cached()
                    .context("fallback retained")?
                    .get_token_data()?,
                expected
            );
            assert!(managed_auth::refresh_pending(&login_file)?);
            save(
                &codex_home,
                tokens(
                    &server,
                    "other-user",
                    "other-account",
                    "unrelated-stale-token",
                )?,
            )?;
            assert!(matches!(
                auto.refresh_token_from_authority().await,
                Err(RefreshTokenError::Transient(_))
            ));
            assert!(managed_auth::refresh_pending(&login_file)?);
            assert!(
                server
                    .received_requests()
                    .await
                    .unwrap_or_default()
                    .is_empty()
            );
            println!("{CHILD_COMPLETE}");
            return Ok(());
        }

        if matches!(
            case.as_str(),
            "ordinary-journal-pending" | "ordinary-journal-rotated"
        ) {
            codex_alab_core::home::save_auth_bytes(
                &managed_auth::refresh_journal_path(&login_file),
                b"unknown pending refresh",
            )?;
            if case == "ordinary-journal-rotated" {
                save(
                    &codex_home,
                    tokens(&server, "user-1", "account-1", "already-published")?,
                )?;
            }
            let result = manager.refresh_token_from_authority().await;
            if case == "ordinary-journal-rotated" {
                result?;
                assert_eq!(
                    manager
                        .auth_cached()
                        .context("updated cache")?
                        .get_token_data()?
                        .refresh_token,
                    "already-published"
                );
            } else {
                assert!(matches!(result, Err(RefreshTokenError::Transient(_))));
                assert_eq!(
                    manager
                        .auth_cached()
                        .context("original cache")?
                        .get_token_data()?
                        .refresh_token,
                    "refresh-1"
                );
            }
            assert!(managed_auth::refresh_pending(&login_file)?);
            assert!(
                server
                    .received_requests()
                    .await
                    .unwrap_or_default()
                    .is_empty()
            );
            println!("{CHILD_COMPLETE}");
            return Ok(());
        }

        if case == "ordinary-reload-race" {
            let expected = manager
                .auth_cached()
                .context("original login")?
                .get_token_data()?;
            save(
                &codex_home,
                tokens(&server, "user-1", "account-1", "newer-same-login")?,
            )?;
            manager.alab_reload_rotated_tokens(&expected).await?;
            let accepted = manager
                .auth_cached()
                .context("same login accepted")?
                .get_token_data()?;
            assert_eq!(accepted.refresh_token, "newer-same-login");
            let generation = manager.auth_change_state_tx.borrow().generation;
            save(
                &codex_home,
                tokens(&server, "other-user", "other-account", "other-login")?,
            )?;
            assert!(manager.alab_reload_rotated_tokens(&expected).await.is_err());
            assert_eq!(
                manager
                    .auth_cached()
                    .context("same login retained")?
                    .get_token_data()?,
                accepted
            );
            std::fs::write(&login_file, b"invalid-json")?;
            assert!(manager.alab_reload_rotated_tokens(&expected).await.is_err());
            assert_eq!(
                manager
                    .auth_cached()
                    .context("cache not cleared")?
                    .get_token_data()?,
                accepted
            );
            assert_eq!(manager.auth_change_state_tx.borrow().generation, generation);
            assert!(
                server
                    .received_requests()
                    .await
                    .unwrap_or_default()
                    .is_empty()
            );
            println!("{CHILD_COMPLETE}");
            return Ok(());
        }

        if case.starts_with("managed-") {
            managed_case(&case, &manager, &login_file, &server).await?;
            println!("{CHILD_COMPLETE}");
            return Ok(());
        }

        if case == "reload-watch" {
            let generation = manager.auth_change_state_tx.borrow().generation;
            let cached_refresh = || {
                manager
                    .auth_cached()
                    .context("no cached auth")?
                    .get_token_data()
                    .map(|tokens| tokens.refresh_token)
                    .map_err(anyhow::Error::from)
            };
            std::fs::write(&login_file, b"invalid-json")?;
            assert_eq!(
                manager
                    .alab_reload_if(|_| panic!("an unreadable login reached the predicate"))
                    .await,
                None
            );
            assert_eq!(cached_refresh()?, "refresh-1");
            assert_eq!(manager.auth_change_state_tx.borrow().generation, generation);
            std::fs::remove_file(&login_file)?;
            assert_eq!(
                manager
                    .alab_reload_if(|_| panic!("a missing login reached the predicate"))
                    .await,
                None
            );
            assert_eq!(cached_refresh()?, "refresh-1");
            assert_eq!(manager.auth_change_state_tx.borrow().generation, generation);

            let expected = |auth: &CodexAuth| auth.get_account_id().as_deref() == Some("account-1");
            save(
                &codex_home,
                tokens(&server, "user-2", "account-2", "another-login")?,
            )?;
            assert_eq!(manager.alab_reload_if(expected).await, None);
            assert_eq!(cached_refresh()?, "refresh-1");
            assert_eq!(manager.auth_change_state_tx.borrow().generation, generation);

            save(
                &codex_home,
                tokens(&server, "user-1", "account-1", "readable-rotation")?,
            )?;
            assert_eq!(manager.alab_reload_if(expected).await, Some(true));
            assert_eq!(cached_refresh()?, "readable-rotation");
            assert!(manager.auth_change_state_tx.borrow().generation > generation);
            let rotated_generation = manager.auth_change_state_tx.borrow().generation;
            assert_eq!(manager.alab_reload_if(expected).await, Some(false));
            assert_eq!(cached_refresh()?, "readable-rotation");
            assert_eq!(
                manager.auth_change_state_tx.borrow().generation,
                rotated_generation
            );
            assert!(
                server
                    .received_requests()
                    .await
                    .unwrap_or_default()
                    .is_empty()
            );
            println!("{CHILD_COMPLETE}");
            return Ok(());
        }

        if case == "refused-lock" {
            let target = login_file.with_file_name("outside-lock");
            std::os::unix::fs::symlink(&target, auth_lock::lock_path(&login_file))?;
            assert!(matches!(
                manager.refresh_token_from_authority().await,
                Err(RefreshTokenError::Transient(_))
            ));
            assert!(sent_refresh_tokens(&server).await?.is_empty());
            assert_eq!(stored_refresh_token(&login_file)?, "refresh-1");
            assert_eq!(
                manager
                    .auth_cached()
                    .context("cached login retained")?
                    .get_token_data()?
                    .refresh_token,
                "refresh-1"
            );
            assert!(!target.exists());
            println!("{CHILD_COMPLETE}");
            return Ok(());
        }

        // aterm holds the login's lock (an open of its own), and the refresh starts meanwhile.
        let holder = AuthLock::try_take(&login_file)?.context("the login lock was free")?;
        let aterm_hold = Arc::new(Mutex::new(Some(holder)));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let arrived = Arc::new(Notify::new());
        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .respond_with(LockWitness {
                login_file: login_file.clone(),
                aterm: Arc::clone(&aterm_hold),
                seen: Arc::clone(&seen),
                arrived: Arc::clone(&arrived),
            })
            .mount(&server)
            .await;
        let finished = AtomicBool::new(false);
        let refresh = async {
            let result = manager.refresh_token_from_authority().await;
            finished.store(true, Ordering::SeqCst);
            result
        };
        let aterm = async {
            tokio::time::sleep(HELD).await;
            let finished_while_held = finished.load(Ordering::SeqCst);
            // What aterm writes before it lets go.
            match case.as_str() {
                "rotated" => save(
                    &codex_home,
                    tokens(&server, "user-1", "account-1", "refresh-2-from-aterm")?,
                )?,
                "replaced" => save(
                    &codex_home,
                    tokens(&server, "user-2", "account-2", "refresh-of-another-login")?,
                )?,
                _ => {}
            }
            // aterm lets go, under the witness's mutex.
            {
                let mut hold = aterm_hold.lock().unwrap_or_else(PoisonError::into_inner);
                drop(hold.take());
            }
            // Where codex sends, aterm waits for the lock again from the moment codex's request
            // arrives (when codex holds it), and reads the login once it has it. That checks only
            // that auth.json already holds codex's answer when aterm next gets the lock. It does
            // not show codex wrote the answer under the lock: persist_tokens is synchronous after
            // the response, so on this current-thread runtime aterm's take cannot run between a
            // release and the write, and a hold dropped just before the write passes as well.
            let read_after_codex = if case == "held" {
                arrived.notified().await;
                let again = AuthLock::take(&login_file, HANG)
                    .await?
                    .context("codex kept the login lock")?;
                let stored = stored_refresh_token(&login_file)?;
                drop(again);
                Some(stored)
            } else {
                None
            };
            anyhow::Ok((finished_while_held, read_after_codex))
        };
        let (result, held) = tokio::time::timeout(HANG, async { tokio::join!(refresh, aterm) })
            .await
            .context("the refresh never finished")?;
        let (finished_while_held, read_after_codex) = held?;
        let seen = seen.lock().unwrap_or_else(PoisonError::into_inner).clone();
        assert!(
            !seen.contains(&AtRequest::AtermHeld),
            "sent while aterm held the lock: {seen:?}"
        );
        assert!(!finished_while_held, "finished while aterm held the lock");

        let sent = sent_refresh_tokens(&server).await?;
        let cached = manager
            .auth_cached()
            .context("no cached auth")?
            .get_token_data()?
            .refresh_token;
        match case.as_str() {
            "held" => {
                result?;
                assert_eq!(
                    seen,
                    vec![AtRequest::CodexHeld],
                    "codex did not hold the login lock through its request"
                );
                assert_eq!(
                    read_after_codex.as_deref(),
                    Some("refresh-from-codex"),
                    "auth.json did not hold codex's answer when aterm next got the login lock"
                );
                assert_eq!(sent, vec!["refresh-1".to_owned()]);
                assert_eq!(stored_refresh_token(&login_file)?, "refresh-from-codex");
                assert_eq!(cached, "refresh-from-codex");
            }
            "rotated" => {
                result?;
                assert_eq!(sent, Vec::<String>::new(), "spent a token aterm had spent");
                assert_eq!(stored_refresh_token(&login_file)?, "refresh-2-from-aterm");
                assert_eq!(cached, "refresh-2-from-aterm");
            }
            "replaced" => {
                let err = result.err().context("refreshed a login no longer stored")?;
                assert!(
                    err.to_string()
                        .contains("Account credentials changed during refresh."),
                    "{err}"
                );
                assert_eq!(sent, Vec::<String>::new(), "spent the old login's token");
                assert_eq!(
                    stored_refresh_token(&login_file)?,
                    "refresh-of-another-login"
                );
            }
            other => anyhow::bail!("unknown case {other}"),
        }
        println!("{CHILD_COMPLETE}");
        Ok(())
    }
}
