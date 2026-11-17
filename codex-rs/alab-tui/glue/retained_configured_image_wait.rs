// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Held is not completed configuration. Its receiver is armed before the actual
//! draw; only a real plan/binding change permits another observation attempt.
use std::io;
use tokio::sync::watch;
type Close = watch::Receiver<Option<std::result::Result<(), io::ErrorKind>>>;
pub(crate) enum Observation {
    Observed,
    Held(Held),
}
pub(crate) struct Held {
    images: watch::Receiver<()>,
    close: Close,
}
pub(super) fn check_close(actual: &Close) -> io::Result<()> {
    let terminal = { *actual.borrow() };
    match terminal {
        None => Ok(()),
        Some(outcome) => Err(io::Error::new(
            outcome.err().unwrap_or(io::ErrorKind::ConnectionAborted),
            "actual configured frontend delivery closed before UI observation",
        )),
    }
}
impl Held {
    pub(super) fn new(images: watch::Receiver<()>, close: Close) -> Self {
        Self { images, close }
    }
    pub(crate) async fn wait(mut self) -> io::Result<()> {
        let mut image_live = true;
        let mut close_live = true;
        loop {
            check_close(&self.close)?;
            tokio::select! {
                result=self.images.changed(),if image_live=>match result {
                    Ok(())=>{
                        check_close(&self.close)?;
                        return Ok(()); // Notification only; caller fetches/checks current real binding again.
                    }
                    Err(_)=>image_live=false, // Lost observer is not restored/applied/stopped.
                },
                result=self.close.changed(),if close_live=>{
                    if result.is_err(){close_live=false;}
                },
                else=>std::future::pending::<()>().await,
            }
        }
    }
}

#[cfg(test)]
#[path = "retained_configured_image_wait_tests.rs"]
mod tests;
