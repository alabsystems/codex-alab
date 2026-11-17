// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The original config-server constructor, with custody of its existing task.
//! Compile as a child of cloud-config::config_server; foreground OAuth remains
//! in its existing caller, and this repeats the actual loader's auth checks.
use super::*;

pub(crate) fn parts(
    config: ConfigServer,
    codex_home: PathBuf,
    factory: HttpClientFactory,
) -> Result<(CloudConfigBundleLoader, tokio::task::JoinHandle<()>), CloudConfigBundleLoadError> {
    let source =
        ConfigServerSource::from_config_server(config.clone(), codex_home.clone(), factory)?;
    let service =
        CloudConfigBundleService::from_source(source, codex_home, CLOUD_CONFIG_BUNDLE_TIMEOUT);
    let (loader, refresh) = crate::bundle_loader::managed_config_bundle_loader_for_service(service);
    Ok((loader.with_config_server(config), refresh))
}
