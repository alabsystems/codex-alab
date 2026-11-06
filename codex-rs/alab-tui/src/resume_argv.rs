// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! `resumeArgv`: the launch command line rewritten to resume this session (tui-life §4.3),
//! read off codex's own clap `Command`, never a flag table, so a flag upstream adds tomorrow
//! is kept with no change here.
//!
//! Every element but `resume` and the thread id is verbatim from the launch argv, in its
//! launch position (aterm checks each against the live process's own argv). Dropped: the
//! root `PROMPT` and every other positional, the `-i/--image` values (re-sending them would
//! submit the prompt again) and the session selectors of `resume`/`fork` (`SESSION_ID`,
//! `--last`, `--all`, `--include-non-interactive`). Root flags stay before `resume`, the
//! subcommand's flags after it. A fork resumes the forked thread, which is the primary one.
//! Any other subcommand (`agents`, `exec`, ...) is refused, and so is anything this cannot
//! rebuild exactly: the result is re-parsed with the same `Command` and must carry the very
//! same flag values as the launch.

use clap::Arg;
use clap::ArgMatches;
use clap::Command;
use clap::parser::ValueSource;
use std::ffi::OsStr;
use std::ffi::OsString;

static COMMAND: std::sync::OnceLock<fn() -> Command> = std::sync::OnceLock::new();

/// Keep codex's clap definition (`life::capture_cli`); only the first call counts.
pub fn set_command(command: fn() -> Command) {
    let _ = COMMAND.set(command);
}

/// The clap definition `life::capture_cli` stored.
pub fn command() -> Option<fn() -> Command> {
    COMMAND.get().copied()
}

/// Arguments that select a session (or start one) and are never carried over.
const DROPPED_IDS: [&str; 6] = [
    "prompt",
    "images",
    "session_id",
    "last",
    "all",
    "include_non_interactive",
];

/// Arguments that cannot be resumed in place (`codex resume` rejects `--worktree`, and
/// aterm refuses `--remote`, `--remote-auth-token-env`, and `--worktree`).
const REFUSED_IDS: [&str; 3] = ["remote", "remote_auth_token_env", "worktree"];

/// The subcommands whose session can be resumed; the rewrite always says `resume`.
const RESUMABLE: [&str; 2] = ["resume", "fork"];

/// The argv that resumes `thread_id`, starting with `argv0`; `None` when the launch cannot be
/// rewritten exactly (a refused subcommand, an unparsable or non-UTF-8 argv, a short-flag
/// cluster mixing a dropped flag with a kept one, or any disagreement with clap's own parse).
pub fn rebuild(
    command: Command,
    launch_args: &[OsString],
    argv0: &str,
    thread_id: &str,
) -> Option<Vec<String>> {
    let mut command = command;
    command.build();
    let launched = command.clone().try_get_matches_from(launch_args).ok()?;
    let args = launch_args
        .iter()
        .map(|arg| arg.to_str())
        .collect::<Option<Vec<&str>>>()?;

    let root = split_level(&command, &args, /*start*/ 1, /*root*/ true)?;
    let (sub_flags, sub_name) = match root.subcommand {
        None => (Vec::new(), None),
        Some((name, start)) => {
            let sub = command.find_subcommand(name)?;
            let canonical = sub.get_name();
            if !RESUMABLE.contains(&canonical) {
                return None;
            }
            let level = split_level(sub, &args, start, /*root*/ false)?;
            (level.kept, Some(canonical))
        }
    };
    if launched.subcommand_name() != sub_name {
        return None;
    }

    let mut rebuilt = vec![argv0.to_owned()];
    rebuilt.extend(root.kept.iter().map(|arg| (*arg).to_owned()));
    rebuilt.push("resume".to_owned());
    rebuilt.extend(sub_flags.iter().map(|arg| (*arg).to_owned()));
    rebuilt.push(thread_id.to_owned());

    let resumed = command.clone().try_get_matches_from(&rebuilt).ok()?;
    let (resume_name, resume_matches) = resumed.subcommand()?;
    if resume_name != "resume"
        || resume_matches
            .try_get_raw("session_id")
            .ok()??
            .collect::<Vec<_>>()
            != [OsStr::new(thread_id)]
    {
        return None;
    }
    if !same_flags(&command, &launched, &resumed, /*skip_globals*/ false) {
        return None;
    }
    let launched_sub = launched.subcommand().map(|(_, matches)| matches);
    let resume_command = command.find_subcommand("resume")?;
    let empty = ArgMatches::default();
    if !same_flags(
        resume_command,
        launched_sub.unwrap_or(&empty),
        resume_matches,
        /*skip_globals*/ true,
    ) {
        return None;
    }
    Some(rebuilt)
}

