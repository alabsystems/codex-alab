// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Host bookkeeping adapted from app-server's socket dispatcher. Protocol
//! handling, connection gates, routing and thread execution remain upstream.

use super::Prepared;
use crate::OutboundControlEvent;
use crate::transport::AppServerTransport;
use crate::transport::ConnectionId;
use crate::transport::ConnectionOrigin;
use crate::transport::ConnectionState;
use crate::transport::OutboundConnectionState;
use crate::transport::TransportEvent;
use crate::transport::route_outgoing_envelope;
use codex_alab_core::native_incoming::Incoming;
use codex_app_server_protocol::JSONRPCMessage;
use futures::FutureExt;
use std::collections::HashMap;
use std::collections::HashSet;
use std::io;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::sync::RwLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use tokio::sync::mpsc;
use tokio::task::JoinError;
use tokio::task::JoinHandle;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

type Joined = Result<(), JoinError>;

#[derive(Default)]
struct Connections {
    active: HashMap<ConnectionId, ConnectionState>,
    cleanup: JoinSet<()>,
    listeners: JoinSet<()>,
}

pub(super) async fn run(
    prepared: Prepared,
    incoming: Incoming,
    stop: CancellationToken,
) -> io::Result<()> {
    run_with_router(prepared, incoming, stop, route).await
}

async fn run_with_router<R, F>(
    prepared: Prepared,
    incoming: Incoming,
    stop: CancellationToken,
    router: R,
) -> io::Result<()>
where
    R: FnOnce(
        mpsc::Receiver<crate::outgoing_message::OutgoingEnvelope>,
        mpsc::Receiver<OutboundControlEvent>,
    ) -> F,
    F: std::future::Future<Output = ()> + Send + 'static,
{
    let Prepared {
        frontier,
        initial_session,
        mut task_mcp,
        processor,
        outgoing,
        outgoing_rx,
        startup_warnings_rx,
        analytics_flush,
        capacity,
        compression,
    } = prepared;
    let (event_tx, mut events) = mpsc::channel(capacity);
    let (control_tx, control_rx) = mpsc::channel(capacity);
    // A completed startup that lost its owner still needs processor cleanup,
    // but must never start accepting a frontend.
    let mut acceptor = if stop.is_cancelled() || incoming.withdrawn().now_or_never().is_some() {
        None
    } else {
        Some(codex_app_server_transport::start_native_incoming_acceptor(
            incoming.clone(),
            event_tx,
            stop.child_token(),
            compression,
        ))
    };
    let mut router = tokio::spawn(router(outgoing_rx, control_rx));
    let mut router_result = None;
    let mut acceptor_result = None;
    let mut connections = Connections::default();
    let processed = {
        // Catch a dispatcher panic while its connection/task custody remains
        // outside this future, so the same cleanup runs before reporting it.
        let processing = dispatch(
            &processor,
            &outgoing,
            &control_tx,
            &mut events,
            startup_warnings_rx,
            &mut connections,
            &stop,
        );
        let processing = AssertUnwindSafe(Box::pin(processing)).catch_unwind();
        tokio::pin!(processing);
        tokio::select! {
            biased;
            () = incoming.withdrawn() => {
                stop_admission(&processor, &outgoing, &incoming, &stop).await;
                processing.await
            }
            () = stop.cancelled() => {
                stop_admission(&processor, &outgoing, &incoming, &stop).await;
                processing.await
            }
            result = &mut router => {
                router_result = Some(result);
                stop_admission(&processor, &outgoing, &incoming, &stop).await;
                processing.await
            }
            result = wait_acceptor(&mut acceptor) => {
                acceptor_result = Some(result);
                stop_admission(&processor, &outgoing, &incoming, &stop).await;
                processing.await
            }
            result = &mut processing => result,
        }
    };
    let mut result =
        processed.unwrap_or_else(|_| Err(io::Error::other("retained dispatcher panicked")));
    if router_result.is_some() || acceptor_result.is_some() {
        remember_error(
            &mut result,
            io::Error::other("retained transport task ended unexpectedly"),
        );
    }
    stop_admission(&processor, &outgoing, &incoming, &stop).await;
    // A connection publishing Closed must be able to finish even if this
    // host stopped while the event queue was full.
    drop(events);
    let _ = control_tx.send(OutboundControlEvent::DisconnectAll).await;
    cancel_listeners(&mut connections.listeners, &mut result).await;
    for (id, connection) in connections.active {
        connection.session.rpc_gate.close().await;
        let processor = processor.clone();
        connections.cleanup.spawn(async move {
            processor.connection_closed(id, &connection.session).await;
        });
    }
    processor.clear_runtime_references();
    processor.cancel_active_login().await;
    while let Some(joined) = connections.cleanup.join_next().await {
        if joined.is_err() {
            remember_error(
                &mut result,
                io::Error::other("retained connection cleanup failed"),
            );
        }
    }
    task_mcp.drain_calls().await;
    if let Some(frontier) = &frontier
        && let Err(error) = frontier.drain().await
    {
        remember_error(&mut result, error);
    } // ALAB(retained-frontier): join actual owned workers before listener/Core teardown.
    if let Some(initial) = &initial_session
        && let Err(error) = initial.drain().await
    {
        remember_error(&mut result, io::Error::other(error));
    }
    processor.clear_all_thread_listeners().await;
    processor.drain_background_tasks().await;
    processor.shutdown_threads().await;
    if let Some(initial) = &initial_session {
        initial.release_primary_after_threads();
    } // ALAB(owned-local): real shutdown/drain, never frontend EOF, releases the primary owner.
    if let Err(error) = task_mcp.finish_after_threads().await {
        remember_error(&mut result, error);
    }
    if let Some(frontier) = &frontier
        && let Err(error) = frontier.finish_after_threads().await
    {
        remember_error(&mut result, error);
    }
    drop(frontier); // Exact primary/ACK/controller captures release after actual cleanup.
    drop(task_mcp);
    drop(initial_session); // Drop exact startup captures after actual thread cleanup.
    drop(processor);
    drop(outgoing);
    drop(control_tx);
    if let Some(joined) = acceptor_result {
        record_join(&mut result, joined);
    } else if let Some(acceptor) = acceptor {
        record_join(&mut result, acceptor.await);
    }
    record_join(
        &mut result,
        match router_result {
            Some(joined) => joined,
            None => router.await,
        },
    );
    analytics_flush.flush().await;
    if let Some(error) = incoming.failure() {
        // Provider failure and the acceptor's completion can become ready in
        // the same poll. Keep the actual cause rather than whichever generic
        // task-exit branch happened to win that race.
        return Err(io::Error::new(error.kind(), error));
    }
    result
}

