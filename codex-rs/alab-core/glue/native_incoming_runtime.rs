// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The caller supplies the complete existing runtime arguments. Only its input
//! transport and actual owner lifetime are extended here; no Config is rebuilt.

use super::AppServerRuntimeOptions;
use super::AppServerTransport;
use std::future::Future;
use std::io;
use std::pin::Pin;

pub(super) type OwnerWithdrawal = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

pub(super) struct PreparedIncoming {
    #[cfg(unix)]
    incoming: Option<codex_alab_core::native_incoming::Incoming>,
}

impl AppServerRuntimeOptions {
    pub(super) fn alab_prepare_incoming(
        &self,
        transport: &AppServerTransport,
    ) -> io::Result<(OwnerWithdrawal, PreparedIncoming)> {
        #[cfg(unix)]
        if let Some(incoming) = self.alab_incoming.clone() {
            if !matches!(transport, AppServerTransport::Off) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "native incoming streams require the local transport to be off",
                ));
            }
            let retained = PreparedIncoming {
                incoming: Some(incoming.clone()),
            };
            return Ok((
                Box::pin(async move {
                    tokio::select! {
                        () = incoming.withdrawn() => {}
                        () = codex_alab_core::native_backend_owner::withdrawn() => {}
                    }
                }),
                retained,
            ));
        }
        #[cfg(not(unix))]
        let _ = transport;
        Ok((
            Box::pin(codex_alab_core::native_backend_owner::withdrawn()),
            PreparedIncoming {
                #[cfg(unix)]
                incoming: None,
            },
        ))
    }
}

impl PreparedIncoming {
    pub(super) fn start(
        self,
        events: tokio::sync::mpsc::Sender<codex_app_server_transport::TransportEvent>,
        shutdown: tokio_util::sync::CancellationToken,
        compression: codex_app_server_transport::WebSocketCompression,
    ) -> Option<tokio::task::JoinHandle<()>> {
        #[cfg(unix)]
        {
            self.incoming.map(|incoming| {
                codex_app_server_transport::start_native_incoming_acceptor(
                    incoming,
                    events,
                    shutdown,
                    compression,
                )
            })
        }
        #[cfg(not(unix))]
        {
            let _ = (events, shutdown, compression);
            None
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use codex_alab_core::native_incoming::Accept;
    use codex_alab_core::native_incoming::Incoming;
    use codex_alab_core::native_incoming::IncomingStreamProvider;
    use codex_alab_core::native_incoming::Withdrawal;

    struct Provider;
    impl IncomingStreamProvider for Provider {
        fn accept(&self) -> Accept<'_> {
            Box::pin(std::future::pending())
        }
        fn withdrawn(&self) -> Withdrawal<'_> {
            Box::pin(std::future::pending())
        }
    }

    #[tokio::test]
    async fn native_incoming_runtime_requires_explicit_off_and_retains_exact_owner() {
        let incoming = Incoming::new(Provider);
        let options = AppServerRuntimeOptions {
            alab_incoming: Some(incoming.clone()),
            ..Default::default()
        };
        assert!(
            options
                .alab_prepare_incoming(&AppServerTransport::Stdio)
                .is_err()
        );
        let (mut withdrawal, retained) = options
            .alab_prepare_incoming(&AppServerTransport::Off)
            .unwrap();
        assert_eq!(retained.incoming.as_ref(), Some(&incoming));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(1), &mut withdrawal)
                .await
                .is_err()
        );
        incoming.stop();
        tokio::time::timeout(std::time::Duration::from_secs(1), withdrawal)
            .await
            .unwrap();
    }
}
