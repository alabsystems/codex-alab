// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Adoption after actual SDK settlement. The operation identifies a native
//! transfer; neither DATA nor a holder string grants the new socket its claim.
use crate::Client;
use crate::Endpoint;
use crate::Error;
use crate::Options;
use std::collections::BTreeMap;
use std::sync::LazyLock;
use tokio::sync::watch;

#[derive(Clone, Debug)]
pub(crate) struct Receipt {
    pub(crate) operation: String,
    pub(crate) primary: String,
    pub(crate) build: u64,
}
impl Receipt {
    pub(crate) async fn check(&self, client: &Client) -> Result<bool, Error> {
        let result = tokio::time::timeout(crate::client::CLAIM_TIMEOUT, async {
            loop {
                match self.check_once(client).await? {
                    Some(adopted) => return Ok(adopted),
                    None => tokio::time::sleep(std::time::Duration::from_millis(50)).await,
                }
            }
        })
        .await;
        match result {
            Ok(result) => result,
            Err(_) => {
                client.close();
                Err(Error::Unanswered)
            }
        }
    }

    async fn check_once(&self, client: &Client) -> Result<Option<bool>, Error> {
        let reply = client
            .request(&format!(
                "@{} native-act adopt source-exec operation={}",
                client.endpoint().sid,
                self.operation,
            ))
            .await?;
        if reply.is_err() {
            return Err(crate::conn::refusal(reply.status));
        }
        if reply.status.len() > 4096 {
            return Ok(Some(false));
        }
        let Some(body) = reply.status.strip_prefix("OK native source-exec ") else {
            return Ok(Some(false));
        };
        let mut fields = BTreeMap::new();
        for word in body.split_ascii_whitespace() {
            let Some((key, value)) = word.split_once('=') else {
                return Ok(Some(false));
            };
            if value.is_empty() || fields.insert(key, value).is_some() {
                return Ok(Some(false));
            }
        }
        if fields.len() != 4
            || fields.get("operation").copied() != Some(self.operation.as_str())
            || fields.get("primary").copied() != Some(self.primary.as_str())
            || fields.get("build").copied() != Some(self.build.to_string().as_str())
        {
            return Ok(Some(false));
        }
        Ok(match fields.get("state").copied() {
            Some("adopted") => Some(true),
            Some("waiting") => None,
            _ => Some(false),
        })
    }
}

#[derive(Clone)]
struct Settled {
    endpoint: Endpoint,
    receipt: Receipt,
}
static SETTLED: LazyLock<watch::Sender<Option<Settled>>> = LazyLock::new(|| watch::channel(None).0);

pub(crate) fn publish(endpoint: &Endpoint, receipt: Receipt) {
    SETTLED.send_replace(Some(Settled {
        endpoint: endpoint.clone(),
        receipt,
    }));
}

/// Wait for settlement on the actual SDK stream, then connect independently.
/// The receipt identifies a native operation; it never grants this socket a claim.
pub(crate) async fn settled_client(endpoint: Endpoint, options: Options) -> Result<Client, Error> {
    let mut ready = SETTLED.subscribe();
    let settled = loop {
        if let Some(settled) = ready.borrow_and_update().clone() {
            break settled;
        }
        ready.changed().await.map_err(|_| Error::Closed)?;
    };
    if settled.endpoint != endpoint {
        return Err(Error::Closed);
    }
    let client = Client::connect(endpoint, options).await?;
    client.adopt_runtime_claim(settled.receipt);
    Ok(client)
}
