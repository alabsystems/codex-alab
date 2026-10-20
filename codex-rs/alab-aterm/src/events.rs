// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! `subscribe @<sid> events` on a SECOND connection, and the claim watch over it.
//!
//! Why a second connection: a connection-bound claim (no `ttl=`) lasts while its
//! connection keeps serving requests, and a connection that turns into a subscribe stream
//! stops serving them, so aterm clears its claim (`aterm ctl help meta`: "it closes, or
//! turns into a subscribe stream: cleared"; `control.rs` `ConnectionScope`). The claim
//! stays on the request connection; this one only listens.
//!
//! Frames (`aterm ctl help subscribe`; `subscribe.rs` `timeline_wire_kind`): the ack
//! `OK subscribe <n>` then `sub <local> <sid>` per target, then `EVENT <local> <kind>
//! [payload]` and `GAP <local> events-dropped=<n>`. `meta` carries `field=<f>
//! value=<pct|->`; CONTRACT §3 adds `harness-policy gen=<n>`. The `mail` stream adds
//! `MAIL <local> id=<n> off=<n> from=<p> kind=<k>` per delivered row (metadata only) and
//! `GAP <local> mail-dropped=<n>`, which hides nothing of the claim.

use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::net::unix::OwnedReadHalf;
use tokio::net::unix::OwnedWriteHalf;

use crate::Endpoint;
use crate::Error;
use crate::conn;
use crate::wire;

/// One frame of the `events` stream, for this session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// `meta field=<f> value=<v>`: a metadata field moved (`value` `None` = cleared).
    Meta {
        /// The field (`supervisor`, `attention`, `title`, …).
        field: String,
        /// The new value, decoded.
        value: Option<String>,
    },
    /// `harness-policy gen=<n>`: aterm's `[harness]` policy cell moved; re-read it.
    HarnessPolicy {
        /// The new generation, when readable.
        generation: Option<u64>,
    },
    /// A person started giving input after 30 s without.
    Human,
    /// The server's agent verdict moved.
    Agent {
        /// The verdict word.
        word: String,
    },
    /// The session is being retired.
    Closing,
    /// The session exited.
    Exited,
    /// The timeline evicted records this watch had not seen.
    Gap,
    /// `MAIL <local> id=<n> off=<n> from=<p> kind=<k>`: a row was delivered into this
    /// session's inbox (metadata only, never a body).
    Mail {
        id: Option<u64>,
        off: Option<u64>,
        from: Option<String>,
        kind: Option<String>,
    },
    /// `GAP <local> mail-dropped=<n>`: the inbox ring evicted rows this watch had not been
    /// shown. A mail gap, never an events one: the claim's watch saw everything.
    MailGap,
    /// `inbox-seen <id> …`: the handled watermark moved.
    InboxSeen,
    /// Any other frame, kept whole.
    Other(String),
}

/// A live `events` subscription. Dropping it closes the push connection.
pub struct Events {
    reader: BufReader<OwnedReadHalf>,
    // Held, not used: dropping a tokio write half shuts the socket's write side down.
    _writer: OwnedWriteHalf,
    local: String,
}

impl Events {
    /// Open the push connection and subscribe to `endpoint`'s session events; the
    /// handshake is bounded by `deadline`.
    pub async fn open(endpoint: &Endpoint, deadline: Duration) -> Result<Events, Error> {
        // `mail` beside `events` (the inbox badge); an aterm that refuses the stream still
        // gets the claim's watch on `events` alone.
        match Events::open_streams(endpoint, deadline, STREAMS).await {
            Err(Error::Refused(_)) => Events::open_streams(endpoint, deadline, "events").await,
            other => other,
        }
    }

    async fn open_streams(
        endpoint: &Endpoint,
        deadline: Duration,
        streams: &str,
    ) -> Result<Events, Error> {
        let opened = conn::open(&endpoint.sock, deadline).await?;
        let sid = endpoint.sid.clone();
        tokio::time::timeout(deadline, subscribe(opened, sid, streams))
            .await
            .map_err(|_| Error::Unanswered)?
    }

    /// The next frame for this session, or `None` when the stream ended (aterm went away
    /// or switched versions: re-resolve and reconnect).
    pub async fn next(&mut self) -> Option<Event> {
        loop {
            let line = conn::read_line(&mut self.reader).await.ok()??;
            if let Some(event) = parse_frame(&line, &self.local) {
                return Some(event);
            }
        }
    }
}