async fn stop_admission(
    processor: &crate::message_processor::MessageProcessor,
    outgoing: &crate::outgoing_message::OutgoingMessageSender,
    incoming: &Incoming,
    stop: &CancellationToken,
) {
    processor.turn_admission.begin_drain();
    incoming.stop();
    processor.alab_stop_initial_session();
    processor.alab_stop_frontier();
    processor.alab_stop_task_mcp();
    stop.cancel();
    outgoing
        .cancel_all_requests(Some(crate::error_code::internal_error(
            "retained runtime is shutting down",
        )))
        .await;
}

async fn wait_acceptor(acceptor: &mut Option<JoinHandle<()>>) -> Joined {
    match acceptor {
        Some(handle) => handle.await,
        None => std::future::pending().await,
    }
}

async fn cancel_listeners(listeners: &mut JoinSet<()>, result: &mut io::Result<()>) {
    listeners.abort_all();
    while let Some(joined) = listeners.join_next().await {
        if let Err(error) = joined
            && !error.is_cancelled()
        {
            remember_error(result, io::Error::other("retained listener task failed"));
        }
    }
}

fn remember_error(result: &mut io::Result<()>, error: io::Error) {
    if result.is_ok() {
        *result = Err(error);
    }
}

fn record_join(result: &mut io::Result<()>, joined: Joined) {
    if joined.is_err() {
        remember_error(result, io::Error::other("retained transport task failed"));
    }
}

