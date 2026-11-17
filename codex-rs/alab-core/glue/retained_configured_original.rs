// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Preserve the first ORIGINAL config/read and every original conversion in a
//! lexical startup operation. This scope supplies DATA only; Native and the
//! backend's actual empty selected slot independently admit the operation.
use super::*;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;

struct OriginalConfig {
    handle: crate::remote::RemoteAppServerRequestHandle,
    frontier: String,
    attempted: Mutex<bool>,
    receipt: Mutex<Option<Receipt>>,
}
tokio::task_local! { static ORIGINAL_CONFIG: Arc<OriginalConfig>; }
pub(crate) fn active() -> bool {
    ORIGINAL_CONFIG.try_with(|_| ()).is_ok()
}
fn unavailable(message: &'static str) -> IoError {
    IoError::new(ErrorKind::InvalidData, message)
}

/// The original typed config/read result has passed through the unchanged
/// caller's complete overlay/projectless/managed-default conversion. This is
/// neither a Native receipt nor proof it was installed/rendered by a UI.
pub struct ConfiguredProjection<T> {
    value: T,
    replay: ConfiguredReplay,
}
impl<T> ConfiguredProjection<T> {
    pub fn into_parts(self) -> (T, ConfiguredReplay) {
        (self.value, self.replay)
    }
}
/// One actual original operation's installation token. No Clone, serializer or
/// public constructor can manufacture or repeat its observed result.
pub struct ConfiguredReplay {
    receipt: Receipt,
    handle: crate::remote::RemoteAppServerRequestHandle,
}
impl ConfiguredReplay {
    pub fn receipt(&self) -> &Receipt {
        &self.receipt
    }
    /// Actual transport identity only; never Native/currentness authority.
    pub fn is_for_client(&self, actual: &AppServerRequestHandle) -> bool {
        matches!(actual, AppServerRequestHandle::Remote(remote) if self.handle.alab_same_channel(remote))
    }
}
impl std::fmt::Debug for ConfiguredReplay {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("ConfiguredReplay(<same original response/install operation; no grant>)")
    }
}

/// Only an actual OwnedLocal frontend owner invokes this around the WHOLE
/// original prepare_fresh_startup_config future. Its error remains unchanged.
/// Nested/foreign/construction scopes refuse before polling that original work.
pub async fn project_scope<T, E>(
    handle: &AppServerRequestHandle,
    frontier: &str,
    original: impl std::future::Future<Output = Result<T, E>>,
) -> Result<ConfiguredProjection<T>, E>
where
    E: From<IoError>,
{
    let AppServerRequestHandle::Remote(actual) = handle else {
        return Err(
            unavailable("configured original policy requires actual physical client").into(),
        );
    };
    if !(metadata::Selection::Project {
        frontier: frontier.into(),
    })
    .valid()
        || active()
        || crate::retained_frontier::original_policy_active()
        || crate::alab_initial_session_client::owns_construction_fallback()
    {
        return Err(unavailable(
            "configured original scope is invalid or overlaps another operation",
        )
        .into());
    }
    let scope = Arc::new(OriginalConfig {
        handle: actual.clone(),
        frontier: frontier.into(),
        attempted: Mutex::new(false),
        receipt: Mutex::new(None),
    });
    let value = ORIGINAL_CONFIG.scope(scope.clone(), original).await?;
    let receipt = scope
        .receipt
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
        .ok_or_else(|| {
            E::from(unavailable(
                "original configured conversion had no actual config/read receipt",
            ))
        })?;
    Ok(ConfiguredProjection {
        value,
        replay: ConfiguredReplay {
            receipt,
            handle: actual.clone(),
        },
    })
}

/// Called by the original request-handle dispatcher. No transport, request ID,
/// unowned fields, trace or server error is reconstructed by the TUI caller.
pub(crate) async fn request(
    handle: &AppServerRequestHandle,
    request: ClientRequest,
) -> IoResult<RequestResult> {
    let Some(scope) = ORIGINAL_CONFIG.try_with(Arc::clone).ok() else {
        return crate::retained_frontier::original_policy::request(handle, request).await;
    };
    if matches!(
        request.method_name(),
        "thread/start" | "thread/fork" | "thread/resume"
    ) {
        return Err(unavailable(
            "configured projection cannot construct a Core inside the config installation scope",
        ));
    }
    if request.method_name() != "config/read" {
        return crate::retained_frontier::original_policy::request(handle, request).await;
    }
    {
        let mut attempted = scope
            .attempted
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if *attempted {
            return Err(unavailable("configured original read already attempted"));
        }
        *attempted = true;
    }
    let AppServerRequestHandle::Remote(actual) = handle else {
        return Err(unavailable("configured original client changed"));
    };
    if !scope.handle.alab_same_channel(actual)
        || crate::retained_frontier::original_policy_active()
        || crate::alab_initial_session_client::owns_construction_fallback()
    {
        return Err(unavailable(
            "configured original request changed channel or operation scope",
        ));
    }
    let wire: JSONRPCRequest =
        serde_json::from_value(serde_json::to_value(request).map_err(IoError::other)?)
            .map_err(IoError::other)?;
    let mut value = match send(
        handle,
        wire,
        &metadata::Selection::Project {
            frontier: scope.frontier.clone(),
        },
    )
    .await
    {
        Ok(value) => value,
        Err(TypedRequestError::Server { source, .. }) => return Ok(Err(source)),
        Err(TypedRequestError::Transport { source, .. }) => return Err(source),
        Err(TypedRequestError::Deserialize { source, .. }) => {
            return Err(IoError::new(ErrorKind::InvalidData, source));
        }
    };
    let receipt =
        remove_receipt(&mut value).map_err(|error| IoError::new(ErrorKind::InvalidData, error))?;
    if receipt.frontier_data() != scope.frontier {
        return Err(unavailable("configured original frontier disagrees"));
    }
    *scope.receipt.lock().unwrap_or_else(PoisonError::into_inner) = Some(receipt);
    Ok(Ok(value))
}
