// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! aterm's `RULE_API_BACK` end to end (R4 of the parity review): a network wall's own route,
//! measured on loopback only, holds the wall while the API is down and continues the work at
//! once when it is measured reachable again.

use super::*;

use codex_alab_autopilot::netwatch::NetTiming;
use codex_alab_autopilot::netwatch::Trust;
use codex_alab_core::session::RecoveryWall;

/// A self-signed certificate for `127.0.0.1`: the acceptor that serves it, and the trust that
/// holds it (the probe's only; codex's own requests never trust it).
fn tls() -> (tokio_rustls::TlsAcceptor, Trust) {
    codex_utils_rustls_provider::ensure_rustls_crypto_provider();
    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_string()])
            .unwrap_or_else(|err| panic!("a test certificate: {err}"));
    let der = cert.der().clone();
    let key = rustls_pki_types::PrivateKeyDer::Pkcs8(rustls_pki_types::PrivatePkcs8KeyDer::from(
        signing_key.serialize_der(),
    ));
    let server = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![der.clone()], key)
        .unwrap_or_else(|err| panic!("a test server config: {err}"));
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(der)
        .unwrap_or_else(|err| panic!("the test certificate is a root: {err}"));
    let trusting = Arc::new(
        rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    );
    (
        tokio_rustls::TlsAcceptor::from(Arc::new(server)),
        Arc::new(move || Ok(Arc::clone(&trusting))),
    )
}

/// A person's turn whose API (an `https` base URL on a loopback port nothing listens on) cannot
/// be reached, measured on that route only (the probe's trust holds the test certificate,
/// codex's own requests never do), with the network ladder and the measured outage's hold set
/// past the test so nothing but a measured return can type: the harness once the wall is held
/// for the measured outage, typing nothing, and the hand that serves the port.
async fn walled_unreached() -> (Harness, Serve) {
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .unwrap_or_else(|err| panic!("a loopback port: {err}"))
        .port();
    let hour = Duration::from_secs(3600);
    let timing = Timing {
        min_work: Duration::ZERO,
        net_backoff: vec![hour],
        down_hold: hour,
        ..Timing::default()
    };
    // The mock's one reply is never asked for: every request goes to the loopback port.
    let h = harness_configured(
        Mode::Current,
        vec![responses::sse_response(reply("unused", "never asked for"))],
        &[("human_grace_s", "0")],
        &[],
        Some(0),
        None,
        timing,
        move |config| {
            config.model_provider.base_url = Some(format!("https://127.0.0.1:{port}/v1"));
            // Core's own wait for the network (on by default) retries a connection that
            // failed inside the turn, which then never ends at the wall.
            config
                .features
                .disable(codex_features::Feature::UnboundedConnectionRetries)
                .unwrap_or_else(|err| panic!("core's own network wait off: {err}"));
        },
    )
    .await;
    let (acceptor, trust) = tls();
    let pause = Duration::from_millis(50);
    assert!(h.autopilot.set_net_probe(
        trust,
        NetTiming {
            down_cadence: [pause; 3],
            steady: pause,
            ..NetTiming::default()
        },
    ));
    assert!(
        h.fake
            .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
            .await
    );
    h.test
        .codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "Do the task.".to_string(),
            text_elements: Vec::new(),
        }]))
        .await
        .unwrap_or_else(|err| panic!("start the task: {err}"));
    wait_until("the route is measured", || {
        journal_has(&h, "is measured while the wall stands")
    })
    .await;
    wait_until("the wall is held for the measured outage", || {
        journal_has(
            &h,
            "the API is unreachable; continuing when it is reachable",
        )
    })
    .await;
    assert!(
        !h.fake.acts().iter().any(|act| act.starts_with("api-")),
        "{:?}",
        h.fake.acts()
    );
    (h, Serve { port, acceptor })
}

/// The API's port, served again: a TLS listener that completes every handshake.
struct Serve {
    port: u16,
    acceptor: tokio_rustls::TlsAcceptor,
}

impl Serve {
    async fn serve(self) {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", self.port))
            .await
            .unwrap_or_else(|err| panic!("the API's port again: {err}"));
        let acceptor = self.acceptor;
        tokio::spawn(async move {
            while let Ok((tcp, _)) = listener.accept().await {
                let acceptor = acceptor.clone();
                tokio::spawn(async move {
                    let _ = acceptor.accept(tcp).await;
                });
            }
        });
    }
}

/// The recorded act uses the same composed recovery words as the native harness.
fn api_back_typed() -> String {
    format!(
        "act api-back@v1: typed {:?}",
        codex_alab_autopilot::words::api_back_text("an API error (the API could not be reached)")
    )
}

/// A MEASURED RETURN IS CONTINUED AT ONCE: a person's turn whose API cannot be reached meets
/// the network wall after core's own retries (its unbounded wait for the network off, as a
/// turn that ends at the wall has it); codex-alab measures that route Down and holds the wall,
/// typing nothing; once a TLS listener serves the port, the measure's Up has the thread type
/// `api-back@v1` at once, in aterm's words with its autonomous recovery instruction. NEGATIVE CONTROL: the
/// ladder and the hold are set past the test, so nothing but the measured return can type: no
/// `api-retry@v1` is ever typed, and nothing is before the listener serves.
#[test]
fn an_unreached_api_measured_back_is_continued_at_once() {
    big_stack(|| async {
        let (h, serve) = walled_unreached().await;
        serve.serve().await;
        wait_until("the measured return is continued", || {
            h.fake.acts().iter().any(|act| act == "api-back@v1")
        })
        .await;
        // The act reaches aterm (`harness_act`) before the supervisor journals it
        // (`record_act`): wait for the journal's line, or it is looked for too early.
        wait_until("the measured return's act is journaled", || {
            journal_has(&h, &api_back_typed())
        })
        .await;
        assert!(
            !h.fake.acts().iter().any(|act| act == "api-retry@v1"),
            "{:?}",
            h.fake.acts()
        );
        // The mock's unused reply expects its request: no request was ever the mock's.
        assert!(h.mock.requests().is_empty());
        h._server.reset().await;
    });
}