async fn dispatch(
    processor: &Arc<crate::message_processor::MessageProcessor>,
    outgoing: &crate::outgoing_message::OutgoingMessageSender,
    controls: &mpsc::Sender<OutboundControlEvent>,
    events: &mut mpsc::Receiver<TransportEvent>,
    mut warnings: mpsc::UnboundedReceiver<String>,
    connections: &mut Connections,
    stop: &CancellationToken,
) -> io::Result<()> {
    let mut created = processor.thread_created_receiver();
    let mut listen_for_threads = true;
    loop {
        tokio::select! {
            biased;
            () = stop.cancelled() => return Ok(()),
            event = events.recv() => {
                let Some(event) = event else {
                    return Err(io::Error::new(io::ErrorKind::BrokenPipe, "native input task ended"));
                };
                match event {
                    TransportEvent::DaemonShutdown => {
                        return Err(io::Error::other("unexpected daemon input on retained host"));
                    }
                    TransportEvent::ConnectionOpened { connection_id, origin, auth, resource, writer, disconnect_sender } => {
                        // The actual native acceptor always supplies this shape.
                        // In particular, no non-disconnectable writer may make
                        // the router wait forever on a saturated client queue.
                        if origin != ConnectionOrigin::WebSocket || auth.is_some() || disconnect_sender.is_none() {
                            if let Some(disconnect) = disconnect_sender { disconnect.cancel(); }
                            return Err(io::Error::other("invalid retained connection transport"));
                        }
                        let initialized = Arc::new(AtomicBool::new(false));
                        let experimental = Arc::new(AtomicBool::new(false));
                        let opted_out = Arc::new(RwLock::new(HashSet::new()));
                        controls.send(OutboundControlEvent::Opened {
                            connection_id, writer, disconnect_sender,
                            initialized: initialized.clone(),
                            experimental_api_enabled: experimental.clone(),
                            opted_out_notification_methods: opted_out.clone(),
                        }).await.map_err(|_| io::Error::other("outbound router ended"))?;
                        connections.active.insert(connection_id, ConnectionState::new(origin, auth, initialized, experimental, opted_out, resource));
                        if let Some(actual) = connections.active.get(&connection_id) { processor.alab_callback_connection_open(connection_id, &actual.session); } // ALAB(retained-ui): SAME real retained dispatcher connection gate before messages.
                    }
                    TransportEvent::AlabIngressDrained { connection_id } => processor.alab_callback_ingress_drained(connection_id), // ALAB(retained-ui): same original reader joined.
                    TransportEvent::ConnectionClosed { connection_id } => {
                        let Some(connection) = connections.active.remove(&connection_id) else { continue; };
                        connection.session.rpc_gate.close().await;
                        processor.alab_callback_connection_closing(connection_id, &connection.session); // ALAB(retained-ui): close same spawned publication admission before async cleanup.
                        let closed = controls.send(OutboundControlEvent::Closed { connection_id }).await.is_ok();
                        let processor = processor.clone();
                        connections.cleanup.spawn(async move { processor.connection_closed(connection_id, &connection.session).await; });
                        if !closed { return Err(io::Error::other("outbound router ended")); }
                        // The host remains alive, including after its last frontend.
                    }
                    TransportEvent::IncomingMessage { connection_id, message } => {
                        let Some(connection) = connections.active.get_mut(&connection_id) else { continue; };
                        if connection.session.rpc_gate.is_closed() { continue; }
                        match message {
                            JSONRPCMessage::Request(request) => {
                                let initialized = connection.session.initialized();
                                processor.process_request(connection_id, request, &AppServerTransport::Off, connection.session.clone()).await;
                                if let Ok(mut methods) = connection.outbound_opted_out_notification_methods.write() {
                                    *methods = connection.session.opted_out_notification_methods();
                                } else {
                                    return Err(io::Error::other("retained notification filter poisoned"));
                                }
                                connection.outbound_experimental_api_enabled.store(connection.session.experimental_api_enabled(), Ordering::Release);
                                if !initialized && connection.session.initialized() {
                                    processor.send_initialize_notifications_to_connection(connection_id).await;
                                    processor.connection_initialized(connection_id, connection.session.request_attestation()).await;
                                    connection.outbound_initialized.store(true, Ordering::Release);
                                }
                            }
                            JSONRPCMessage::Response(response) => processor.process_response(connection_id, response).await,
                            JSONRPCMessage::Notification(notification) => processor.process_notification(notification).await,
                            JSONRPCMessage::Error(error) => processor.process_error(connection_id, error).await,
                        }
                    }
                }
            }
            Some(warning) = warnings.recv(), if connections.active.values().any(|connection| connection.session.initialized()) => {
                crate::startup_warnings::send_warning(outgoing, warning).await;
            }
            result = created.recv(), if listen_for_threads => match result {
                Ok(thread) => {
                    let ids = connections.active.iter().filter_map(|(id, connection)| connection.session.initialized().then_some(*id)).collect();
                    let processor = processor.clone();
                    connections.listeners.spawn(async move { processor.try_attach_thread_listener(thread, ids).await; });
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => tracing::warn!("retained host missed thread creation notification"),
                Err(tokio::sync::broadcast::error::RecvError::Closed) => listen_for_threads = false,
            },
            Some(joined) = connections.cleanup.join_next(), if !connections.cleanup.is_empty() => {
                joined.map_err(|_| io::Error::other("retained connection cleanup failed"))?;
            }
            Some(joined) = connections.listeners.join_next(), if !connections.listeners.is_empty() => {
                joined.map_err(|_| io::Error::other("retained listener task failed"))?;
            }
        }
    }
}

async fn route(
    mut envelopes: mpsc::Receiver<crate::outgoing_message::OutgoingEnvelope>,
    mut controls: mpsc::Receiver<OutboundControlEvent>,
) {
    let mut connections = HashMap::<ConnectionId, OutboundConnectionState>::new();
    loop {
        tokio::select! {
            biased;
            control = controls.recv() => match control {
                Some(OutboundControlEvent::Opened {
                    connection_id, writer, disconnect_sender, initialized,
                    experimental_api_enabled, opted_out_notification_methods,
                }) => {
                    connections.insert(connection_id, OutboundConnectionState::new(
                        writer, initialized, experimental_api_enabled,
                        opted_out_notification_methods, disconnect_sender,
                    ));
                }
                Some(OutboundControlEvent::Closed { connection_id }) => { connections.remove(&connection_id); }
                Some(OutboundControlEvent::DisconnectAll) => {
                    for connection in connections.values() { connection.request_disconnect(); }
                    connections.clear();
                }
                None => break,
            },
            envelope = envelopes.recv() => {
                let Some(envelope) = envelope else { break; };
                route_outgoing_envelope(&mut connections, envelope).await;
            }
        }
    }
}

#[cfg(test)]
#[path = "owned_local_runtime_dispatch_tests.rs"]
mod tests;
