// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A destination load runs in its caller's actual source/resource admission.
//! No detached prewarm may read B before that admission is established.
use super::*;
pub fn loader(
    manager: Arc<AuthManager>,
    base: String,
    home: PathBuf,
    factory: HttpClientFactory,
) -> CloudConfigBundleLoader {
    let service = Arc::new(
        CloudConfigBundleService::new(
            manager,
            Arc::new(BackendBundleClient::new(base, factory)),
            home,
            CLOUD_CONFIG_BUNDLE_TIMEOUT,
        )
        .without_cache(),
    );
    let snapshots = service.clone();
    let policy = service.policy.clone();
    CloudConfigBundleLoader::from_getter(move || {
        let service = service.clone();
        async move { service.destination_snapshot().await.bundle }
    })
    .with_ema_policy_snapshots(policy, move || {
        let service = snapshots.clone();
        async move { service.destination_snapshot().await }
    })
}