/// A MEASURED RETURN A HOLD KEEPS BACK STAYS OWED IN THE RECORD AS THE PLAIN RETRY (parity-3
/// review, F1): the API measured back while the owner holds the session, `api-back@v1` is due
/// at once and waits on the hold; the record carries the wall's plain retry meanwhile
/// (`api-retry@v1` in the retry's words, which claim no measure a relaunch has not taken), so
/// a crash at rest under the hold hands aterm's relaunch the wall's try where it carried
/// nothing; its recovery names the wall's route as measured (B2), so that relaunch restores the
/// wall and measures the route again rather than waiting out the hold blind. At the lift the
/// measured return is continued in its own words, and from then on the record owes nothing of
/// the held point: nothing while the act's turn runs, then exactly the plain retry of the wall
/// that turn met again, bound to it. NEGATIVE CONTROL: nothing is typed under the hold, and no
/// `api-retry@v1` ever is.
#[test]
fn a_measured_return_held_back_stays_owed_as_the_plain_retry() {
    big_stack(|| async {
        let (h, serve) = walled_unreached().await;
        h.autopilot
            .supervisor()
            .set_recheck(Duration::from_millis(200));
        h.fake.set_status("hold", "1");
        h.fake.set_effective_quietly(false, "hold");
        h.fake.emit("hold 1 reason=owner origin=cli");
        wait_until("the hold is read", || {
            journal_has(&h, "why=hold: the claim is kept")
        })
        .await;
        serve.serve().await;
        wait_until("the measured return waits on the hold", || {
            journal_has(&h, "not now (aterm says not now: Hold)")
        })
        .await;
        let owed = h
            .shared
            .owed_act()
            .unwrap_or_else(|| panic!("the record carries the held try"));
        assert_eq!(owed.thread, h.thread);
        assert_eq!(owed.rule.as_deref(), Some("api-retry@v1"));
        let held = owed.clone();
        let text = owed.text.unwrap_or_default();
        assert!(
            text.starts_with("Codex reported an API error (the API could not be reached)")
                && !text.contains("reachable again"),
            "the retry's words, no measure: {text}"
        );
        assert_eq!(owed.mode.as_deref(), Some("cruise"));
        assert_eq!(
            h.shared.recovery().map(|recovery| recovery.wall),
            Some(RecoveryWall::Network {
                what: "an API error (the API could not be reached)".to_string(),
                measured: true,
            }),
            "the record's recovery names the measured route"
        );
        assert!(
            !h.fake.acts().iter().any(|act| act.starts_with("api-")),
            "nothing typed under the hold: {:?}",
            h.fake.acts()
        );
        h.fake.set_status("hold", "0");
        h.fake.set_effective_quietly(true, "enabled");
        h.fake.emit("hold 0 reason=owner origin=cli");
        wait_until("the measured return is continued at the lift", || {
            h.fake.acts().iter().any(|act| act == "api-back@v1")
        })
        .await;
        // The act reaches aterm (`harness_act`) before the supervisor journals it
        // (`record_act`): wait for the journal's line, or it is looked for too early.
        wait_until("the measured return's act is journaled", || {
            journal_has(&h, &api_back_typed())
        })
        .await;
        // THE HELD POINT IS ANSWERED (CX-C C4, the end check 84b40cb5c weakened, restored as
        // strong as it is right): the held try names its native point, the failed turn the hold
        // stood at, and from the act on the record owes nothing of that point, whatever its
        // time or words. "Owes nothing" itself is not the law: the typed turn meets the
        // listener's refusal and the wall again, and that later wall's own plain retry is
        // rightly owed at once. So the record owes nothing as the act is journaled (its own
        // turn runs), or that later turn's try; and once that wall is decided, exactly its
        // plain retry, bound to the act's turn.
        let held_turn = held
            .native_origin
            .as_ref()
            .map(|origin| origin.turn_id.clone())
            .unwrap_or_else(|| panic!("the held try names its native point: {held:?}"));
        let act_turn = h
            .autopilot
            .supervisor()
            .journal()
            .iter()
            .find_map(|line| {
                line.strip_prefix(api_back_typed().as_str())?
                    .rsplit_once("; turn ")?
                    .1
                    .strip_suffix(')')
                    .map(str::to_string)
            })
            .unwrap_or_else(|| panic!("the act names its turn"));
        assert_ne!(act_turn, held_turn);
        let of_act = |owed: &codex_alab_core::session::OwedAct| {
            owed.native_origin
                .as_ref()
                .is_some_and(|origin| origin.turn_id == act_turn)
        };
        assert!(
            h.shared.owed_act().is_none_or(|owed| of_act(&owed)),
            "the act left the held point owed: {:?}",
            h.shared.owed_act()
        );
        wait_until("the later wall's own plain retry is owed", || {
            h.shared.owed_act().is_some_and(|owed| of_act(&owed))
        })
        .await;
        let later = h
            .shared
            .owed_act()
            .unwrap_or_else(|| panic!("the later wall's try"));
        assert_eq!(later.rule.as_deref(), Some("api-retry@v1"), "{later:?}");
        assert_eq!(later.thread, h.thread);
        assert!(
            !h.fake.acts().iter().any(|act| act == "api-retry@v1"),
            "{:?}",
            h.fake.acts()
        );
        assert!(h.mock.requests().is_empty());
        h._server.reset().await;
    });
}
