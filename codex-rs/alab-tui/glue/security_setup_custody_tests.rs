// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Real prefetch and App admission against gated HTTP and an isolated notice service.

use crate::app::tests::disconnect::serve_reconnect_requests;
use crate::app::tests::make_test_app_with_channels;
use crate::app_event::AppEvent;
use crate::app_server_session::AppServerSession;
use crate::app_server_session::ThreadParamsMode;
use crate::security_setup::Identity;
use crate::security_setup::alab_notices::Delivery;
use crate::security_setup::alab_notices::Fetched;
use crate::security_setup::alab_notices::TestTransport;
use app_test_support::ChatGptAuthFixture;
use app_test_support::write_chatgpt_auth;
use codex_alab_tui::notice_fixtures::Client;
use codex_alab_tui::notice_fixtures::EndOutcome;
use codex_alab_tui::notice_fixtures::FakeAterm;
use codex_alab_tui::notice_fixtures::LaterOutcome;
use codex_alab_tui::notice_fixtures::Mode;
use codex_alab_tui::notice_fixtures::ObserveLaterOutcome;
use codex_alab_tui::notice_fixtures::Options;
use codex_alab_tui::notices::ObservedLater;
use codex_alab_tui::notices::RoutedNotice;
use codex_alab_tui::notices::TAG;
use codex_alab_tui::notices::key;
use codex_alab_tui::notices::later;
use codex_login::AuthCredentialsStoreMode;
use serde_json::json;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::sync::oneshot;
use tokio::sync::watch;
use tokio::task::JoinHandle;

type Result<T> = color_eyre::eyre::Result<T>;

struct Transport {
    client: Client,
    routed: watch::Sender<usize>,
    settlements: AtomicUsize,
    settle_gate: Mutex<Option<(oneshot::Sender<()>, oneshot::Receiver<()>)>>,
}

impl TestTransport for Transport {
    fn observe<'a>(
        &'a self,
        identity: Identity,
    ) -> Pin<Box<dyn Future<Output = Option<ObservedLater>> + Send + 'a>> {
        Box::pin(async move {
            match self
                .client
                .observe_open_later(&key(&identity.account, &identity.user), TAG)
                .await
                .unwrap()
            {
                ObserveLaterOutcome::Open(observed) => Some(observed),
                _ => None,
            }
        })
    }

    fn route<'a>(
        &'a self,
        notice: RoutedNotice,
    ) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>> {
        Box::pin(async move {
            let taken = matches!(
                self.client.notice_later(&later(&notice)).await.unwrap(),
                LaterOutcome::Taken { .. }
            );
            self.routed.send_modify(|count| *count += 1);
            taken
        })
    }

    fn settle<'a>(
        &'a self,
        observed: ObservedLater,
    ) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>> {
        Box::pin(async move {
            self.settlements.fetch_add(1, Ordering::SeqCst);
            let gate = self.settle_gate.lock().unwrap().take();
            if let Some((entered, released)) = gate {
                entered.send(()).unwrap();
                released.await.unwrap();
            }
            matches!(
                self.client.settle_observed_later(&observed).await.unwrap(),
                EndOutcome::Ended
            )
        })
    }
}

async fn bounded<T>(future: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(60), future)
        .await
        .expect("security setup fixture hung")
}

async fn request(listener: &TcpListener) -> Result<TcpStream> {
    let (stream, _) = listener.accept().await?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).await?;
    assert!(line.starts_with("GET /wham/security-setup "));
    loop {
        line.clear();
        if reader.read_line(&mut line).await? == 0 || line == "\r\n" {
            break;
        }
    }
    Ok(reader.into_inner())
}

async fn response(mut stream: TcpStream, status: u16, body: serde_json::Value) -> Result<()> {
    let body = body.to_string();
    stream.write_all(format!(
        "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    ).as_bytes()).await?;
    stream.shutdown().await?;
    Ok(())
}

fn notice() -> RoutedNotice {
    RoutedNotice {
        account: "fixture-account".into(),
        user: "fixture-user".into(),
        title: "Set up account security".into(),
        description: "The latest response still requires setup.".into(),
        url: "https://chatgpt.com/cyber".into(),
    }
}

async fn auth_server() -> Result<(AppServerSession, JoinHandle<Result<Vec<String>>>)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = crate::resolve_remote_addr(&format!("ws://{}", listener.local_addr()?))?;
    let task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await?;
        serve_reconnect_requests(tokio_tungstenite::accept_async(stream).await?, |request| {
            assert_eq!(request.method, "getAuthStatus");
            std::future::ready(Some(json!({"result": {
                "authMethod": "chatgpt", "authToken": "fixture-token",
                "requiresOpenaiAuth": true
            }})))
        })
        .await
    });
    let server = AppServerSession::new(
        crate::backend_id::BackendId::new(),
        crate::connect_remote_app_server(endpoint).await?,
        ThreadParamsMode::Embedded,
    );
    Ok((server, task))
}

