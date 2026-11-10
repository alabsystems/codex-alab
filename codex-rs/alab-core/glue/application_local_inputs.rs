// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Local configuration consistency only. No load, publication, credential or
//! native admission authority is conferred by an occurrence.
use super::super::*;
use std::sync::Mutex;
use std::sync::MutexGuard;

#[derive(Default)]
struct Occurrence;

pub(crate) struct LocalInputs {
    current: Mutex<Arc<Occurrence>>,
}
impl Default for LocalInputs {
    fn default() -> Self {
        Self {
            current: Mutex::new(Arc::new(Occurrence)),
        }
    }
}

/// Lexically declared before the existing runtime write guard. Rust drops
/// that guard first, then publishes this fresh occurrence before unlocking.
pub(crate) struct Update<'a> {
    guard: MutexGuard<'a, Arc<Occurrence>>,
}
impl Drop for Update<'_> {
    fn drop(&mut self) {
        *self.guard = Arc::new(Occurrence);
    }
}
impl LocalInputs {
    pub(crate) fn begin_update(&self) -> Update<'_> {
        // Preserve the ordinary setter's recovery semantics. Poison remains
        // recorded: no destination proof may be minted or validated afterward.
        Update {
            guard: self
                .current
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        }
    }
}

pub(crate) struct Snapshot {
    owner: Arc<LocalInputs>,
    occurrence: Arc<Occurrence>,
    pub(crate) cli_overrides: Vec<(String, TomlValue)>,
    pub(crate) runtime_feature_enablement: BTreeMap<String, bool>,
}
impl Snapshot {
    pub(crate) fn validate(&self, manager: &ConfigManager) -> std::io::Result<()> {
        self.with_current(manager, || Ok(()))
    }

    /// Join local freshness to one synchronous adoption operation. The callback
    /// must not await or reenter local-input mutation; no native permission is
    /// supplied here. Other resource and native proofs remain necessary.
    pub(crate) fn with_current(
        &self,
        manager: &ConfigManager,
        adopt: impl FnOnce() -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        if !Arc::ptr_eq(&self.owner, &manager.local_inputs) {
            return Err(std::io::Error::other("local configuration owner differs"));
        }
        let current = self
            .owner
            .current
            .lock()
            .map_err(|_| std::io::Error::other("local configuration occurrence poisoned"))?;
        if !Arc::ptr_eq(&current, &self.occurrence) {
            return Err(std::io::Error::other("local configuration changed"));
        }
        let result = adopt();
        drop(current);
        result
    }
}
impl ConfigManager {
    /// Copy both actual live inputs under the same short mutex as writers.
    /// No guard escapes this synchronous method or is held across an await.
    pub(crate) fn alab_capture_local_inputs(&self) -> std::io::Result<Snapshot> {
        let current = self
            .local_inputs
            .current
            .lock()
            .map_err(|_| std::io::Error::other("local configuration occurrence poisoned"))?;
        let cli_overrides = self
            .cli_overrides
            .read()
            .map_err(|_| std::io::Error::other("CLI overrides poisoned"))?
            .clone();
        let runtime_feature_enablement = self
            .runtime_feature_enablement
            .read()
            .map_err(|_| std::io::Error::other("runtime feature inputs poisoned"))?
            .clone();
        Ok(Snapshot {
            owner: Arc::clone(&self.local_inputs),
            occurrence: Arc::clone(&current),
            cli_overrides,
            runtime_feature_enablement,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registered_source() -> ConfigManager {
        ConfigManager::new(
            PathBuf::from("/not-read-by-local-input-controls"),
            Vec::new(),
            LoaderOverrides::default(),
            true,
            CloudConfigBundleLoader::default(),
            Arg0DispatchPaths::default(),
            super::super::alab_no_external_thread_config_loader(
                codex_config::NoopThreadConfigLoader,
            ),
        )
    }
    fn routes(manager: &ConfigManager) -> codex_login::AuthRouteConfig {
        let factory = |policy| {
            codex_http_client::HttpClientFactory::new(
                codex_http_client::OutboundProxyPolicy::RespectSystemProxy,
            )
            .with_network_policy(policy)
        };
        codex_login::AuthRouteConfig::from_http_client_factory(factory(
            manager.network_policy.policy(),
        ))
        .with_local_bootstrap_factory(factory(manager.local_network_policy.policy()))
    }

    #[test]
    fn actual_runtime_mutator_invalidates_equal_value_aba_across_destination_owner() {
        let source = registered_source();
        let destination = source
            .alab_begin_registered_destination(&routes(&source))
            .unwrap();
        assert!(Arc::ptr_eq(
            &source.local_inputs,
            &destination.manager.local_inputs
        ));
        source
            .extend_runtime_feature_enablement([("flag".into(), false)])
            .unwrap();
        let old = source.alab_capture_local_inputs().unwrap();
        old.validate(&destination.manager).unwrap();
        source
            .extend_runtime_feature_enablement([("flag".into(), true)])
            .unwrap();
        destination
            .manager
            .extend_runtime_feature_enablement([("flag".into(), false)])
            .unwrap();
        let current = destination.manager.alab_capture_local_inputs().unwrap();
        assert_eq!(old.cli_overrides, current.cli_overrides);
        assert_eq!(
            old.runtime_feature_enablement,
            current.runtime_feature_enablement
        );
        assert!(old.validate(&source).is_err());
        let mut adopted = false;
        assert!(
            old.with_current(&source, || {
                adopted = true;
                Ok(())
            })
            .is_err()
        );
        assert!(!adopted, "stale configuration must never execute adoption");
        current
            .with_current(&source, || {
                adopted = true;
                Ok(())
            })
            .unwrap();
        assert!(adopted);
        current.validate(&source).unwrap();
        let foreign = registered_source();
        assert!(current.validate(&foreign).is_err());
    }

    #[test]
    fn capture_waits_for_actual_runtime_writer_and_returns_its_settled_inputs() {
        let source = registered_source();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let deadline = std::time::Duration::from_secs(60);
        std::thread::scope(|threads| {
            let manager = &source;
            let writer = threads.spawn(move || {
                manager.extend_runtime_feature_enablement(std::iter::once_with(|| {
                    entered_tx.send(()).unwrap();
                    release_rx.recv_timeout(deadline).unwrap();
                    ("settled".into(), true)
                }))
            });
            entered_rx.recv_timeout(deadline).unwrap();
            let reader = threads.spawn(|| source.alab_capture_local_inputs());
            release_tx.send(()).unwrap();
            writer.join().unwrap().unwrap();
            let snapshot = reader.join().unwrap().unwrap();
            assert_eq!(
                snapshot.runtime_feature_enablement.get("settled"),
                Some(&true)
            );
            assert_eq!(snapshot.cli_overrides, source.current_cli_overrides());
            snapshot.validate(&source).unwrap();
        });
    }

    #[test]
    fn poisoned_occurrence_never_mints_or_validates_a_snapshot() {
        let source = registered_source();
        let old = source.alab_capture_local_inputs().unwrap();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _update = source.local_inputs.begin_update();
            panic!("interrupted local input update");
        }));
        source
            .extend_runtime_feature_enablement([("flag".into(), true)])
            .unwrap();
        assert!(old.validate(&source).is_err());
        assert!(source.alab_capture_local_inputs().is_err());
    }
}
