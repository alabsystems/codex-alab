// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use codex_alab_tui::model_policy::Apply;
use codex_alab_tui::model_policy::Outcome;
use codex_alab_tui::model_policy::Selection;
use codex_alab_tui::model_policy::Target;
use codex_alab_tui::model_policy::operation_id;
use codex_alab_tui::record;
use codex_app_server::alab_model_defaults::ResultState;
use codex_app_server::alab_model_defaults::{self};
use futures::StreamExt;
use futures::stream;
use serde::Deserialize;
use serde::Serialize;
use std::fs::OpenOptions;
use std::fs::{self};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    operation: String,
    #[arg(value_parser = ["defaults", "sessions", "defaults,sessions"])]
    scope: String,
}

#[derive(Serialize, Deserialize)]
struct Session {
    target: Target,
    outcome: Outcome,
}
#[derive(Serialize, Deserialize)]
struct Report {
    version: u32,
    operation: String,
    scope: String,
    defaults: Option<Outcome>,
    sessions: Vec<Session>,
    #[serde(default)]
    unidentified: Vec<u32>,
}

fn save<T: Serialize>(path: &Path, report: &T) -> Result<()> {
    let dir = path.parent().context("model receipt directory")?;
    let mut file = tempfile::NamedTempFile::new_in(dir)?;
    serde_json::to_writer(file.as_file_mut(), report)?;
    file.flush()?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    fs::File::open(dir)?.sync_all()?;
    Ok(())
}

async fn snapshot(dir: &Path, args: &Args) -> Result<Report> {
    let mut sessions = Vec::new();
    let mut unidentified = Vec::new();
    if args.scope.contains("sessions") && dir.try_exists()? {
        let mut pids = std::collections::BTreeSet::new();
        for entry in fs::read_dir(dir)? {
            let name = entry?.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if let Some(pid) = name
                .strip_suffix(".model.sock")
                .or_else(|| name.strip_suffix(".json"))
                .and_then(|s| s.parse::<u32>().ok())
            {
                pids.insert(pid);
            }
        }
        let mut identities = stream::iter(pids)
            .map(|pid| async move { (pid, codex_alab_tui::model_policy::identify(dir, pid).await) })
            .buffer_unordered(12);
        while let Some((pid, identified)) = identities.next().await {
            if let Some(target) = identified {
                sessions.push(Session {
                    target,
                    outcome: Outcome::Pending,
                });
            } else if let Some(target) = Target::read(dir, pid) {
                sessions.push(Session {
                    target,
                    outcome: Outcome::Unsupported,
                });
            } else if dir.join(format!("{pid}.model.sock")).try_exists()?
                && codex_alab_tui::identity::of_pid(pid).is_ok()
            {
                unidentified.push(pid);
            }
        }
        sessions.sort_by_key(|s| s.target.pid);
        unidentified.sort_unstable();
    }
    Ok(Report {
        version: 1,
        operation: args.operation.clone(),
        scope: args.scope.clone(),
        defaults: args.scope.contains("defaults").then_some(Outcome::Pending),
        sessions,
        unidentified,
    })
}

