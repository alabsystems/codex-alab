// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Private backend dispatch, before configuration takes the native constructor.
use crate::managed_auth::unavailable;
use std::ffi::OsString;
use std::io;

const MARKER: &str = "--alab-owned-backend";

/// Main-stack custody of the original bootstrap socket. This is not a root or
/// frontend attachment, nor proof that any separate tool process has ended.
#[must_use = "retain backend bootstrap custody through dispatch"]
pub struct Startup {
    #[cfg(unix)]
    _control: Option<super::native_backend_owner::Owner>,
}

fn requested(arguments: &[OsString]) -> io::Result<bool> {
    if !arguments.get(1).is_some_and(|argument| argument == MARKER) {
        return Ok(false);
    }
    if arguments
        .iter()
        .filter(|argument| *argument == MARKER)
        .count()
        != 1
    {
        return Err(unavailable());
    }
    let mut tail = &arguments[2..];
    while tail.first().is_some_and(|argument| argument == "--config") {
        if tail.len() < 2 || tail[1].is_empty() {
            return Err(unavailable());
        }
        tail = &tail[2..];
    }
    if tail.len() != 7
        || tail[0] != "app-server"
        || tail[1] != "--listen"
        || tail[2] != "ws://127.0.0.1:0"
        || tail[3] != "--ws-auth"
        || tail[4] != "capability-token"
        || tail[5] != "--ws-token-sha256"
        || !tail[6]
            .to_str()
            .is_some_and(|digest| crate::managed_auth::valid_hex(digest, 64))
    {
        return Err(unavailable());
    }
    Ok(true)
}

/// Strip only the validated private dispatch marker before the normal CLI
/// parser runs. It is a request to bind an actual socket, never authority.
pub fn arguments() -> io::Result<Vec<OsString>> {
    let mut arguments = std::env::args_os().collect::<Vec<_>>();
    if requested(&arguments)? {
        arguments.remove(1);
    }
    Ok(arguments)
}

/// Admit the constructor and, for a private backend, bind the physical socket
/// before any configuration or authentication workflow can consume that seed.
/// A malformed private request or failed binding never falls back to normal
/// app-server startup. Keep the returned owner until dispatch has finished.
pub async fn enter() -> io::Result<Startup> {
    let owned = requested(&std::env::args_os().collect::<Vec<_>>())?;
    #[cfg(debug_assertions)]
    if owned {
        eprintln!("Native backend startup: constructor");
    }
    crate::native_source_launch::enter().await?;
    #[cfg(debug_assertions)]
    if owned {
        eprintln!("Native backend startup: admitted");
    }
    #[cfg(unix)]
    {
        use std::os::fd::AsFd as _;
        let control = if owned {
            if !crate::native_source_launch::requested() {
                return Err(unavailable());
            }
            let socket = std::io::stdin().as_fd().try_clone_to_owned()?;
            let control = crate::native_source_launch::bind_owned_control(socket.into()).await?;
            crate::native_source_launch::separate_backend_constructor(&control)?;
            #[cfg(debug_assertions)]
            eprintln!("Native backend startup: socket bound");
            // The retained clone is close-on-exec. Do not lend the bootstrap
            // socket to helpers through inherited standard input.
            use std::os::fd::AsRawFd as _;
            let input = std::fs::File::open("/dev/null")?;
            if unsafe { libc::dup2(input.as_raw_fd(), libc::STDIN_FILENO) } < 0 {
                return Err(io::Error::last_os_error());
            }
            let owner = super::native_backend_owner::retain(control)?;
            #[cfg(debug_assertions)]
            eprintln!("Native backend startup: owner retained");
            let member = super::native_backend_owner::tool_member()?.ok_or_else(unavailable)?;
            member
                .root_protocol(&format!("{:032x}", rand::random::<u128>()))
                .await?;
            #[cfg(debug_assertions)]
            eprintln!("Native backend startup: root available");
            Some(owner)
        } else {
            None
        };
        Ok(Startup { _control: control })
    }
    #[cfg(not(unix))]
    {
        if owned {
            return Err(unavailable());
        }
        Ok(Startup {})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn private_arguments() -> Vec<OsString> {
        [
            "codex",
            MARKER,
            "--config",
            "model=\"example\"",
            "app-server",
            "--listen",
            "ws://127.0.0.1:0",
            "--ws-auth",
            "capability-token",
            "--ws-token-sha256",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        ]
        .into_iter()
        .map(Into::into)
        .collect()
    }

    #[test]
    fn private_dispatch_cannot_select_another_workflow_or_public_listener() {
        let arguments = private_arguments();
        assert!(requested(&arguments).unwrap());
        for (position, value) in [
            (4, "login"),
            (6, "ws://0.0.0.0:0"),
            (8, "none"),
            (10, "bad"),
        ] {
            let mut changed = arguments.clone();
            changed[position] = value.into();
            assert!(requested(&changed).is_err());
        }
        let mut changed = arguments.clone();
        changed.push(MARKER.into());
        assert!(requested(&changed).is_err());
        let mut changed = arguments;
        changed.swap(1, 4);
        assert!(!requested(&changed).unwrap());
    }

    #[test]
    fn ordinary_dispatch_does_not_request_socket_binding() {
        for words in [
            vec!["codex"],
            vec!["codex", "app-server"],
            vec!["codex", "login"],
            vec!["codex", "exec", "--", MARKER],
        ] {
            let arguments = words.into_iter().map(Into::into).collect::<Vec<_>>();
            assert!(!requested(&arguments).unwrap());
        }
    }
}
