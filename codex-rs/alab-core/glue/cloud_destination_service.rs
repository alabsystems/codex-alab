// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Explicit destination loads are fresh, fallible observations in caller scope.
use super::*;
impl<S: BundleSource + 'static> CloudConfigBundleService<S> {
    pub(crate) async fn destination_snapshot(&self) -> CloudConfigBundleSnapshot {
        let mut snapshot = CloudConfigBundleSnapshot {
            config_server: None,
            bundle: self.load_startup_bundle_with_timeout().await,
            binding: None,
        };
        // Ineligibility is a successful empty source observation, not an old
        // enterprise bundle. It bypasses the remote fetch's observe hook.
        if matches!(&snapshot.bundle, Ok(None)) {
            self.policy
                .observe_remote_bundle(&CloudConfigBundle::default());
        }
        self.policy.publish_snapshot(&mut snapshot);
        snapshot
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    struct Source(Arc<AtomicUsize>);
    impl BundleSource for Source {
        type Auth = ();
        type FetchContext = ();
        fn begin_fetch(&self) {}
        async fn auth(&self) -> Result<Option<()>, CloudConfigBundleLoadError> {
            if self.0.fetch_add(1, Ordering::AcqRel) == 0 {
                Err(CloudConfigBundleLoadError::new(
                    CloudConfigBundleLoadErrorCode::Auth,
                    None,
                    "fixture authorization unavailable",
                ))
            } else {
                Ok(None)
            }
        }
        fn identity(&self, _: &()) -> (Option<String>, Option<String>) {
            (None, None)
        }
        async fn get_bundle(
            &self,
            _: &mut (),
            _: &mut (),
        ) -> Result<CloudConfigBundle, BundleRequestError> {
            panic!("source was not eligible")
        }
        async fn recover_auth(
            &self,
            _: &mut (),
            _: &mut (),
            _: Option<u16>,
            _: usize,
        ) -> Result<bool, BundleRequestError> {
            panic!("source was not eligible")
        }
    }
    #[tokio::test]
    async fn explicit_load_reobserves_auth_after_error_without_detached_or_sticky_startup() {
        let home = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let service = CloudConfigBundleService::from_source(
            Source(calls.clone()),
            home.path().into(),
            Duration::from_secs(1),
        )
        .without_cache();
        assert_eq!(calls.load(Ordering::Acquire), 0);
        assert!(service.destination_snapshot().await.bundle.is_err());
        assert!(matches!(
            service.destination_snapshot().await.bundle,
            Ok(None)
        ));
        assert!(matches!(
            service.destination_snapshot().await.bundle,
            Ok(None)
        ));
        assert_eq!(
            calls.load(Ordering::Acquire),
            3,
            "each explicit load observes actual source again"
        );
    }
    struct ChangingSource(AtomicUsize);
    impl BundleSource for ChangingSource {
        type Auth = usize;
        type FetchContext = ();
        fn begin_fetch(&self) {}
        async fn auth(&self) -> Result<Option<usize>, CloudConfigBundleLoadError> {
            let observation = self.0.fetch_add(1, Ordering::AcqRel);
            Ok((observation < 2).then_some(observation))
        }
        fn identity(&self, _: &usize) -> (Option<String>, Option<String>) {
            (None, None)
        }
        async fn get_bundle(
            &self,
            auth: &mut usize,
            _: &mut (),
        ) -> Result<CloudConfigBundle, BundleRequestError> {
            let mut bundle = CloudConfigBundle::default();
            bundle
                .config_toml
                .enterprise_managed
                .push(codex_config::CloudConfigFragment {
                    id: "fixture".into(),
                    name: "fixture".into(),
                    contents: format!("model = \"fixture-{auth}\""),
                });
            Ok(bundle)
        }
        async fn recover_auth(
            &self,
            _: &mut usize,
            _: &mut (),
            _: Option<u16>,
            _: usize,
        ) -> Result<bool, BundleRequestError> {
            panic!("fixture fetch succeeds")
        }
    }
    #[tokio::test]
    async fn changed_fetch_and_later_ineligibility_revoke_previous_ema_observations() {
        use codex_config::CloudConfigBundleBindingStatus::Current;
        use codex_config::CloudConfigBundleBindingStatus::Stale;
        let home = tempfile::tempdir().unwrap();
        let service = CloudConfigBundleService::from_source(
            ChangingSource(AtomicUsize::new(0)),
            home.path().into(),
            Duration::from_secs(1),
        )
        .without_cache();
        let first = service.destination_snapshot().await;
        let first_binding = first.binding.unwrap();
        assert!(matches!(first_binding.read().status, Current));
        let second = service.destination_snapshot().await;
        assert_ne!(first.bundle, second.bundle);
        let second_binding = second.binding.unwrap();
        assert!(matches!(first_binding.read().status, Stale));
        assert!(matches!(second_binding.read().status, Current));
        let empty = service.destination_snapshot().await;
        assert!(matches!(empty.bundle, Ok(None)));
        assert!(matches!(second_binding.read().status, Stale));
        assert!(matches!(empty.binding.unwrap().read().status, Current));
    }
}
