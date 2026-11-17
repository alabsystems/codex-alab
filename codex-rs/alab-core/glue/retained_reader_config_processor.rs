// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::outgoing_message::alab_reader_claim::metadata::AccountData;
use crate::outgoing_message::alab_reader_claim::metadata::ManagedCloudData;
use crate::outgoing_message::alab_reader_claim::metadata::ReadKind;
impl ConfigRequestProcessor {
    pub(crate) async fn alab_retained_managed_read(
        &self,
        params: ConfigReadParams,
        kind: ReadKind,
    ) -> Result<(ConfigReadResponse, Option<ManagedCloudData>, AccountData), JSONRPCErrorError>
    {
        let cwd = params.cwd.as_ref().map(PathBuf::from);
        let response = self.read(params).await?; // Original typed config/layer/feature/redaction policy.
        // Preserve the original direct AuthConfig read, but obtain its policy
        // from this SAME actual backend's original ConfigManager. Native direct
        // reader admission remains in AuthConfig::load_auth; no C2 reads storage.
        let config = self.load_latest_config(cwd).await?;
        let auth = config.auth_config().load_auth(false).await.map_err(|_| {
            crate::error_code::internal_error("actual backend account observation unavailable")
        })?;
        let account = AccountData::from_actual(auth.as_ref());
        drop(auth); // Native reader custody is not projected to the frontend.
        let cloud = if kind == ReadKind::Managed {
            let actual = self
                .config_manager
                .alab_retained_managed_snapshot()
                .await
                .map_err(|_| {
                    crate::error_code::internal_error("actual backend managed snapshot unavailable")
                })?;
            Some(ManagedCloudData::from_actual(&actual))
        } else {
            None
        };
        Ok((response, cloud, account))
    }
}

#[cfg(all(test, unix))]
#[path = "retained_reader_runtime_tests.rs"]
mod tests;
