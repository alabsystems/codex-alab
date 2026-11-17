// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! SAME live backend managed loader, frozen once without a credential copy.
use super::*;
impl ConfigManager {
    pub(crate) async fn alab_retained_managed_snapshot(
        &self,
    ) -> std::io::Result<codex_config::CloudConfigBundleSnapshot> {
        let actual = {
            self.cloud_config_bundle
                .read()
                .map_err(|_| std::io::Error::other("actual managed loader unavailable"))?
                .clone()
        };
        Ok(actual.get_snapshot().await)
    }
}