async fn run_home(
    home: &Path,
    args: &Args,
    paths: codex_arg0::Arg0DispatchPaths,
) -> Result<serde_json::Value> {
    let receipts = home.join("alab").join("model-policy");
    record::ensure_dir(&receipts)?;
    let path = receipts.join(format!("{}.json", args.operation));
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(receipts.join("apply.lock"))?;
    lock.try_lock()
        .context("a model policy is already being applied")?;
    let dir = record::sessions_dir(home);
    let mut report: Report = if path.try_exists()? {
        serde_json::from_reader(fs::File::open(&path)?)?
    } else {
        let report = snapshot(&dir, args).await?;
        save(&path, &report)?;
        report
    };
    ensure!(
        report.version == 1 && report.operation == args.operation && report.scope == args.scope,
        "model policy operation collision"
    );
    if report.defaults == Some(Outcome::Pending) {
        report.defaults = Some(
            match alab_model_defaults::apply(
                home,
                receipts.join("managed-default").try_exists()?,
                paths,
            )
            .await
            {
                Ok(ResultState::Applied(model)) => Outcome::Applied {
                    model,
                    effort: "ultra".into(),
                },
                Ok(ResultState::Overridden) => Outcome::Overridden,
                Ok(ResultState::Unsupported) => Outcome::Unsupported,
                Err(_) => Outcome::Pending,
            },
        );
        if matches!(report.defaults, Some(Outcome::Applied { .. })) {
            save(&receipts.join("managed-default"), &args.operation)?;
        }
        save(&path, &report)?;
    }
    let targets: Vec<_> = report
        .sessions
        .iter()
        .enumerate()
        .filter(|(_, session)| {
            matches!(
                session.outcome,
                Outcome::Pending | Outcome::Held | Outcome::Unsupported
            )
        })
        .map(|(index, session)| (index, session.target.clone()))
        .collect();
    let mut replies = stream::iter(targets)
        .map(|(index, target)| {
            let dir = &dir;
            async move {
                let result = if codex_alab_tui::identity::of_pid(target.pid)
                    .ok()
                    .as_ref()
                    .map(record::Identity::from)
                    .as_ref()
                    != Some(&target.identity)
                {
                    Outcome::Replaced
                } else {
                    let request = Apply {
                        operation: args.operation.clone(),
                        target,
                        selection: Selection::LatestSolUltra,
                    };
                    codex_alab_tui::model_policy::apply(dir, &request).await
                };
                (index, result)
            }
        })
        .buffer_unordered(12);
    while let Some((index, outcome)) = replies.next().await {
        if report.sessions[index].outcome != outcome {
            report.sessions[index].outcome = outcome;
            save(&path, &report)?;
        }
    }
    // Only the application receipt crosses to aterm. No process environment, account identifiers or drafts.
    let mut sessions: Vec<_> = report
        .sessions
        .iter()
        .map(|s| serde_json::json!({"thread":s.target.thread, "result":s.outcome}))
        .collect();
    sessions.extend(
        report
            .unidentified
            .iter()
            .map(|pid| serde_json::json!({"process":pid,"thread":null,"result":{"status":"held"}})),
    );
    Ok(
        serde_json::json!({"version":1,"operation":report.operation,"scope":report.scope,"defaults":report.defaults,"sessions":sessions}),
    )
}

pub(crate) async fn run(args: Args, paths: codex_arg0::Arg0DispatchPaths) -> Result<()> {
    ensure!(
        operation_id(&args.operation),
        "invalid model policy operation"
    );
    let primary = codex_core::config::find_codex_home()?;
    let dir = primary.join("alab/model-policy");
    record::ensure_dir(&dir)?;
    let roster = dir.join(format!("{}.homes", args.operation));
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(dir.join(format!("{}.lock", args.operation)))?;
    lock.try_lock()
        .context("the model policy is being delivered")?;
    let homes: Vec<codex_alab_tui::model_homes::Home> = if roster.try_exists()? {
        serde_json::from_reader(fs::File::open(&roster)?)?
    } else {
        let homes = codex_alab_tui::model_homes::known(&primary)?;
        save(&roster, &homes)?;
        homes
    };
    let mut homes = homes.iter();
    let first = homes.next().context("empty model home roster")?;
    ensure!(
        first.current() && std::fs::canonicalize(&primary)? == first.path,
        "model receipt home changed"
    );
    let mut receipt = run_home(&first.path, &args, paths.clone()).await?;
    let mut other = Vec::new();
    for home in homes {
        if !home.current() {
            other.push(serde_json::json!({"status":"replaced"}));
            continue;
        }
        match run_home(&home.path, &args, paths.clone()).await {
            Ok(value) => other.push(serde_json::json!({"receipt":value})),
            Err(_) => other.push(serde_json::json!({"status":"pending"})),
        }
    }
    if !other.is_empty() {
        receipt["homes"] = other.into();
    }
    serde_json::to_writer(std::io::stdout(), &receipt)?;
    println!();
    Ok(())
}
