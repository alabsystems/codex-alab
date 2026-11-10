// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Native credential scope augments existing destination restrictions; it never widens them.
use super::*;
use codex_alab_core::native_source_lifetime::NetworkScope;
use codex_alab_core::native_source_lifetime::NetworkUse;

// Policies are copied through model/HTTP futures. Source observations stay on
// the heap; only actual UseState below owns counted resource/activity leases.
#[derive(Clone, Default, PartialEq, Eq)]
pub(super) struct Binding(Option<Arc<Scopes>>);
#[derive(Clone, Default, PartialEq, Eq)]
struct Scopes {
    native: Option<Result<Vec<NetworkScope>, ()>>,
    catalog: Option<Result<Origin, ()>>,
    destination_required: bool,
}
#[derive(Clone, PartialEq, Eq)]
enum Origin {
    Manager(Box<codex_alab_core::manager_catalog::Scope>),
    Reader(Box<codex_alab_core::manager_catalog::ReaderScope>),
    Both(
        Box<codex_alab_core::manager_catalog::Scope>,
        Box<codex_alab_core::manager_catalog::ReaderScope>,
    ),
}
impl Origin {
    fn join(&self, other: &Self) -> Option<Self> {
        if self == other {
            return Some(self.clone());
        }
        match (self, other) {
            (Self::Manager(manager), Self::Reader(reader))
            | (Self::Reader(reader), Self::Manager(manager))
                if reader.compatible_manager(manager) =>
            {
                Some(Self::Both(manager.clone(), reader.clone()))
            }
            (Self::Both(manager, _), Self::Manager(component)) if manager == component => {
                Some(self.clone())
            }
            (Self::Manager(component), right @ Self::Both(manager, _)) if component == manager => {
                Some(right.clone())
            }
            (Self::Both(_, reader), Self::Reader(component)) if reader == component => {
                Some(self.clone())
            }
            (Self::Reader(component), right @ Self::Both(_, reader)) if component == reader => {
                Some(right.clone())
            }
            _ => None,
        }
    }
    fn permits_pair(&self, native: &[NetworkScope]) -> bool {
        match self {
            Self::Both(manager, reader) if reader.compatible_manager(manager) => {
                reader.native_scope().is_ok_and(|actual| {
                    native.len() == 2
                        && native.iter().any(|scope| manager.matches_native(scope))
                        && native.iter().any(|scope| scope == &actual)
                })
            }
            _ => native.len() <= 1,
        }
    }
    fn acquire(&self) -> Result<OriginUse, codex_alab_core::assignment::Refused> {
        match self {
            Self::Manager(scope) => scope.acquire().map(|actual| OriginUse::Manager(Box::new(actual))),
            Self::Reader(scope) => scope.acquire().map(|actual| OriginUse::Reader(Box::new(actual))),
            Self::Both(manager, reader) if reader.compatible_manager(manager) => {
                Ok(OriginUse::Both(Box::new(manager.acquire()?), Box::new(reader.acquire()?)))
            }
            Self::Both(..) => Err(codex_alab_core::assignment::Refused::Unavailable),
        }
    }
    fn revoked(&self) -> bool {
        match self {
            Self::Manager(scope) => scope.revoked(),
            Self::Reader(scope) => scope.revoked(),
            Self::Both(manager, reader) => manager.revoked() || reader.revoked(),
        }
    }
}
#[derive(Clone, Debug)]
enum OriginUse {
    Manager(Box<codex_alab_core::manager_catalog::Use>),
    Reader(Box<codex_alab_core::manager_catalog::ReaderUse>),
    Both(
        Box<codex_alab_core::manager_catalog::Use>,
        Box<codex_alab_core::manager_catalog::ReaderUse>,
    ),
}
impl OriginUse {
    fn live(&self) -> bool {
        match self {
            Self::Manager(actual) => actual.live(),
            Self::Reader(actual) => actual.live(),
            Self::Both(manager, reader) => manager.live() && reader.live(),
        }
    }
    fn acquire_native(&self, native: &NetworkScope) -> std::io::Result<NetworkUse> {
        match self {
            Self::Manager(actual) => actual.acquire_native(native),
            Self::Reader(actual) => actual.acquire_native(native),
            Self::Both(manager, reader) => manager
                .acquire_native(native)
                .or_else(|_| reader.acquire_native(native)),
        }
    }
    async fn revoked(&self) {
        match self {
            Self::Manager(actual) => actual.revoked().await,
            Self::Reader(actual) => actual.revoked().await,
            Self::Both(manager, reader) => {
                tokio::select! { _ = manager.revoked() => {}, _ = reader.revoked() => {} }
            }
        }
    }
}
impl Binding {
    pub(super) const NONE: Self = Self(None);
    fn new(
        native: Option<Result<NetworkScope, ()>>,
        catalog: Option<Result<Origin, ()>>,
        destination_required: bool,
    ) -> Self {
        if native.is_none() && catalog.is_none() && !destination_required {
            Self::NONE
        } else {
            Self(Some(Arc::new(Scopes {
                native: native.map(|value| value.map(|scope| vec![scope])),
                catalog,
                destination_required,
            })))
        }
    }
    pub(super) fn is_bound(&self) -> bool {
        self.0.is_some()
    }
    pub(super) fn bind(&self, credentials: &Self) -> Option<Self> {
        let left = self.0.as_deref().cloned().unwrap_or_default();
        let right = credentials.0.as_deref().cloned().unwrap_or_default();
        let catalog = match (&left.catalog, &right.catalog) {
            (Some(Ok(left)), Some(Ok(right))) => Some(Ok(left.join(right)?)),
            (Some(left), Some(right)) if left != right => return None,
            (None, _) => right.catalog.clone(),
            _ => left.catalog.clone(),
        };
        let native = match (&left.native, &right.native) {
            (Some(Ok(left)), Some(Ok(right))) => {
                let mut scopes = left.clone();
                for scope in right {
                    if !scopes.contains(scope) {
                        scopes.push(scope.clone());
                    }
                }
                if scopes.len() > 2
                    || (scopes.len() > 1
                        && !matches!(&catalog, Some(Ok(origin)) if origin.permits_pair(&scopes)))
                {
                    return None;
                }
                Some(Ok(scopes))
            }
            (Some(left), Some(right)) if left != right => return None,
            (None, _) => right.native.clone(),
            _ => left.native.clone(),
        };
        if native.is_none()
            && catalog.is_none()
            && !left.destination_required
            && !right.destination_required
        {
            return Some(Self::NONE);
        }
        Some(Self(Some(Arc::new(Scopes {
            native,
            catalog,
            destination_required: left.destination_required || right.destination_required,
        }))))
    }
    pub(super) fn acquire(&self) -> Result<Option<Use>, NetworkPolicyDenied> {
        let Some(scopes) = self.0.as_deref() else {
            return Ok(None);
        };
        let catalog = match &scopes.catalog {
            None => None,
            Some(Ok(scope)) => Some(scope.acquire().map_err(|_| {
                if scope.revoked() {
                    NetworkPolicyDenied::Revoked
                } else {
                    NetworkPolicyDenied::Unavailable
                }
            })?),
            Some(Err(())) => return Err(NetworkPolicyDenied::Unavailable),
        };
        let native = match &scopes.native {
            None => None,
            Some(Ok(scopes)) => {
                let mut actual = Vec::with_capacity(scopes.len());
                for scope in scopes {
                    actual.push(
                        match &catalog {
                            Some(origin) => origin.acquire_native(scope),
                            None => scope.acquire(),
                        }
                        .map_err(|_| NetworkPolicyDenied::Unavailable)?,
                    );
                }
                Some(actual)
            }
            Some(Err(())) => return Err(NetworkPolicyDenied::Unavailable),
        };
        Ok(Some(Use(Arc::new(UseState { native, catalog }))))
    }
}
impl NetworkPolicy {
    /// Inspect configuration constraints without accepting a credential/account binding.
    pub fn alab_managed_configuration(&self) -> std::io::Result<bool> {
        if self.account.is_some() || self.alab_native.is_bound() {
            return Err(std::io::Error::other(
                "credential-bound configuration unavailable",
            ));
        }
        Ok(self.state.is_some())
    }

