// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Withdrawal of the actual native backend owner, not frontend disconnection.
use std::sync::OnceLock;
use tokio::sync::watch;

static WITHDRAWN: OnceLock<watch::Receiver<bool>> = OnceLock::new();

/// Wait only for withdrawal of the private backend's physical owner. Ordinary
/// processes have no such owner and this future never completes for them.
/// This is cancellation input, not a statement that cleanup has completed.
pub async fn withdrawn() {
    let Some(receiver) = WITHDRAWN.get() else {
        return std::future::pending().await;
    };
    wait(receiver.clone()).await;
}

async fn wait(mut receiver: watch::Receiver<bool>) {
    while !*receiver.borrow_and_update() {
        if receiver.changed().await.is_err() {
            return;
        }
    }
}

#[cfg(unix)]
pub(super) struct Owner(tokio::task::JoinHandle<()>, std::sync::Arc<ToolChannel>);

#[cfg(unix)]
struct ToolChannel {
    stream: tokio::net::UnixStream,
    member: std::sync::Arc<dyn crate::native_source::Member>,
    write: tokio::sync::Mutex<()>,
}

#[cfg(unix)]
static TOOL_CHANNEL: OnceLock<std::sync::Weak<ToolChannel>> = OnceLock::new();

/// The actual retained constructor, never reconstructed from environment data.
#[cfg(unix)]
pub(crate) fn tool_member()
-> std::io::Result<Option<std::sync::Arc<dyn crate::native_source::Member>>> {
    match TOOL_CHANNEL.get() {
        None => Ok(None),
        Some(channel) => channel
            .upgrade()
            .map(|channel| Some(channel.member.clone()))
            .ok_or_else(crate::managed_auth::unavailable),
    }
}

/// Whether this process has an actual live private constructor. This is only
/// route selection; native rechecks SDK membership and the socket on every use.
#[cfg(unix)]
pub fn has_tool_owner() -> std::io::Result<bool> {
    tool_member().map(|member| member.is_some())
}

/// Publish the actual private listener on this backend's original channel.
/// Ordinary processes have no native owner and keep their normal transport.
#[cfg(unix)]
#[allow(
    clippy::await_holding_invalid_type,
    reason = "serialize the entire listener descriptor frame on the original socket"
)]
pub async fn publish_listener(listener: &tokio::net::TcpListener) -> std::io::Result<()> {
    use std::os::fd::AsFd as _;
    let Some(weak) = TOOL_CHANNEL.get() else {
        return Ok(());
    };
    let channel = weak
        .upgrade()
        .ok_or_else(crate::managed_auth::unavailable)?;
    let _write = channel.write.lock().await;
    let operation = format!("{:032x}", rand::random::<u128>());
    let nonce = channel.member.prepare_owned_listener(&operation).await?;
    let descriptor = listener.as_fd().try_clone_to_owned()?;
    tokio::time::timeout(
        std::time::Duration::from_secs(60),
        crate::native_root_io::send_listener(&channel.stream, &nonce, descriptor),
    )
    .await
    .map_err(|_| std::io::Error::from(std::io::ErrorKind::TimedOut))?
}

/// Send on the retained backend constructor's original physical channel.
/// This does not report that target exec succeeded. On an uncertain delivery,
/// reconcile the same tool operation through the SDK before doing more work.
#[cfg(unix)]
#[allow(
    clippy::await_holding_invalid_type,
    reason = "serialize a complete asynchronous descriptor frame on the original socket; cancellation closes the write side"
)]
pub async fn send_tool_io(
    operation: &str,
    tool: &str,
    payload: &[u8],
    descriptors: [std::os::fd::OwnedFd; 3],
) -> std::io::Result<crate::native_root::IoOffer> {
    let channel = TOOL_CHANNEL
        .get()
        .and_then(std::sync::Weak::upgrade)
        .ok_or_else(crate::managed_auth::unavailable)?;
    let _write = channel.write.lock().await;
    let offer = channel
        .member
        .root_prepare_tool(operation, tool, payload)
        .await?;
    if let crate::native_root::IoOffer::Pending { nonce } = &offer {
        tokio::time::timeout(
            std::time::Duration::from_secs(60),
            crate::native_root_io::send(&channel.stream, nonce, payload, descriptors),
        )
        .await
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::TimedOut))??;
    }
    Ok(offer)
}

#[cfg(unix)]
impl Drop for Owner {
    fn drop(&mut self) {
        self.0.abort();
        use std::os::fd::AsRawFd as _;
        // Outstanding senders cannot extend backend ownership after Startup ends.
        unsafe { libc::shutdown(self.1.stream.as_raw_fd(), libc::SHUT_RDWR) };
    }
}

#[cfg(unix)]
pub(super) fn retain(control: crate::native_owned_control::Bound) -> std::io::Result<Owner> {
    let (owner, withdrawn) = monitor(control);
    TOOL_CHANNEL
        .set(std::sync::Arc::downgrade(&owner.1))
        .map_err(|_| crate::managed_auth::unavailable())?;
    WITHDRAWN
        .set(withdrawn)
        .map_err(|_| crate::managed_auth::unavailable())?;
    Ok(owner)
}

#[cfg(unix)]
fn monitor(control: crate::native_owned_control::Bound) -> (Owner, watch::Receiver<bool>) {
    let (stream, member) = control.into_parts();
    let channel = std::sync::Arc::new(ToolChannel {
        stream,
        member,
        write: tokio::sync::Mutex::new(()),
    });
    let (sender, receiver) = watch::channel(false);
    let observed = channel.clone();
    let task = tokio::spawn(async move {
        observe(&observed.stream).await;
        // A closed owner must also interrupt any pending descriptor delivery.
        use std::os::fd::AsRawFd as _;
        unsafe { libc::shutdown(observed.stream.as_raw_fd(), libc::SHUT_RDWR) };
        sender.send_replace(true);
    });
    (Owner(task, channel), receiver)
}

#[cfg(unix)]
async fn observe(stream: &tokio::net::UnixStream) {
    // Bootstrap consumed the challenge. Native sends no further frames; any
    // byte, EOF or IO failure withdraws ownership. Source writes use the other
    // direction of this same physical socket.
    loop {
        if stream.readable().await.is_err() {
            return;
        }
        match stream.try_read(&mut [0]) {
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => continue,
            _ => return,
        }
    }
}

#[cfg(all(test, unix))]
#[path = "native_backend_owner_tests.rs"]
mod tests;
