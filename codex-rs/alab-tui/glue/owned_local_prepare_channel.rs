// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Bounded Source preparation data on one actually supplied endpoint. This
//! protocol supplies no Native admission, SDK Ready, Go or input-frontier ACK.
use super::*;
use serde::Deserialize;
use serde::Serialize;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;

const MAGIC: &[u8; 8] = b"ALABPRE1";
const LIMIT: usize = 4096;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Status {
    Running,
    NotRunning,
    Unknown,
}
impl From<crate::oss_selection::ProviderStatus> for Status {
    fn from(value: crate::oss_selection::ProviderStatus) -> Self {
        match value {
            crate::oss_selection::ProviderStatus::Running => Self::Running,
            crate::oss_selection::ProviderStatus::NotRunning => Self::NotRunning,
            crate::oss_selection::ProviderStatus::Unknown => Self::Unknown,
        }
    }
}
impl From<Status> for crate::oss_selection::ProviderStatus {
    fn from(value: Status) -> Self {
        match value {
            Status::Running => Self::Running,
            Status::NotRunning => Self::NotRunning,
            Status::Unknown => Self::Unknown,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(super) enum Request {
    Select {
        request: String,
        lmstudio: Status,
        ollama: Status,
    },
    Automatic {
        request: String,
        provider: String,
    },
    Complete {
        request: String,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(super) enum Reply {
    Selected { request: String, provider: String },
    Observed { request: String },
}

pub(super) async fn write<T: Serialize>(stream: &mut UnixStream, message: &T) -> io::Result<()> {
    let body = serde_json::to_vec(message).map_err(|_| invalid())?;
    if body.len() > LIMIT {
        return Err(invalid());
    }
    stream.write_all(MAGIC).await?;
    stream.write_all(&(body.len() as u32).to_be_bytes()).await?;
    stream.write_all(&body).await
}
pub(super) async fn read<T: serde::de::DeserializeOwned>(stream: &mut UnixStream) -> io::Result<T> {
    let mut header = [0_u8; 12];
    stream.read_exact(&mut header).await?;
    if &header[..8] != MAGIC {
        return Err(invalid());
    }
    let length = u32::from_be_bytes(header[8..].try_into().map_err(|_| invalid())?) as usize;
    if length == 0 || length > LIMIT {
        return Err(invalid());
    }
    let mut body = vec![0; length];
    stream.read_exact(&mut body).await?;
    serde_json::from_slice(&body).map_err(|_| invalid())
}
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid Source preparation frame",
    )
}

pub(super) async fn select(
    stream: &mut UnixStream,
    detection: crate::oss_selection::OssProviderDetection,
) -> io::Result<crate::oss_selection::OssProviderSelection> {
    let request = uuid::Uuid::new_v4().to_string();
    let selected = match detection {
        crate::oss_selection::OssProviderDetection::AutoSelected(selection) => {
            write(
                stream,
                &Request::Automatic {
                    request: request.clone(),
                    provider: selection.provider.clone(),
                },
            )
            .await?;
            let Reply::Observed { request: actual } = read(stream).await? else {
                return Err(invalid());
            };
            if actual != request {
                return Err(invalid());
            }
            selection
        }
        crate::oss_selection::OssProviderDetection::NeedsSelection {
            lmstudio_status,
            ollama_status,
        } => {
            write(
                stream,
                &Request::Select {
                    request: request.clone(),
                    lmstudio: lmstudio_status.into(),
                    ollama: ollama_status.into(),
                },
            )
            .await?;
            let Reply::Selected {
                request: actual,
                provider,
            } = read(stream).await?
            else {
                return Err(invalid());
            };
            if actual != request
                || !matches!(provider.as_str(), "lmstudio" | "ollama" | "__CANCELLED__")
            {
                return Err(invalid());
            }
            crate::oss_selection::OssProviderSelection {
                provider,
                manually_selected: true,
            }
        }
    };
    if selected.provider == "__CANCELLED__" {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "OSS provider selection was cancelled by user",
        ));
    }
    write(
        stream,
        &Request::Complete {
            request: request.clone(),
        },
    )
    .await?;
    let Reply::Observed { request: actual } = read(stream).await? else {
        return Err(invalid());
    };
    if actual != request {
        return Err(invalid());
    }
    // Complete is Source data. Keep this endpoint until its peer has consumed
    // the frame and closed, before the phase owner can hand off its resources.
    // No app-server bytes belong on this dedicated preparation endpoint.
    let mut trailing = [0_u8; 1];
    if stream.read(&mut trailing).await? != 0 {
        return Err(invalid());
    }
    Ok(selected)
}

/// Caller retains the real foreground terminal/input owner and uses the
/// original restored-terminal handoff around this existing picker interaction.
pub async fn serve_frontend(stream: UnixStream) -> io::Result<()> {
    serve_frontend_with(stream, |lmstudio, ollama| {
        crate::oss_selection::select_oss_provider(lmstudio.into(), ollama.into())
    })
    .await
}

async fn serve_frontend_with<F, Fut>(mut stream: UnixStream, select: F) -> io::Result<()>
where
    F: FnOnce(Status, Status) -> Fut,
    Fut: Future<Output = io::Result<crate::oss_selection::OssProviderSelection>>,
{
    let request = match read(&mut stream).await? {
        Request::Select {
            request,
            lmstudio,
            ollama,
        } => {
            let selected = select(lmstudio, ollama).await?;
            let cancelled = selected.provider == "__CANCELLED__";
            write(
                &mut stream,
                &Reply::Selected {
                    request: request.clone(),
                    provider: selected.provider,
                },
            )
            .await?;
            if cancelled {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "OSS provider selection was cancelled by user",
                ));
            }
            request
        }
        Request::Automatic { request, provider } => {
            // Preserve an explicitly/configured provider exactly; the original
            // final config policy validates it. This is data, never authority.
            let _ = provider;
            write(
                &mut stream,
                &Reply::Observed {
                    request: request.clone(),
                },
            )
            .await?;
            request
        }
        Request::Complete { .. } => return Err(invalid()),
    };
    let Request::Complete { request: actual } = read(&mut stream).await? else {
        return Err(invalid());
    };
    if actual != request {
        return Err(invalid());
    }
    write(&mut stream, &Reply::Observed { request }).await?;
    // Dropping this stream completes the dedicated Source exchange. An extra
    // shutdown can race the phase owner's close on macOS after Complete.
    Ok(())
}

#[cfg(test)]
pub(super) async fn serve_test_choice(
    stream: UnixStream,
    provider: &'static str,
    gate: tokio::sync::oneshot::Receiver<()>,
    entered: tokio::sync::oneshot::Sender<()>,
) -> io::Result<()> {
    serve_frontend_with(stream, move |_, _| async move {
        let _ = entered.send(());
        gate.await
            .map_err(|_| io::Error::other("test choice gate closed"))?;
        Ok(crate::oss_selection::OssProviderSelection {
            provider: provider.into(),
            manually_selected: true,
        })
    })
    .await
}
