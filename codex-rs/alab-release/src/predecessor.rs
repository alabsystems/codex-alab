// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Execute the old target reader from its frozen source, under its selected compiler.

use super::CompilerStage;
use super::ProtectedArgs;
use super::compile;
use codex_alab_release::Release;
use codex_alab_release::Result;
use codex_alab_release::product::Compiler;
use codex_alab_release::product::Inputs;
use codex_alab_release::product::{self};
use codex_alab_release::protected::Protected;
use codex_alab_release::protected::Worker;
use serde::Deserialize;
use serde::Serialize;
use std::ffi::OsString;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;

const TEST: &str = "upgrade_admission::tests::compiled_target_probe_matches_the_retained_image";

#[derive(clap::Args)]
pub(crate) struct Args {
    #[arg(long)]
    pub(super) repo: PathBuf,
    #[arg(long)]
    pub(super) publisher: PathBuf,
    #[arg(long)]
    pub(super) state: PathBuf,
    #[arg(long)]
    pub(super) cargo_home: PathBuf,
    #[arg(long)]
    pub(super) v8_cache: PathBuf,
    #[arg(long)]
    pub(super) source: String,
    #[arg(long, default_value = "aarch64-apple-darwin")]
    pub(super) target: String,
    #[arg(long)]
    pub(super) candidate: PathBuf,
    #[arg(long)]
    pub(super) candidate_version: String,
    #[arg(long)]
    pub(super) candidate_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Candidate {
    pub(super) executable: PathBuf,
    version: String,
    sha256: String,
}

impl Candidate {
    pub(super) fn check(&self) -> Result<()> {
        if self.version.split('.').count() != 3
            || !self.version.split('.').all(|part| {
                !part.is_empty()
                    && part.len() <= 9
                    && (part.len() == 1 || !part.starts_with('0'))
                    && part.bytes().all(|byte| byte.is_ascii_digit())
            })
            || !self.executable.is_absolute()
            || self.executable.canonicalize()? != self.executable
            || product::file_hash(&self.executable)? != self.sha256
        {
            return Err("predecessor probe candidate identity changed".into());
        }
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Observation {
    schema: u32,
    source: String,
    pub(super) predecessor_version: String,
    target: String,
    candidate: Candidate,
    compiler: Compiler,
    test: String,
    test_binary_sha256: String,
}

/// The caller owns which signed chain supplied the pair. A compiler observation
/// has no signing or index publication privileges.
pub(super) fn run(args: Args) -> Result<Observation> {
    let candidate = Candidate {
        executable: args.candidate.canonicalize()?,
        version: args.candidate_version,
        sha256: args.candidate_sha256,
    };
    candidate.check()?;
    let root = args.state.join(codex_alab_release::source::PREDECESSOR_DIR);
    super::space::predecessor(&args.repo, &root, &args.cargo_home, &args.v8_cache)?;
    let release = Release::prepare(&args.repo, &args.source, &args.target)?;
    super::space::reader_checkout(
        &args.repo,
        &release,
        &root,
        &args.cargo_home,
        &args.v8_cache,
    )?;
    let state = codex_alab_release::source::predecessor_state(&args.state)?;
    let checkout = codex_alab_release::source::Checkout::prepare(&args.repo, &release, &state)?;
    let intent = checkout.campaign.join("intent.json");
    release.save(&intent)?;
    let v8 = codex_alab_release::v8::resolve(&args.repo, &release, &args.v8_cache)?;
    let inputs = Inputs::new(&release, &checkout, &v8)?;
    // Pass a private verified snapshot to the frozen reader. The normal package
    // path can be independently replaced while a long compiler stage runs.
    let snapshot = tempfile::Builder::new()
        .prefix("candidate-")
        .tempdir_in(&checkout.campaign)?;
    let mut pinned = candidate.clone();
    pinned.executable = snapshot.path().join("codex");
    std::fs::copy(&candidate.executable, &pinned.executable)?;
    pinned.check()?;
    let options = arguments(&inputs);
    let roots = ProtectedArgs {
        repo: args.repo,
        cargo_home: args.cargo_home,
        read: vec![],
        write: vec![inputs.target.clone()],
    };
    let completion = compile(
        roots,
        intent,
        args.publisher,
        options,
        Some(CompilerStage::Predecessor {
            inputs,
            candidate: pinned.clone(),
        }),
    )?;
    checkout.check(&release)?;
    v8.check()?;
    candidate.check()?;
    let mut observation = completion
        .predecessor
        .ok_or("missing predecessor test observation")?;
    if observation.source != release.source
        || observation.predecessor_version != release.version
        || observation.target != release.target
        || observation.candidate != pinned
        || observation.test != TEST
    {
        return Err("protected predecessor observation names another test or package".into());
    }
    observation.candidate = candidate;
    Ok(observation)
}

pub(super) fn arguments(inputs: &Inputs) -> Vec<OsString> {
    let mut args: Vec<OsString> = [
        "--unverified",
        "test",
        "--locked",
        "--no-run",
        "--message-format=json",
        "--manifest-path",
    ]
    .into_iter()
    .map(Into::into)
    .collect();
    args.push(inputs.source.join("codex-rs/Cargo.toml").into_os_string());
    args.push("--target-dir".into());
    args.push(inputs.target.clone().into_os_string());
    args.extend(["-p", "codex-alab-tui", "--lib"].map(Into::into));
    args
}

/// Stream output while the child runs. A malformed producer cannot leave a
/// child blocked forever on a full pipe or grow a single diagnostic without bound.
fn output_lines(
    command: &mut Command,
    retire: impl FnOnce(&mut std::process::Child) -> std::io::Result<()>,
    mut handle: impl FnMut(&str) -> Result<()>,
) -> Result<()> {
    let mut child = command.stdout(Stdio::piped()).spawn()?;
    let mut reader = BufReader::new(child.stdout.take().ok_or("missing child stdout")?);
    let output = (|| -> Result<()> {
        let mut line = String::new();
        loop {
            line.clear();
            let n = (&mut reader)
                .take(4 * 1024 * 1024 + 1)
                .read_line(&mut line)?;
            if n == 0 {
                return Ok(());
            }
            if n > 4 * 1024 * 1024 {
                return Err("oversized child output record".into());
            }
            handle(line.trim_end())?;
        }
    })();
    drop(reader);
    if let Err(error) = output {
        // Even a failed shutdown (for example an already closed worker socket)
        // still leaves this owning wait. The invalid record is the stage error.
        let _ = retire(&mut child);
        let _ = child.wait();
        return Err(error);
    }
    let status = child.wait()?;
    crate::stage_status::require_success("protected predecessor command", status)
}

pub(super) fn compile_test(
    worker: &Worker,
    command: &mut Command,
    inputs: &Inputs,
) -> Result<PathBuf> {
    let manifest = inputs.source.join("codex-rs/alab-tui/Cargo.toml");
    let deps = inputs.target.join("debug/deps");
    let mut artifact = None;
    output_lines(
        command,
        |_| worker.retire(),
        |line| {
            if !line.starts_with('{') {
                eprintln!("{line}");
                return Ok(());
            }
            let row: serde_json::Value = serde_json::from_str(line)?;
            if row["reason"] == "compiler-message" {
                if let Some(text) = row["message"]["rendered"].as_str() {
                    eprint!("{text}");
                }
            } else if row["reason"] == "compiler-artifact"
                && row["manifest_path"].as_str() == manifest.to_str()
                && row["target"]["name"] == "codex_alab_tui"
                && row["target"]["kind"] == serde_json::json!(["lib"])
                && row["profile"]["test"] == true
            {
                let binary = Path::new(
                    row["executable"]
                        .as_str()
                        .ok_or("missing test executable")?,
                );
                if binary.parent() != Some(&deps)
                    || binary.canonicalize()? != binary
                    || artifact.replace(binary.to_owned()).is_some()
                {
                    return Err("ambiguous or unexpected predecessor test executable".into());
                }
            }
            Ok(())
        },
    )?;
    artifact.ok_or_else(|| "the frozen source produced no codex-alab-tui library test".into())
}

pub(super) fn observe(
    worker: &Worker,
    protected: &Protected,
    release: &Release,
    candidate: &Candidate,
    binary: &Path,
    compiler: Compiler,
) -> Result<Observation> {
    candidate.check()?;
    let hash = product::file_hash(binary)?;
    let mut cmd = worker.disposable_command(
        protected,
        binary.as_os_str(),
        &[
            "--exact".into(),
            "--ignored".into(),
            "--format=pretty".into(),
            TEST.into(),
        ],
    )?;
    cmd.env("ALAB_TEST_UPGRADE_TARGET", &candidate.executable)
        .env("ALAB_TEST_UPGRADE_TARGET_VERSION", &candidate.version);
    let mut result = HarnessResult::default();
    output_lines(
        &mut cmd,
        |_| worker.retire(),
        |line| {
            eprintln!("{line}");
            result.read(line);
            Ok(())
        },
    )?;
    result.check()?;
    candidate.check()?;
    if product::file_hash(binary)? != hash {
        return Err("predecessor test executable changed during probe".into());
    }
    Ok(Observation {
        schema: 1,
        source: release.source.clone(),
        predecessor_version: release.version.clone(),
        target: release.target.clone(),
        candidate: candidate.clone(),
        compiler,
        test: TEST.into(),
        test_binary_sha256: hash,
    })
}

#[derive(Default)]
struct HarnessResult {
    runs: usize,
    passed: usize,
    summaries: usize,
    unexpected: bool,
}
impl HarnessResult {
    fn read(&mut self, line: &str) {
        if line.starts_with("running ") {
            self.runs += 1;
            self.unexpected |= line != "running 1 test";
        } else if line.starts_with("test result:") {
            self.summaries += 1;
            self.unexpected |=
                !line.starts_with("test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; ");
        } else if line.starts_with("test ") {
            self.passed += 1;
            self.unexpected |= line != format!("test {TEST} ... ok");
        }
    }
    fn check(self) -> Result<()> {
        if self.unexpected || (self.runs, self.passed, self.summaries) != (1, 1, 1) {
            return Err("predecessor's exact target reader did not pass exactly once".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn streamed_predecessor_preserves_its_exit_or_its_own_parse_failure() {
        for (script, expected) in [
            ("exit 2", 1),
            ("exit 75", 75),
            ("kill -KILL $$", 128 + libc::SIGKILL),
        ] {
            let mut command = Command::new("/bin/sh");
            command.args(["-c", script]);
            let error =
                output_lines(&mut command, std::process::Child::kill, |_| Ok(())).unwrap_err();
            assert_eq!(
                crate::stage_status::exit_code(error.as_ref()),
                Some(expected)
            );
        }
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "echo invalid; read -r hold"])
            .stdin(Stdio::piped());
        let mut stopped_pid = None;
        let error = output_lines(
            &mut command,
            |child| {
                stopped_pid = Some(child.id());
                child.kill()?;
                Err(std::io::Error::from(std::io::ErrorKind::NotConnected))
            },
            |_| Err("invalid predecessor record".into()),
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "invalid predecessor record");
        let pid = libc::pid_t::try_from(stopped_pid.unwrap()).unwrap();
        // SAFETY: the pid is the fixture's own child, checked without signalling.
        let waited = unsafe { libc::waitpid(pid, std::ptr::null_mut(), libc::WNOHANG) };
        assert_eq!(waited, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ECHILD)
        );
        assert_eq!(crate::stage_status::exit_code(error.as_ref()), None);
    }

    #[test]
    fn requires_one_named_passing_method_and_its_one_summary() {
        fn accepts(lines: &[&str]) -> bool {
            let mut result = HarnessResult::default();
            for line in lines {
                result.read(line);
            }
            result.check().is_ok()
        }
        let name = format!("test {TEST} ... ok");
        let summary = "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 294 filtered out; finished in 0.03s";
        assert!(accepts(&["running 1 test", &name, summary]));
        assert!(!accepts(&["running 0 tests", summary]));
        assert!(!accepts(&["running 1 test", "test other ... ok", summary]));
        assert!(!accepts(&["running 1 test", &name, &name, summary]));
        assert!(!accepts(&["running 1 test", &name, summary, summary]));
    }
}
