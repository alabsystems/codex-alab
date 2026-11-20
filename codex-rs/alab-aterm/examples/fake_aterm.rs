// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The in-process FAKE aterm (`codex_alab_aterm::fake`) as a process of its own, for a REAL
//! codex binary to talk to (alab/smoke/native.py): never a real aterm.
//!
//! `fake_aterm <rendezvous dir>` lays the fake out in that directory (point the codex
//! process's `XDG_RUNTIME_DIR` at its parent and its `ATERM_PARENT_SESSION_ID` at the sid),
//! prints `SID <sid>`, then reads commands on stdin, one per line:
//!
//! - `leader <pid>`: that pid leads the session's foreground group (its claim is proven);
//! - `policy <key> <value>`: one `[harness]` key (a new generation, announced);
//! - `status <key> <value>`: one raw `@self status` field (`status autopilot land`);
//! - `dump`: one `STATE <json>` line (claim, acts, attention, notices, requests);
//! - `watch-exec`, `exec-next <index>`, `exec-reply <index> <JSON string>`: hold the
//!   exec protocol so the external test controller can inspect and answer the exact pending request;
//! - `quit` (or end of input).

use std::io::BufRead;
use std::io::Write;

use codex_alab_aterm::fake::FakeAterm;
use codex_alab_aterm::fake::Mode;

fn json_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if u32::from(c) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn json_list<'a>(items: impl IntoIterator<Item = &'a String>) -> String {
    let items: Vec<String> = items.into_iter().map(|s| json_str(s)).collect();
    format!("[{}]", items.join(","))
}

fn dump(fake: &FakeAterm) -> String {
    let claim = match fake.claim() {
        Some(c) => format!(
            "{{\"holder\":{},\"proven_pid\":{}}}",
            json_str(&c.holder),
            c.proven_pid
                .map_or_else(|| "null".to_string(), |p| p.to_string())
        ),
        None => "null".to_string(),
    };
    let attention: Vec<String> = fake
        .attention()
        .iter()
        .map(|(k, v)| format!("{}:{}", json_str(k), json_str(v)))
        .collect();
    format!(
        "{{\"claim\":{claim},\"acts\":{},\"attention\":{{{}}},\"notices\":{},\"requests\":{}}}",
        json_list(&fake.acts()),
        attention.join(","),
        json_list(&fake.notices()),
        json_list(&fake.requests()),
    )
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let Some(dir) = std::env::args_os().nth(1) else {
        eprintln!("usage: fake_aterm <rendezvous dir>");
        std::process::exit(2);
    };
    std::fs::create_dir_all(&dir)?;
    let fake = FakeAterm::start(std::path::Path::new(&dir), Mode::Current).await?;
    fake.set_foreground_leader(None);
    println!("SID {}", fake.sid());
    std::io::stdout().flush()?;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let mut exec_index = 0;
    while let Some(line) = rx.recv().await {
        let mut words = line.splitn(3, ' ');
        match (words.next(), words.next(), words.next()) {
            (Some("leader"), Some(pid), None) => {
                fake.set_foreground_leader(pid.parse().ok());
                println!("OK");
            }
            (Some("policy"), Some(key), Some(value)) => {
                fake.set_policy_key(key, value);
                println!("OK");
            }
            // A raw `status` field, such as the session's Autopilot word (`status autopilot
            // land`).
            (Some("status"), Some(key), Some(value)) => {
                fake.set_status(key, value);
                println!("OK");
            }
            (Some("watch-exec"), None, None) => {
                fake.gate("codex-exec-handoff");
                println!("OK");
            }
            (Some("exec-next"), Some(index), None) if index.parse() == Ok(exec_index) => {
                let held = fake.gated_requests();
                println!("REQUEST {}", serde_json::to_string(&held.get(exec_index))?);
            }
            (Some("exec-reply"), Some(index), Some(encoded)) if index.parse() == Ok(exec_index) => {
                let held = fake.gated_requests();
                let reply = serde_json::from_str::<String>(encoded)?;
                if let Some(request) = held.get(exec_index)
                    && !reply.contains(['\r', '\n'])
                {
                    fake.reply_once(request, &format!("{reply}\n"));
                    fake.release_held("codex-exec-handoff");
                    exec_index += 1;
                    println!("OK");
                } else {
                    println!("ERR no pending exec request or invalid reply");
                }
            }
            (Some("dump"), None, None) => println!("STATE {}", dump(&fake)),
            (Some("quit"), None, None) => break,
            _ => println!("ERR {line}"),
        }
        std::io::stdout().flush()?;
    }
    Ok(())
}
