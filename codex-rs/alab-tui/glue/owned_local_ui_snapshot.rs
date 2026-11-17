// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Exact cloud-bundle DATA for frontend-only configuration preparation.
//! The backend retains its live binding, EMA policy, AuthManager and refresh.
//! A UI snapshot cannot authorize credential/model use or goal continuation.
use codex_config::CloudConfigBundle;
use codex_config::CloudConfigBundleLoader;
use codex_config::CloudConfigBundleSnapshot;
use codex_config::ConfigServer;
use codex_utils_absolute_path::AbsolutePathBuf;
use serde::Deserialize;
use serde::Serialize;
use std::io;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;

const MAGIC: &[u8; 8] = b"ALABUIB1";
const LIMIT: usize = 16 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OssChoice {
    pub(crate) provider: String,
    pub(crate) manually_selected: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UiSnapshot {
    schema: u32,
    #[serde(deserialize_with = "required_option")]
    bundle: Option<CloudConfigBundle>,
    #[serde(deserialize_with = "required_option")]
    config_server: Option<ConfigServer>,
    #[serde(with = "absolute_unix_path")]
    pub(crate) selected_cwd: AbsolutePathBuf,
    #[serde(deserialize_with = "required_option")]
    pub(crate) oss: Option<OssChoice>,
    #[serde(deserialize_with = "required_option")]
    pub(crate) initial_session_claim: Option<String>,
}
impl std::fmt::Debug for UiSnapshot {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("UiSnapshot(<private configuration data>)")
    }
}
impl UiSnapshot {
    /// The actual completed builder must use this SAME cloud snapshot. No
    /// ConfigRead map, synthetic config, default bundle or live handle is sent.
    pub(crate) fn from_backend(
        cloud: &CloudConfigBundleSnapshot,
        selected_cwd: AbsolutePathBuf,
        oss: Option<OssChoice>,
    ) -> io::Result<Self> {
        let bundle = cloud.bundle.as_ref().map_err(|_| unavailable())?.clone();
        Ok(Self {
            schema: 1,
            bundle,
            config_server: cloud.config_server.clone(),
            selected_cwd,
            oss,
            initial_session_claim: None,
        })
    }

    pub(crate) fn with_initial_session(mut self, claim: &str) -> io::Result<Self> {
        if claim.len() != 32
            || !claim
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(unavailable());
        }
        self.initial_session_claim = Some(claim.into());
        Ok(self)
    }

    /// Validate decoded configuration DATA regardless of its transport. No
    /// Native role, config reader retirement or UI application follows from it.
    pub(crate) fn validate_data(&self) -> io::Result<()> {
        if self.schema != 1
            || self.initial_session_claim.as_ref().is_some_and(|claim| {
                claim.len() != 32
                    || !claim
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            })
        {
            return Err(unavailable());
        }
        Ok(())
    }

    pub(crate) fn replace_managed_data(
        &mut self,
        actual: &codex_app_server_client::retained_readers::ManagedCloudData,
    ) -> io::Result<()> {
        let (bundle, server) = actual.successful_parts()?;
        self.bundle = bundle;
        self.config_server = server;
        self.validate_data()
    }

    pub(crate) fn config_server_data(&self) -> Option<&ConfigServer> {
        self.config_server.as_ref()
    }

    /// Only the original frontend ConfigBuilder consumes this frozen DATA.
    /// It intentionally carries no backend live binding/EMA/auth resources.
    /// The frontend must route auth/account/model operations to the actual
    /// backend client and keep its original local workspace/network semantics.
    pub(crate) fn ui_cloud_loader(&self) -> CloudConfigBundleLoader {
        let bundle = self.bundle.clone();
        let loader = CloudConfigBundleLoader::new(async move { Ok(bundle) });
        match self.config_server.clone() {
            Some(config_server) => loader.with_config_server(config_server),
            None => loader,
        }
    }
}

fn unavailable() -> io::Error {
    io::Error::other("owned local UI snapshot unavailable")
}

pub(crate) async fn write(stream: &mut UnixStream, snapshot: &UiSnapshot) -> io::Result<()> {
    let body = serde_json::to_vec(snapshot).map_err(|_| unavailable())?;
    if body.is_empty() || body.len() > LIMIT {
        return Err(unavailable());
    }
    stream.write_all(MAGIC).await?;
    stream.write_all(&(body.len() as u32).to_be_bytes()).await?;
    stream.write_all(&body).await?;
    stream.shutdown().await
}

/// Read only from an actual retained Native-delivered preparation endpoint.
/// This is a bounded data transfer, not an app-server configuration response,
/// Native delivery acknowledgement or UI/input-frontier acknowledgement.
pub(crate) async fn read(stream: &mut UnixStream) -> io::Result<UiSnapshot> {
    let mut header = [0; 12];
    stream.read_exact(&mut header).await?;
    if &header[..8] != MAGIC {
        return Err(unavailable());
    }
    let length = u32::from_be_bytes(header[8..].try_into().map_err(|_| unavailable())?) as usize;
    if length == 0 || length > LIMIT {
        return Err(unavailable());
    }
    let mut body = vec![0; length];
    stream.read_exact(&mut body).await?;
    let snapshot: UiSnapshot = serde_json::from_slice(&body).map_err(|_| unavailable())?;
    snapshot.validate_data()?;
    let mut tail = [0; 1];
    if stream.read(&mut tail).await? != 0 {
        return Err(unavailable());
    }
    Ok(snapshot)
}

// Preserve selected Unix cwd bytes just as ALABSET1 preserves the original
// argv/environment. JSON path strings would reject or change non-UTF-8 paths.
mod absolute_unix_path {
    use super::AbsolutePathBuf;
    use serde::Deserialize;
    use serde::Deserializer;
    use serde::Serialize;
    use serde::Serializer;
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::ffi::OsStringExt;
    pub(super) fn serialize<S: Serializer>(
        path: &AbsolutePathBuf,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        path.as_path().as_os_str().as_bytes().serialize(serializer)
    }
    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<AbsolutePathBuf, D::Error> {
        let bytes = Vec::<u8>::deserialize(deserializer)?;
        if bytes.contains(&0) {
            return Err(serde::de::Error::custom("invalid selected cwd"));
        }
        AbsolutePathBuf::from_absolute_path(std::path::PathBuf::from(OsString::from_vec(bytes)))
            .map_err(|_| serde::de::Error::custom("selected cwd must be absolute"))
    }
}

#[cfg(test)]
#[path = "owned_local_ui_snapshot_tests.rs"]
mod tests;

// The existing snapshot serializer always emits these fields, including null.
// Missing policy/choice DATA is not an observed empty bundle or absent claim.
fn required_option<'de, D, T>(decoder: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(decoder)
}