/// Rebuild a durable exec invocation without replaying its prompt or attachments.
/// This preserves exec's output and execution options, rather than converting it
/// to a TUI invocation. It supplies no authority to resume an interrupted turn.
pub fn rebuild_exec(
    mut command: Command,
    launch_args: &[OsString],
    argv0: &str,
    thread_id: &str,
) -> Option<Vec<String>> {
    command.build();
    let launched = command.clone().try_get_matches_from(launch_args).ok()?;
    let (name, launched_exec) = launched.subcommand()?;
    if name != "exec"
        || launched_exec
            .try_get_one::<bool>("ephemeral")
            .ok()
            .flatten()
            == Some(&true)
    {
        return None;
    }
    let args = launch_args
        .iter()
        .map(|arg| arg.to_str())
        .collect::<Option<Vec<_>>>()?;
    let root = split_level(&command, &args, 1, true)?;
    let (_, start) = root.subcommand?;
    let exec = command.find_subcommand("exec")?;
    let level = split_level(exec, &args, start, true)?;
    let mut child_flags = Vec::new();
    if let Some((name, start)) = level.subcommand {
        let child = exec.find_subcommand(name)?;
        if child.get_name() != "resume" || launched_exec.subcommand_name() != Some("resume") {
            return None;
        }
        child_flags = split_level(child, &args, start, false)?.kept;
    } else if launched_exec.subcommand().is_some() {
        return None;
    }
    let mut rebuilt = vec![argv0.to_owned()];
    rebuilt.extend(root.kept.into_iter().map(str::to_owned));
    rebuilt.push("exec".into());
    rebuilt.extend(level.kept.into_iter().map(str::to_owned));
    rebuilt.push("resume".into());
    rebuilt.extend(child_flags.into_iter().map(str::to_owned));
    rebuilt.push(thread_id.to_owned());
    let resumed = command.clone().try_get_matches_from(&rebuilt).ok()?;
    let (name, resumed_exec) = resumed.subcommand()?;
    let (child, resumed_turn) = resumed_exec.subcommand()?;
    if name != "exec"
        || child != "resume"
        || resumed_turn
            .try_get_raw("session_id")
            .ok()??
            .collect::<Vec<_>>()
            != [OsStr::new(thread_id)]
        || !same_flags(&command, &launched, &resumed, false)
        || !same_flags(exec, launched_exec, resumed_exec, false)
    {
        return None;
    }
    let empty = ArgMatches::default();
    if !same_flags(
        exec.find_subcommand("resume")?,
        launched_exec.subcommand().map(|(_, m)| m).unwrap_or(&empty),
        resumed_turn,
        true,
    ) {
        return None;
    }
    Some(rebuilt)
}

struct Level<'a> {
    kept: Vec<&'a str>,
    subcommand: Option<(&'a str, usize)>,
}

