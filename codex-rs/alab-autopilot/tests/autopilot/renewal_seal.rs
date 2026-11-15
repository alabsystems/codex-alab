// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A LOGIN HANDOFF'S SEAL ENDS WITH THE FOLLOW THAT LETS ITS POINT GO (review of 06dfcad367,
//! finding 1), against alab-aterm's in-process fake aterm and a scratch ChatGPT login codex's own
//! login code wrote (fake keys only, never a real credential); the model is a loopback mock.

use super::*;
use codex_alab_autopilot::supervisor::Ready;

use super::login_fixtures as fixtures;

/// The reason an act's checks give while a handoff has sealed the claim.
const SEALED: &str = "the claim stands sealed for an account handoff";
/// The journal's line when a follow lets a handoff's point go with its seal standing.
const SEAL_LET_GO: &str =
    "the account handoff's follow let its point go; the claim's seal for it ends";

/// THE LOGIN COMES BACK IN ITS OWN FILE WHILE A RENEWAL IT ASKED FOR STANDS SEALED. A turn at
/// a refused subscription login asks the host to renew it (`logins=renew`): the host answers
/// `ready`, codex-alab seals the claim, binds the receipt and sends `commit`, and the host goes on
/// answering `waiting`. The same login is then signed in again in its file. The follow lets the
/// point go to the login's own return (a receipt nobody accepted yet is not this path's once the
/// file renewed itself), though the record's intent is still bound to the receipt, and the
/// same-file recovery's admission has to re-confirm the claim. With the seal left standing that
/// admission waited "sealed" at every try, and the original request never went again. It ends
/// with the follow, the journal says so, and the original request goes again, once, on the
/// renewed login.
#[test]
fn a_login_handoff_sealed_then_signed_in_again_in_its_file_recovers_the_original_request() {
    big_stack(|| async {
        let (login, identity, refresh) = subscription_fixture("sealed").await;
        let TestLogin::Manager { manager, home } = login else {
            panic!("the subscription fixture is a file-backed login")
        };
        let refused = || {
            wiremock::ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "error": {"message": "fixture sign-in expired",
                    "type": "invalid_request_error", "code": "invalid_api_key"}
            }))
        };
        let h = harness_login(
            Mode::Current,
            vec![
                refused(),
                refused(),
                responses::sse_response(reply("recovered", "Recovered original work.")),
            ],
            &[
                ("continue", "false"),
                ("human_grace_s", "0"),
                ("logins", "renew"),
            ],
            &[],
            Some(0),
            Some(TestLogin::Manager {
                manager,
                home: Arc::clone(&home),
            }),
            Timing {
                min_work: Duration::ZERO,
                login_look: Duration::from_millis(25),
                ..Timing::default()
            },
            |_| {},
            None,
        )
        .await;
        h.fake.gate("codex-renewal");
        h.test
            .submit_turn("Recover this original request.")
            .await
            .unwrap_or_else(|err| panic!("the refused login's turn: {err}"));
        let wall = h
            .shared
            .latest_turn(&h.thread)
            .unwrap_or_else(|| panic!("the walled turn has no id"));
        assert_eq!(h.mock.requests().len(), 2, "core spent the refused login");
        let id = "0123456789abcdef0123456789abcdef";
        let receipt = format!(
            "id={id} thread={} wall={wall} account={} user={}",
            h.thread, identity.account, identity.user
        );
        let prepare = format!(
            "@{} codex-renewal prepare thread={} wall={wall} account={} user={}",
            h.fake.sid(),
            h.thread,
            identity.account,
            identity.user
        );
        wait_until("the renewal is asked for", || {
            !h.fake.gated_requests().is_empty()
        })
        .await;
        assert_eq!(h.fake.gated_requests(), std::slice::from_ref(&prepare));
        h.fake.reply_once(
            &prepare,
            &format!("OK codex-renewal state=ready {receipt}\n"),
        );
        h.fake.reply_always(
            &format!("@{} codex-renewal status id={id}", h.fake.sid()),
            &format!("OK codex-renewal state=waiting {receipt}\n"),
        );
        h.fake.open_gate("codex-renewal");
        let commit = format!("@{} codex-renewal commit id={id}", h.fake.sid());
        wait_until("the bound receipt is committed", || {
            h.shared
                .owed_act()
                .and_then(|owed| owed.renewal)
                .and_then(|renewal| renewal.receipt)
                .as_deref()
                == Some(id)
                && h.fake.requests().contains(&commit)
        })
        .await;
        let sup = h.autopilot.supervisor();
        let ready = sup.ready().await;
        assert!(
            matches!(&ready, Ready::Wait { why, .. } if why == SEALED),
            "POSITIVE CONTROL: the committed renewal has sealed the claim: {ready:?}"
        );
        assert_eq!(h.mock.requests().len(), 2);
        // The same login, a fresh credential, in its own file.
        fixtures::chatgpt(
            home.path(),
            "sealed-user",
            "sealed-account",
            "sealed-renewed",
            Some(&refresh.uri()),
        );
        wait_until(
            "the original request goes again on the renewed login",
            || h.mock.requests().len() == 3,
        )
        .await;
        assert!(journal_has(&h, SEAL_LET_GO), "{:#?}", sup.journal());
        let requests = h.mock.requests();
        assert_eq!(
            requests[0].body_json()["input"],
            requests[2].body_json()["input"],
            "the failed request's own input goes again"
        );
        h.test.codex.shutdown_and_wait().await.expect("shutdown");
    });
}
