// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Registered direct readers participate without inventing an AuthManager.
use super::*;
use codex_alab_core::manager_catalog as catalog;
use codex_alab_core::native_reader_retirement::Observer;
use codex_alab_core::native_source_lifetime::Lifetime;
use codex_alab_core::native_source_lifetime::MemberIncarnation;
use codex_alab_core::native_source_lifetime::ReaderLoaded;

pub(super) struct Reader {
    identity: MemberIncarnation,
    observer: Observer,
    loaded: Mutex<Option<ReaderLoaded>>,
    policy: Mutex<Option<super::alab_native_source::target_policy::Witness>>,
    config: AuthConfig,
    modes: Option<Vec<AuthMode>>,
}
impl Reader {
    pub(super) fn begin(
        record: &Arc<catalog::Record>,
        config: &AuthConfig,
        modes: Option<&[AuthMode]>,
        member: &Lifetime,
    ) -> std::io::Result<Arc<Self>> {
        let reader = Arc::new(Self {
            identity: member.incarnation(),
            observer: member.retirement_observer(),
            loaded: Mutex::new(None),
            policy: Mutex::new(None),
            config: config.clone(),
            modes: modes.map(<[AuthMode]>::to_vec),
        });
        record
            .attach_reader(reader.clone())
            .map_err(|_| codex_alab_core::managed_auth::unavailable())?;
        #[cfg(test)]
        super::alab_native_member::tests::reader_admitted(record);
        Ok(reader)
    }
    pub(super) fn complete(&self, member: &Lifetime, auth: &CodexAuth) -> std::io::Result<()> {
        if member.incarnation() != self.identity {
            return Err(codex_alab_core::managed_auth::unavailable());
        }
        let loaded = member.reader_loaded()?;
        let witness = if loaded.no_native_reader().is_none() {
            let CodexAuth::Chatgpt(auth) = auth else {
                return Err(codex_alab_core::managed_auth::unavailable());
            };
            let tokens = auth
                .current_token_data()
                .ok_or_else(codex_alab_core::managed_auth::unavailable)?;
            Some(super::alab_native_source::target_policy::Witness::from_tokens(&tokens)?)
        } else {
            None
        };
        let mut current = self
            .loaded
            .lock()
            .map_err(|_| codex_alab_core::managed_auth::unavailable())?;
        if current.is_some() {
            return Err(codex_alab_core::managed_auth::unavailable());
        }
        *self
            .policy
            .lock()
            .map_err(|_| codex_alab_core::managed_auth::unavailable())? = witness;
        *current = Some(loaded);
        Ok(())
    }
}
impl catalog::ReaderBackend for Reader {
    fn identity(&self) -> &MemberIncarnation {
        &self.identity
    }
    fn loaded(&self) -> Result<ReaderLoaded, codex_alab_core::assignment::Refused> {
        let loaded = self
            .loaded
            .lock()
            .map_err(|_| codex_alab_core::assignment::Refused::Unavailable)?
            .clone()
            .ok_or(codex_alab_core::assignment::Refused::Pending)?;
        if !self.identity.reader_still_loaded(&loaded) {
            return Err(codex_alab_core::assignment::Refused::Unavailable);
        }
        Ok(loaded)
    }
    fn retirement(&self) -> Observer {
        self.observer.clone()
    }
    fn check_target(
        &self,
        target: &codex_alab_core::assignment_service::Target,
    ) -> Result<(), codex_alab_core::assignment::Refused> {
        let loaded = self.loaded()?;
        // Unrelated readers are a distinct actual source class. They must be
        // classified Disjoint, never reinterpreted as native target consumers.
        if loaded.no_native_reader().is_some()
            || target.validate(&target.operation).is_err()
            || !self
                .config
                .is_login_method_allowed(ForcedLoginMethod::Chatgpt)
            || self
                .modes
                .as_ref()
                .is_some_and(|modes| !modes.contains(&AuthMode::Chatgpt))
            || self
                .config
                .effective_chatgpt_workspaces()
                .is_some_and(|workspaces| {
                    !workspaces.iter().any(|workspace| {
                        super::alab_native_source::principal(
                            b"aterm/codex-renewal/account/v1\0",
                            workspace,
                        ) == target.account
                    })
                })
        {
            return Err(codex_alab_core::assignment::Refused::Unavailable);
        }
        self.policy
            .lock()
            .map_err(|_| codex_alab_core::assignment::Refused::Unavailable)?
            .as_ref()
            .ok_or(codex_alab_core::assignment::Refused::Unavailable)?
            .source_target(target)
            .map_err(|_| codex_alab_core::assignment::Refused::Unavailable)
    }
}

pub(super) async fn load(
    config: &AuthConfig,
    enable_api: bool,
    modes: Option<&[AuthMode]>,
) -> std::io::Result<Option<CodexAuth>> {
    let config = config.clone();
    let modes = modes.map(<[AuthMode]>::to_vec);
    let parent = codex_alab_core::assignment::current_activity();
    // This owned actor survives a dropped caller. It retains every accepted
    // constructor/load until an actual result and subsequent cleanup settle.
    let worker = Box::pin(async move {
        let factory = catalog::global()
            .factory(codex_alab_core::assignment::current_activity().as_ref())
            .map_err(|_| codex_alab_core::managed_auth::unavailable())?;
        if !factory.admitted() {
            return Err(codex_alab_core::managed_auth::unavailable());
        }
        let result = factory
            .scope(super::alab_native_member::read_direct(
                &config,
                enable_api,
                modes.as_deref(),
                factory.record(),
                factory.domain(),
            ))
            .await
            .map_err(|_| codex_alab_core::managed_auth::unavailable())??;
        factory
            .complete()
            .map_err(|_| codex_alab_core::managed_auth::unavailable())?;
        Ok(result)
    });
    tokio::spawn(async move {
        match parent {
            Some(parent) => parent.scope(worker).await,
            None => worker.await,
        }
    })
    .await
    .map_err(|_| codex_alab_core::managed_auth::unavailable())?
}