    /// Retain endpoint limits under an independently owned destination controller.
    /// This never copies the source account's composed policy or native scope.
    pub fn alab_for_destination_controller(
        &self,
        controller: &NetworkPolicyController,
    ) -> std::io::Result<Self> {
        self.alab_managed_configuration()?;
        if self
            .state
            .as_ref()
            .is_some_and(|source| Arc::ptr_eq(source, &controller.state))
        {
            return Err(std::io::Error::other(
                "destination account controller is shared",
            ));
        }
        let mut destination = controller.policy();
        destination.endpoints = self.endpoints.clone();
        Ok(destination)
    }

    pub fn with_manager_source(
        mut self,
        source: Result<codex_alab_core::manager_catalog::Scope, ()>,
    ) -> Self {
        self.alab_native = self
            .alab_native
            .bind(&Binding::new(
                None,
                Some(source.map(|actual| Origin::Manager(Box::new(actual)))),
                false,
            ))
            .unwrap_or(Binding::new(Some(Err(())), Some(Err(())), true));
        self
    }

    pub fn with_reader_source(
        mut self,
        source: Result<codex_alab_core::manager_catalog::ReaderScope, ()>,
    ) -> Self {
        let native = source
            .as_ref()
            .map_err(|_| ())
            .and_then(|reader| reader.native_scope().map_err(|_| ()));
        self.alab_native = self
            .alab_native
            .bind(&Binding::new(
                Some(native),
                Some(source.map(|actual| Origin::Reader(Box::new(actual)))),
                true,
            ))
            .unwrap_or(Binding::new(Some(Err(())), Some(Err(())), true));
        self
    }