#[tokio::test]
async fn old_empty_fetch_cannot_settle_a_newer_accepted_notice() -> Result<()> {
    bounded(async {
        let (mut app, mut events, _operations) = make_test_app_with_channels().await;
        if let Some(home) = app._test_codex_home.take() {
            let _ = home.keep();
        }
        app.config.cli_auth_credentials_store_mode = AuthCredentialsStoreMode::File;
        write_chatgpt_auth(
            &app.config.codex_home,
            ChatGptAuthFixture::new("fixture-token")
                .account_id("fixture-account")
                .chatgpt_user_id("fixture-user"),
            AuthCredentialsStoreMode::File,
        )
        .expect("write synthetic account auth");
        let dir = tempfile::tempdir()?.keep();
        let fake = FakeAterm::start(&dir, Mode::Current).await?;
        let client = Client::connect(fake.endpoint(), Options::default()).await?;
        let (routed, mut routed_events) = watch::channel(0);
        let transport = Arc::new(Transport {
            client,
            routed,
            settlements: AtomicUsize::new(0),
            settle_gate: Mutex::default(),
        });
        let delivery = Delivery {
            transport: Some(transport.clone()),
        };
        let mut initial = notice();
        initial.description = "An earlier response required setup.".into();
        assert!(matches!(
            transport.client.notice_later(&later(&initial)).await?,
            LaterOutcome::Taken { .. }
        ));

        let listener = TcpListener::bind("127.0.0.1:0").await?;
        app.config.chatgpt_base_url = format!("http://{}", listener.local_addr()?);
        let (seen, old_seen) = oneshot::channel();
        let (release, released) = oneshot::channel();
        let http = tokio::spawn(async move {
            let old = request(&listener).await?;
            seen.send(()).unwrap();
            let old = tokio::spawn(async move {
                released.await?;
                response(old, 200, json!({"notice": null})).await
            });
            let new = request(&listener).await?;
            response(
                new,
                200,
                json!({"notice": {
                    "title": notice().title, "description": notice().description,
                    "action": {"label": "Set up security", "url": notice().url}
                }}),
            )
            .await?;
            old.await??;
            Result::Ok(())
        });
        let (mut server, daemon) = auth_server().await?;
        let mut tui = crate::tui::test_support::make_test_tui()?;
        let old = crate::security_setup::prefetch_using(
            &app.config,
            &server,
            app.app_event_tx.clone(),
            app.chat_widget.security_setup_request_id,
            delivery.clone(),
        )
        .unwrap();
        old_seen.await?;
        app.chat_widget.invalidate_security_setup();
        let new = crate::security_setup::prefetch_using(
            &app.config,
            &server,
            app.app_event_tx.clone(),
            app.chat_widget.security_setup_request_id,
            delivery,
        )
        .unwrap();
        while *routed_events.borrow_and_update() == 0 {
            tokio::select! {
                changed = routed_events.changed() => { changed?; }
                event = events.recv() => {
                    app.handle_event(&mut tui, &mut server, event.unwrap()).await?;
                }
            }
        }
        new.await?;
        release.send(()).unwrap();
        old.await?;
        while let Ok(event) = events.try_recv() {
            app.handle_event(&mut tui, &mut server, event).await?;
        }
        http.await??;
        let live = transport
            .client
            .request_lines(&format!("messages live need=later tag={TAG}"))
            .await?;
        server.shutdown().await?;
        daemon.await??;
        transport.client.close();
        assert_eq!(*routed_events.borrow(), 1);
        assert!(live.lines.iter().any(|line| {
            line.contains(&format!("wire.{}", key(&notice().account, &notice().user)))
        }));
        assert_eq!(transport.settlements.load(Ordering::SeqCst), 0);
        Ok(())
    })
    .await
}

#[derive(Clone, Copy)]
enum EmptyCase {
    Current,
    IdentityChanged,
    Restated,
    Refused,
    TimedOut,
    InitiallyAbsent,
}

#[tokio::test]
async fn a_current_empty_fetch_settles_the_observed_notice() -> Result<()> {
    empty_fetch(EmptyCase::Current).await
}

#[tokio::test]
async fn an_empty_fetch_cannot_settle_after_the_saved_identity_changes() -> Result<()> {
    empty_fetch(EmptyCase::IdentityChanged).await
}