/// Walk one command level's tokens from `start`, keeping each flag (with its values) that is
/// not dropped, and stopping at the subcommand (root only).
fn split_level<'a>(
    command: &Command,
    args: &[&'a str],
    start: usize,
    root: bool,
) -> Option<Level<'a>> {
    let mut kept = Vec::new();
    let mut index = start;
    while index < args.len() {
        let token = args[index];
        if token == "--" {
            break;
        }
        if let Some(long) = token.strip_prefix("--") {
            let (name, attached) = match long.split_once('=') {
                Some((name, _)) => (name, true),
                None => (long, false),
            };
            let arg = find_long(command, name)?;
            if refused(arg) {
                return None;
            }
            let end = values_end(arg, args, index + 1, attached);
            if !dropped(arg) {
                kept.extend_from_slice(&args[index..end]);
            }
            index = end;
            continue;
        }
        if let Some(cluster) = token.strip_prefix('-')
            && !cluster.is_empty()
        {
            let mut members = Vec::new();
            let mut end = index + 1;
            for (offset, short) in cluster.char_indices() {
                let arg = find_short(command, short)?;
                if refused(arg) {
                    return None;
                }
                members.push(arg);
                if takes_values(arg) {
                    let attached = offset + short.len_utf8() < cluster.len();
                    end = values_end(arg, args, index + 1, attached);
                    break;
                }
            }
            let dropped_members = members.iter().filter(|arg| dropped(arg)).count();
            if dropped_members == 0 {
                kept.extend_from_slice(&args[index..end]);
            } else if dropped_members != members.len() {
                return None;
            }
            index = end;
            continue;
        }
        if root && command.find_subcommand(token).is_some() {
            return Some(Level {
                kept,
                subcommand: Some((token, index + 1)),
            });
        }
        // A positional (PROMPT, SESSION_ID): never carried over.
        index += 1;
    }
    Some(Level {
        kept,
        subcommand: None,
    })
}

fn find_long<'c>(command: &'c Command, name: &str) -> Option<&'c Arg> {
    command.get_arguments().find(|arg| {
        arg.get_long() == Some(name)
            || arg
                .get_all_aliases()
                .is_some_and(|aliases| aliases.contains(&name))
    })
}

fn find_short(command: &Command, short: char) -> Option<&Arg> {
    command.get_arguments().find(|arg| {
        arg.get_short() == Some(short)
            || arg
                .get_all_short_aliases()
                .is_some_and(|aliases| aliases.contains(&short))
    })
}

fn takes_values(arg: &Arg) -> bool {
    arg.get_action().takes_values()
        && arg
            .get_num_args()
            .is_none_or(|range| range.max_values() > 0)
}

fn dropped(arg: &Arg) -> bool {
    DROPPED_IDS.contains(&arg.get_id().as_str())
}

fn refused(arg: &Arg) -> bool {
    REFUSED_IDS.contains(&arg.get_id().as_str())
}

/// The index after `arg`'s values, which start at `next` unless they are `attached` to the
/// flag token. A one-value flag takes the next token whatever it looks like (clap accepted
/// the launch); a many-value flag takes its minimum values plus tokens until one is an option.
fn values_end(arg: &Arg, args: &[&str], next: usize, attached: bool) -> usize {
    if !takes_values(arg) || attached {
        return next;
    }
    let range = arg.get_num_args();
    let (min, max) = range.map_or((1, 1), |range| (range.min_values(), range.max_values()));
    if min >= 1 && max == 1 {
        return (next + 1).min(args.len());
    }
    let mut end = (next + min).min(args.len());
    while end < args.len()
        && !(args[end].len() > 1 && args[end].starts_with('-'))
        && (end - next) < max
    {
        end += 1;
    }
    end
}

/// Every non-dropped argument given on the command line has the same raw values in both
/// parses, and neither side has one the other lacks. Global arguments (`-c`, `--enable`) are
/// compared once, at the root: clap shows them at every level.
fn same_flags(
    command: &Command,
    launched: &ArgMatches,
    resumed: &ArgMatches,
    skip_globals: bool,
) -> bool {
    let given = |matches: &ArgMatches| -> Vec<(String, Vec<OsString>)> {
        command
            .get_arguments()
            .filter(|arg| !dropped(arg) && !(skip_globals && arg.is_global_set()))
            .filter_map(|arg| {
                let id = arg.get_id().as_str();
                if !matches!(matches.try_contains_id(id), Ok(true))
                    || matches.value_source(id) != Some(ValueSource::CommandLine)
                {
                    return None;
                }
                let values = matches
                    .try_get_raw(id)
                    .ok()
                    .flatten()
                    .map(|values| values.map(OsStr::to_os_string).collect())
                    .unwrap_or_default();
                Some((id.to_owned(), values))
            })
            .collect()
    };
    given(launched) == given(resumed)
}