/// The streams the claim's watch subscribes to.
pub const STREAMS: &str = "events,mail";

async fn subscribe(mut opened: conn::Opened, sid: String, streams: &str) -> Result<Events, Error> {
    let request = format!("subscribe @{sid} {streams}\n");
    opened.writer.write_all(request.as_bytes()).await?;
    opened.writer.flush().await?;
    let ack = conn::read_line(&mut opened.reader)
        .await?
        .ok_or(Error::Closed)?;
    let Some(n) = ack.strip_prefix("OK subscribe ") else {
        return Err(conn::refusal(ack));
    };
    if n.trim() != "1" {
        return Err(Error::Malformed(ack));
    }
    let sub = conn::read_line(&mut opened.reader)
        .await?
        .ok_or(Error::Closed)?;
    let mut toks = sub.split_ascii_whitespace();
    let (Some("sub"), Some(local), Some(acked)) = (toks.next(), toks.next(), toks.next()) else {
        return Err(Error::Malformed(sub));
    };
    if acked != sid {
        return Err(Error::Malformed(sub));
    }
    Ok(Events {
        local: local.to_string(),
        reader: opened.reader,
        _writer: opened.writer,
    })
}

/// A frame for watch `local`, or `None` for one that is not (another target, a timestamp).
pub fn parse_frame(line: &str, local: &str) -> Option<Event> {
    let mut parts = line.splitn(4, ' ');
    let tag = parts.next()?;
    if parts.next()? != local {
        return None;
    }
    match tag {
        "GAP" => {
            let payload = parts.collect::<Vec<_>>().join(" ");
            if payload.starts_with("mail-dropped=") {
                Some(Event::MailGap)
            } else {
                Some(Event::Gap)
            }
        }
        "MAIL" => {
            let payload = parts.collect::<Vec<_>>().join(" ");
            let fields = wire::parse_fields(&payload);
            Some(Event::Mail {
                id: fields.get("id").and_then(|v| v.parse().ok()),
                off: fields.get("off").and_then(|v| v.parse().ok()),
                from: wire::field(&fields, "from"),
                kind: wire::field(&fields, "kind"),
            })
        }
        "EVENT" => {
            let kind = parts.next()?;
            let payload = parts.next().unwrap_or("");
            Some(parse_event(kind, payload, line))
        }
        _ => None,
    }
}

fn parse_event(kind: &str, payload: &str, line: &str) -> Event {
    let fields = wire::parse_fields(payload);
    match kind {
        "meta" => match fields.get("field") {
            Some(field) => Event::Meta {
                field: wire::pct_decode(field),
                value: wire::field(&fields, "value"),
            },
            None => Event::Other(line.to_string()),
        },
        "harness-policy" => Event::HarnessPolicy {
            generation: fields.get("gen").and_then(|g| g.parse().ok()),
        },
        "human" => Event::Human,
        "agent" => Event::Agent {
            word: payload
                .split_ascii_whitespace()
                .next()
                .unwrap_or("")
                .to_string(),
        },
        "inbox-seen" => Event::InboxSeen,
        "closing" => Event::Closing,
        "exited" => Event::Exited,
        _ => Event::Other(line.to_string()),
    }
}

/// What an event means to a claim holder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signal {
    /// Nothing for the claim.
    Nothing,
    /// The claim is not (or may not be) ours any more: go passive now. `by` names the new
    /// holder when one was named. A `GAP` is this too: what it hid cannot be known.
    Lost {
        /// The holder now shown, if any.
        by: Option<String>,
    },
    /// Our own claim was (re)shown.
    Ours,
    /// The policy moved: re-read `harness policy` before the next act.
    PolicyChanged {
        /// Its generation, if readable.
        generation: Option<u64>,
    },
    /// A person is at the keys: wait the grace.
    Human,
    /// The session is ending.
    Ended,
}

/// The claim watch of CONTRACT §2: passive on any `meta-change field=supervisor` not
/// naming `holder`.
pub fn claim_signal(event: &Event, holder: &str) -> Signal {
    match event {
        Event::Meta { field, value } if field == "supervisor" => match value {
            Some(v) if v == holder => Signal::Ours,
            other => Signal::Lost { by: other.clone() },
        },
        Event::Gap => Signal::Lost { by: None },
        Event::HarnessPolicy { generation } => Signal::PolicyChanged {
            generation: *generation,
        },
        Event::Human => Signal::Human,
        Event::Closing | Event::Exited => Signal::Ended,
        _ => Signal::Nothing,
    }
}