    pub fn with_native_source(mut self, source: Result<NetworkScope, ()>) -> Self {
        let next = Binding::new(Some(source), None, true);
        self.alab_native = self.alab_native.bind(&next).unwrap_or(Binding::new(
            Some(Err(())),
            Some(Err(())),
            true,
        ));
        if let Some(scopes) = &mut self.alab_native.0 {
            let scopes = Arc::make_mut(scopes);
            if let (Some(Ok(native)), Some(Ok(catalog))) = (&scopes.native, &mut scopes.catalog)
                && let Origin::Manager(manager) = catalog
                && let [native] = native.as_slice()
            {
                **manager = manager.as_ref().clone().with_native_scope(native);
            }
        }
        self
    }
}
#[derive(Clone, Debug)]
pub(super) struct Use(Arc<UseState>);
#[derive(Debug)]
struct UseState {
    native: Option<Vec<NetworkUse>>,
    catalog: Option<OriginUse>,
}
impl Use {
    fn live(&self) -> bool {
        self.0
            .native
            .as_ref()
            .is_none_or(|uses| uses.iter().all(NetworkUse::live))
            && self.0.catalog.as_ref().is_none_or(OriginUse::live)
    }
    async fn prepare(&self) -> std::io::Result<()> {
        if let Some(native) = &self.0.native {
            for actual in native {
                actual.prepare().await?;
            }
        }
        if self.live() {
            Ok(())
        } else {
            Err(std::io::Error::other("credential source unavailable"))
        }
    }
    fn revoked(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            let native = async {
                match &self.0.native {
                    Some(native) => {
                        let first = async {
                            match native.first() {
                                Some(actual) => actual.revoked().await,
                                None => std::future::pending().await,
                            }
                        };
                        let second = async {
                            match native.get(1) {
                                Some(actual) => actual.revoked().await,
                                None => std::future::pending().await,
                            }
                        };
                        tokio::select! { _ = first => {}, _ = second => {} }
                    }
                    None => std::future::pending().await,
                }
            };
            let catalog = async {
                match &self.0.catalog {
                    Some(catalog) => catalog.revoked().await,
                    None => std::future::pending().await,
                }
            };
            tokio::select! { _ = native => {}, _ = catalog => {} }
        })
    }
}
pub(super) fn check(permit: &NetworkPermit) -> Result<(), NetworkPolicyDenied> {
    if permit
        .alab_native
        .as_ref()
        .is_some_and(|source| !source.live())
    {
        Err(NetworkPolicyDenied::Revoked)
    } else {
        Ok(())
    }
}
pub(super) async fn revoked(
    permit: &NetworkPermit,
    application: impl std::future::Future<Output = ()>,
) {
    let source = async {
        match &permit.alab_native {
            Some(source) => source.revoked().await,
            None => std::future::pending().await,
        }
    };
    tokio::select! { _ = application => {}, _ = source => {} }
}

pub(super) async fn prepare(permit: &NetworkPermit) -> Result<(), NetworkPolicyDenied> {
    if let Some(source) = &permit.alab_native {
        source
            .prepare()
            .await
            .map_err(|_| NetworkPolicyDenied::Unavailable)?;
    }
    check(permit)
}

pub(super) async fn run<T>(
    permit: &NetworkPermit,
    operation: impl std::future::Future<Output = T>,
) -> Result<T, NetworkPolicyDenied> {
    prepare(permit).await?;
    Ok(operation.await)
}

impl NetworkPermit {
    /// Keep the source-aware body operation behind the HTTP crate's Send boundary.
    pub(crate) fn alab_response_text(
        &self,
        response: reqwest::Response,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<String, crate::HttpError>> + Send + '_>,
    > {
        Box::pin(async move { Ok(self.run(response.text()).await??) })
    }
}

impl NetworkPolicy {
    /// Negative restriction only: known native material cannot escape as raw
    /// headers without its concrete destination owner. This grants no source use.
    pub fn with_credential_destination_required(mut self) -> Self {
        self.alab_native = self
            .alab_native
            .bind(&Binding::new(None, None, true))
            .unwrap_or(Binding::new(Some(Err(())), Some(Err(())), true));
        self
    }
    pub fn alab_requires_credential_destination(&self) -> bool {
        self.alab_native
            .0
            .as_ref()
            .is_some_and(|scopes| scopes.destination_required || scopes.native.is_some())
    }
    pub fn alab_has_native_member(&self) -> bool {
        self.alab_native
            .0
            .as_ref()
            .is_some_and(|scopes| scopes.native.is_some())
    }
}
impl NetworkPermit {
    pub async fn alab_prepare_source(&self) -> Result<(), NetworkPolicyDenied> {
        prepare(self).await
    }
}

impl NetworkPolicy {
    pub fn alab_admit_credential_use(&self) -> Result<CredentialSourceUse, NetworkPolicyDenied> {
        Ok(CredentialSourceUse {
            source: self.alab_native.acquire()?,
        })
    }
}
impl CredentialSourceUse {
    pub async fn prepare(&self) -> Result<(), NetworkPolicyDenied> {
        if let Some(source) = &self.source {
            source
                .prepare()
                .await
                .map_err(|_| NetworkPolicyDenied::Unavailable)?;
        }
        self.check()
    }
    pub fn check(&self) -> Result<(), NetworkPolicyDenied> {
        if self.source.as_ref().is_some_and(|source| !source.live()) {
            Err(NetworkPolicyDenied::Revoked)
        } else {
            Ok(())
        }
    }
}