#[tokio::test]
async fn an_empty_fetch_cannot_settle_a_notice_restated_during_http() -> Result<()> {
    empty_fetch(EmptyCase::Restated).await
}

#[tokio::test]
async fn a_refused_fetch_preserves_the_notice() -> Result<()> {
    empty_fetch(EmptyCase::Refused).await
}

#[tokio::test]
async fn a_timed_out_fetch_preserves_the_notice() -> Result<()> {
    empty_fetch(EmptyCase::TimedOut).await
}

#[tokio::test]
async fn an_empty_fetch_without_an_observed_entry_cannot_settle_a_new_one() -> Result<()> {
    empty_fetch(EmptyCase::InitiallyAbsent).await
}

async fn empty_fetch(case: EmptyCase) -> Result<()> {
    bounded(async {
        let (mut app, mut events, _operations) = make_test_app_with_channels().await;
        if let Some(home) = app._test_codex_home.take() {
            let _ = home.keep();
        }
        app.config.cli_auth_credentials_store_mode = AuthCredentialsStoreMode::File;
        write_chatgpt_auth(
            &app.config.codex_home,
            ChatGptAuthFixture::new("fixture-token")
                .account_id("fixture-account")
                .chatgpt_user_id("fixture-user"),
            AuthCredentialsStoreMode::File,
        )
        .expect("write synthetic account auth");
        let dir = tempfile::tempdir()?.keep();
        let fake = FakeAterm::start(&dir, Mode::Current).await?;
        let client = Client::connect(fake.endpoint(), Options::default()).await?;
        let (routed, _routed_events) = watch::channel(0);
        let transport = Arc::new(Transport {
            client,
            routed,
            settlements: AtomicUsize::new(0),
            settle_gate: Mutex::default(),
        });
        let delivery = Delivery {
            transport: Some(transport.clone()),
        };
        if !matches!(case, EmptyCase::InitiallyAbsent) {
            assert!(matches!(
                transport.client.notice_later(&later(&notice())).await?,
                LaterOutcome::Taken { .. }
            ));
        }
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        app.config.chatgpt_base_url = format!("http://{}", listener.local_addr()?);
        let (seen, received) = oneshot::channel();
        let (release, released) = oneshot::channel();
        let http = tokio::spawn(async move {
            let stream = request(&listener).await?;
            seen.send(()).unwrap();
            released.await?;
            let status = if matches!(case, EmptyCase::Refused) {
                403
            } else {
                200
            };
            let result = response(stream, status, json!({"notice": null})).await;
            if matches!(case, EmptyCase::TimedOut) {
                // A client that already reached its own deadline may close before writing.
                return Result::Ok(());
            }
            result
        });
        let (mut server, daemon) = auth_server().await?;
        let mut tui = crate::tui::test_support::make_test_tui()?;
        let fetch = crate::security_setup::prefetch_using(
            &app.config,
            &server,
            app.app_event_tx.clone(),
            app.chat_widget.security_setup_request_id,
            delivery,
        )
        .unwrap();
        received.await?;
        match case {
            EmptyCase::IdentityChanged => {
                write_chatgpt_auth(
                    &app.config.codex_home,
                    ChatGptAuthFixture::new("other-fixture-token")
                        .account_id("other-fixture-account")
                        .chatgpt_user_id("other-fixture-user"),
                    AuthCredentialsStoreMode::File,
                )
                .expect("write changed synthetic account auth");
            }
            EmptyCase::Restated | EmptyCase::InitiallyAbsent => {
                let mut restated = notice();
                restated.description = "A newer notice still requires account security.".into();
                assert!(matches!(
                    transport.client.notice_later(&later(&restated)).await?,
                    LaterOutcome::Taken { .. }
                ));
            }
            EmptyCase::TimedOut => {
                // This awaits the actual production deadline, not a fixture latency budget.
                fetch.await?;
                release.send(()).unwrap();
                http.await??;
                while let Ok(event) = events.try_recv() {
                    app.handle_event(&mut tui, &mut server, event).await?;
                }
                let live = transport
                    .client
                    .request_lines(&format!("messages live need=later tag={TAG}"))
                    .await?;
                server.shutdown().await?;
                daemon.await??;
                transport.client.close();
                assert_eq!(transport.settlements.load(Ordering::SeqCst), 0);
                assert_eq!(live.lines.len(), 1);
                return Ok(());
            }
            EmptyCase::Current | EmptyCase::Refused => {}
        }
        release.send(()).unwrap();
        fetch.await?;
        while let Ok(event) = events.try_recv() {
            app.handle_event(&mut tui, &mut server, event).await?;
        }
        http.await??;
        let live = transport
            .client
            .request_lines(&format!("messages live need=later tag={TAG}"))
            .await?;
        server.shutdown().await?;
        daemon.await??;
        transport.client.close();
        let attempts = usize::from(matches!(case, EmptyCase::Current | EmptyCase::Restated));
        assert_eq!(transport.settlements.load(Ordering::SeqCst), attempts);
        assert_eq!(
            live.lines.len(),
            usize::from(!matches!(case, EmptyCase::Current))
        );
        Ok(())
    })
    .await
}

