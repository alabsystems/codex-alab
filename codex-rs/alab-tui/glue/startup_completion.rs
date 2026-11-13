// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Test-only startup witnesses for the current-thread startup fixtures.
//! The returned scope outlives App::run and holds no app-server client or request handle.

use super::App;
use super::AppExitInfo;
use super::ExitReason;
use crate::app_event::AppEvent;
use crate::app_event::ExitMode;
use crate::legacy_core::config::Config;
use codex_protocol::ThreadId;
use color_eyre::eyre::Result;
use color_eyre::eyre::WrapErr;
use std::cell::RefCell;
use std::future::Future;
use std::marker::PhantomData;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;
use tokio::sync::oneshot;

struct Observer {
    home: PathBuf,
    attached: Option<oneshot::Sender<ThreadId>>,
}

thread_local! {
    static OBSERVER: RefCell<Option<Observer>> = const { RefCell::new(None) };
}

/// One current-thread fixture owns this observation until its scope ends.
/// This is deliberately !Send: another test/runtime cannot adopt the scope.
pub(super) struct StartupWitness {
    home: PathBuf,
    attached: oneshot::Receiver<ThreadId>,
    _same_thread: PhantomData<Rc<()>>,
}

impl StartupWitness {
    fn observe(home: &Path) -> Self {
        let (attached_tx, attached) = oneshot::channel();
        OBSERVER.with(|slot| {
            let mut slot = slot.borrow_mut();
            assert!(slot.is_none(), "one startup observer per fixture thread");
            *slot = Some(Observer {
                home: home.to_path_buf(),
                attached: Some(attached_tx),
            });
        });
        Self {
            home: home.to_path_buf(),
            attached,
            _same_thread: PhantomData,
        }
    }

    pub(super) async fn until_attached_and_exited(
        &mut self,
        run: &mut (impl Future<Output = Result<AppExitInfo>> + Unpin),
    ) -> Result<()> {
        let early_exit = tokio::time::timeout(Duration::from_secs(60), async {
            tokio::select! {
                attached = &mut self.attached => {
                    attached.wrap_err("startup completion observer was dropped")?;
                    Ok::<_, color_eyre::eyre::Report>(None)
                }
                result = &mut *run => {
                    let exit = result?;
                    self.attached.try_recv()
                        .wrap_err("startup exited before attaching the created thread")?;
                    Ok(Some(exit))
                }
            }
        })
        .await
        .wrap_err("startup did not attach its created thread")??;
        let exit = match early_exit {
            Some(exit) => exit,
            None => tokio::time::timeout(Duration::from_secs(60), &mut *run)
                .await
                .wrap_err("attached startup did not finish its normal exit path")??,
        };
        assert!(matches!(exit.exit_reason, ExitReason::UserRequested));
        Ok(())
    }
}

pub(super) fn observe(config: &Config) -> StartupWitness {
    StartupWitness::observe(config.codex_home.as_path())
}

impl Drop for StartupWitness {
    fn drop(&mut self) {
        OBSERVER.with(|slot| {
            if slot
                .borrow()
                .as_ref()
                .is_some_and(|observer| observer.home == self.home)
            {
                slot.replace(None);
            }
        });
    }
}

/// Called only after the real startup-result handler has returned successfully.
pub(super) fn completed(app: &App, started: Option<ThreadId>) {
    if attached(
        app.config.codex_home.as_path(),
        started,
        app.pending_startup_thread_start,
        app.primary_thread_id,
        app.chat_widget.thread_id(),
    ) {
        app.app_event_tx
            .send(AppEvent::Exit(ExitMode::ShutdownFirst));
    }
}

fn attached(
    home: &Path,
    started: Option<ThreadId>,
    pending: bool,
    primary: Option<ThreadId>,
    displayed: Option<ThreadId>,
) -> bool {
    let Some(started) =
        started.filter(|id| !pending && primary == Some(*id) && displayed == Some(*id))
    else {
        return false;
    };
    OBSERVER.with(|slot| {
        if let Some(observer) = slot.borrow_mut().as_mut()
            && observer.home == home
            && let Some(sender) = observer.attached.take()
        {
            return sender.send(started).is_ok();
        }
        false
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::oneshot::error::TryRecvError;

    #[tokio::test(flavor = "current_thread")]
    async fn attached_startup_does_not_hide_a_backend_drain_failure() {
        use crate::app_server_session::AppServerSession;
        use crate::app_server_session::ThreadParamsMode;
        use crate::backend_id::BackendId;
        use codex_app_server_client::AppServerClient;
        use codex_app_server_client::CloudClient;
        use codex_app_server_client::CloudOperation;

        let home = Path::new("/fixture/drain-failure");
        let id = ThreadId::new();
        let mut witness = StartupWitness::observe(home);
        assert!(attached(home, Some(id), false, Some(id), Some(id)));
        let (client, mut commands, _events) = CloudClient::channel(1);
        let session = AppServerSession::new(
            BackendId::new(),
            AppServerClient::Cloud(client),
            ThreadParamsMode::Cloud,
        );
        let mut run = Box::pin(async move {
            session.shutdown().await?;
            Ok(AppExitInfo::fatal("backend drain unexpectedly succeeded"))
        });
        let (result, ()) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(witness.until_attached_and_exited(&mut run), async {
                let request = commands.recv().await.expect("real backend shutdown");
                assert!(matches!(request.operation, CloudOperation::Shutdown));
                request
                    .response
                    .send(Err(std::io::Error::other("injected backend drain failure")))
                    .unwrap();
            })
        })
        .await
        .expect("in-memory backend shutdown must settle");
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("injected backend drain failure")
        );
        drop(witness);
        OBSERVER.with(|slot| assert!(slot.borrow().is_none()));
    }

    #[test]
    fn only_the_successfully_attached_expected_thread_completes_startup() {
        let home = Path::new("/fixture/current");
        let id = ThreadId::new();
        let other = ThreadId::new();
        let mut witness = StartupWitness::observe(home);
        for (source, started, pending, primary, displayed) in [
            (home, Some(id), true, Some(id), Some(id)),
            (home, None, false, Some(id), Some(id)),
            (home, Some(id), false, None, None),
            (home, Some(id), false, Some(other), Some(other)),
            (home, Some(id), false, Some(id), Some(other)),
            (
                Path::new("/fixture/stale"),
                Some(id),
                false,
                Some(id),
                Some(id),
            ),
        ] {
            attached(source, started, pending, primary, displayed);
            assert_eq!(witness.attached.try_recv(), Err(TryRecvError::Empty));
        }
        attached(home, Some(id), false, Some(id), Some(id));
        assert_eq!(witness.attached.try_recv(), Ok(id));
    }
}