#[tokio::test]
async fn app_awaits_settlement_and_rechecks_identity_for_queued_results() -> Result<()> {
    bounded(async {
        let (mut app, _events, _operations) = make_test_app_with_channels().await;
        if let Some(home) = app._test_codex_home.take() {
            let _ = home.keep();
        }
        app.config.cli_auth_credentials_store_mode = AuthCredentialsStoreMode::File;
        write_chatgpt_auth(
            &app.config.codex_home,
            ChatGptAuthFixture::new("fixture-token")
                .account_id("fixture-account").chatgpt_user_id("fixture-user"),
            AuthCredentialsStoreMode::File,
        ).expect("write synthetic account auth");
        let dir = tempfile::tempdir()?.keep();
        let fake = FakeAterm::start(&dir, Mode::Current).await?;
        let client = Client::connect(fake.endpoint(), Options::default()).await?;
        let (routed, routed_events) = watch::channel(0);
        let (entered, waiting) = oneshot::channel();
        let (release, released) = oneshot::channel();
        let transport = Arc::new(Transport {
            client, routed, settlements: AtomicUsize::new(0),
            settle_gate: Mutex::new(Some((entered, released))),
        });
        let delivery = Delivery { transport: Some(transport.clone()) };
        let identity = Identity { account: notice().account, user: notice().user };
        assert!(matches!(transport.client.notice_later(&later(&notice())).await?, LaterOutcome::Taken { .. }));
        let observed = transport.observe(identity.clone()).await.expect("open initial entry");
        let (mut server, daemon) = auth_server().await?;
        let mut tui = crate::tui::test_support::make_test_tui()?;
        let request_id = app.chat_widget.security_setup_request_id;
        let mut handling = Box::pin(app.handle_event(
            &mut tui, &mut server,
            AppEvent::SecuritySetupLoaded {
                request_id, identity: identity.clone(),
                notice: Fetched::new(None, Some(observed), delivery.clone()),
            },
        ));
        tokio::select! {
            result = &mut handling => { result?; panic!("App returned before its accepted settlement"); }
            entered = waiting => { entered?; }
        }
        // The callback is known to be waiting on its gate. App must still own it,
        // rather than returning and allowing another result to overtake it.
        assert!(futures::poll!(handling.as_mut()).is_pending());
        release.send(()).unwrap();
        handling.await?;
        let current_notice = || crate::security_setup::Notice {
            title: notice().title, description: notice().description,
            action: crate::security_setup::Action {
                label: "Set up security".into(), url: notice().url,
            },
        };
        app.handle_event(&mut tui, &mut server, AppEvent::SecuritySetupLoaded {
            request_id, identity: identity.clone(),
            notice: Fetched::new(Some(current_notice()), None, delivery.clone()),
        }).await?;
        let observed = transport.observe(identity.clone()).await.expect("newly filed entry");
        write_chatgpt_auth(
            &app.config.codex_home,
            ChatGptAuthFixture::new("other-fixture-token")
                .account_id("other-fixture-account").chatgpt_user_id("other-fixture-user"),
            AuthCredentialsStoreMode::File,
        ).expect("write changed synthetic account auth");
        // A queued result can retain the current request id across a saved-login change.
        // Both shapes must revalidate identity at App admission before any native action.
        for notice in [
            Fetched::new(Some(current_notice()), None, delivery.clone()),
            Fetched::new(None, Some(observed), delivery),
        ] {
            app.handle_event(&mut tui, &mut server, AppEvent::SecuritySetupLoaded {
                request_id, identity: identity.clone(), notice,
            }).await?;
        }
        let live = transport.client.request_lines(&format!("messages live need=later tag={TAG}")).await?;
        server.shutdown().await?;
        daemon.await??;
        transport.client.close();
        assert_eq!(transport.settlements.load(Ordering::SeqCst), 1);
        assert_eq!(*routed_events.borrow(), 1);
        assert_eq!(live.lines.len(), 1);
        Ok(())
    }).await
}
