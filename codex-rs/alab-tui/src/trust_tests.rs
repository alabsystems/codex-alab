// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The folder-trust answer: aterm's root spelling, the decision and its why, each refusal
//! beside the case it refuses, and the policy read against a fake aterm.

use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use codex_alab_aterm::HarnessPolicy;
use codex_alab_aterm::PolicyAnswer;
use codex_alab_aterm::fake::FakeAterm;
use codex_alab_aterm::fake::Mode;
use codex_alab_aterm::policy::ApproveLevel;
use pretty_assertions::assert_eq;

use crate::trust::Gate;
use crate::trust::Grant;
use crate::trust::Verdict;
use crate::trust::abs_components;
use crate::trust::decide;
use crate::trust::judge_settled_with;
use crate::trust::judge_with;
use crate::trust::read_policy;
use crate::trust::real_components;
use crate::trust::roots_from_config;

const HOME: &str = "/Users/_owner";
const UID: u32 = 502;

fn policy(line: &str) -> HarnessPolicy {
    match PolicyAnswer::parse(line) {
        PolicyAnswer::Supported(policy) => policy,
        PolicyAnswer::Unsupported(line) => panic!("unsupported: {line}"),
    }
}

fn on(approve: &str, roots: &str) -> HarnessPolicy {
    policy(&format!(
        "OK gen=3 effective=on why=enabled approve={approve} trust_roots={roots}"
    ))
}

/// A launch in `folder`, whose trust codex writes for `target`.
fn gate(folder: &str, target: &str) -> Gate {
    Gate {
        embedded: true,
        restricted: false,
        launch_cwd: Some(PathBuf::from(folder)),
        folder: PathBuf::from(folder),
        trust_target: PathBuf::from(target),
    }
}

/// No symlinks: every path leads where it is written.
fn lexical(comps: &[String]) -> Option<Vec<String>> {
    Some(comps.to_vec())
}

fn verdict(gate: &Gate, policy: &HarnessPolicy) -> Verdict {
    decide(gate, policy, Some(Path::new(HOME)), UID, lexical)
}

fn shown(verdict: Verdict) -> String {
    match verdict {
        Verdict::Show(why) => why,
        Verdict::Trust(grant) => panic!("trusted: {grant:?}"),
    }
}

fn trusted(verdict: Verdict) -> Grant {
    match verdict {
        Verdict::Trust(grant) => grant,
        Verdict::Show(why) => panic!("shown: {why}"),
    }
}

/// aterm's own spelling test (`trust_roots_are_read_from_the_config_spelling`): `~/` is home,
/// a trailing `*` any suffix of the last component; `..`, a glob elsewhere, a relative path
/// and a bare `*` are dropped. `/tmp/claude-*` is this user's own.
#[test]
fn roots_are_read_as_aterm_reads_them() {
    let roots = roots_from_config(
        &[
            "~/proj*",
            "~",
            "/private/tmp/claude-*",
            "/opt/x/",
            "~/../etc",
            "/a/*/b",
            "/a/b*c",
            "relative",
            "*",
            "/",
            "~/src/*",
        ]
        .map(String::from),
        Some(Path::new(HOME)),
        UID,
    );
    let labels: Vec<String> = roots.iter().map(crate::trust::TrustRoot::label).collect();
    assert_eq!(
        labels,
        [
            "/Users/_owner/proj*",
            "/Users/_owner",
            "/private/tmp/claude-502",
            "/opt/x"
        ]
    );
    assert_eq!(roots[0].spec(), "~/proj*");
    let held = |path: &str| {
        let comps = abs_components(path).expect("absolute");
        roots.iter().any(|root| root.holds(&comps))
    };
    assert!(held("/Users/_owner/proj-b2/sub") && held("/private/tmp/claude-502/x"));
    assert!(!held("/private/tmp/claude-503/x") && !held("/private/tmp/other"));
    assert!(!held("/opt/xy") && !held("/etc/hosts"));
    // No home: the `~` specs are dropped, never read against `/`.
    let homeless = roots_from_config(&["~".into(), "~/x".into()], None, UID);
    assert_eq!(homeless, Vec::new());
}

#[test]
fn the_launch_folder_under_a_root_is_trusted_at_safe_and_all() {
    let launch = gate("/Users/_owner/Developer/app", "/Users/_owner/Developer/app");
    for approve in ["safe", "all"] {
        let grant = trusted(verdict(&launch, &on(approve, "~/Developer")));
        assert_eq!(grant.root.as_deref(), Some("~/Developer"));
        assert_eq!(grant.rule(), crate::trust::RULE);
        assert_eq!(
            grant.trust_target,
            PathBuf::from("/Users/_owner/Developer/app")
        );
        assert_eq!(grant.approve.word(), approve);
    }
    let notice = trusted(verdict(&launch, &on("all", "~/Developer"))).notice();
    assert_eq!(
        notice.message,
        "Trusted /Users/_owner/Developer/app under aterm's trust root ~/Developer"
    );
    assert!(notice.hint.contains("approve=all"), "{}", notice.hint);
}

/// approve=none never trusts; an unknown or missing word is none.
#[test]
fn approve_none_shows_the_screen() {
    let launch = gate("/Users/_owner/Developer/app", "/Users/_owner/Developer/app");
    assert!(shown(verdict(&launch, &on("none", "~/Developer"))).contains("approve=none"));
    let missing = policy("OK gen=3 effective=on why=enabled trust_roots=~/Developer");
    assert_eq!(missing.approve_level(), ApproveLevel::None);
    assert!(shown(verdict(&launch, &missing)).contains("approve=none"));
}

/// effective=off (a hold, Autopilot off, disabled, ...) never trusts; the same policy on does.
#[test]
fn effective_off_shows_the_screen() {
    let launch = gate("/Users/_owner/Developer/app", "/Users/_owner/Developer/app");
    let held = policy("OK gen=3 effective=off why=hold approve=all trust_roots=~/Developer");
    let why = shown(verdict(&launch, &held));
    assert!(why.contains("effective=off why=hold"), "{why}");
    let autopilot_off =
        policy("OK gen=3 effective=off why=autopilot-off approve=all trust_roots=~/Developer");
    assert!(shown(verdict(&launch, &autopilot_off)).contains("why=autopilot-off"));
    trusted(verdict(&launch, &on("all", "~/Developer")));
}

/// Under approve=safe no root, or none aterm would take, never trusts.
#[test]
fn no_trust_root_shows_the_screen_under_safe() {
    let launch = gate("/Users/_owner/Developer/app", "/Users/_owner/Developer/app");
    assert!(shown(verdict(&launch, &on("safe", "-"))).contains("no trust root"));
    let elsewhere = shown(verdict(&launch, &on("safe", "/opt")));
    assert!(elsewhere.contains("under no trust root"), "{elsewhere}");
}

/// Under approve=all (aterm's default) the launch folder is trusted under no root, as aterm's
/// full power presses "Trust and continue" for any folder (`trust-any@v1`); the same folder
/// under safe is the screen (the negative control).
#[test]
fn full_power_trusts_the_launch_folder_under_no_root() {
    let sub = gate(
        "/Users/_owner/Developer/repo/crates/x",
        "/Users/_owner/Developer/repo",
    );
    for roots in ["-", "/opt", "~/Developer/repo/crates"] {
        let grant = trusted(verdict(&sub, &on("all", roots)));
        assert_eq!(grant.root, None, "{roots}");
        assert_eq!(grant.rule(), crate::trust::RULE_ANY);
        assert_eq!(
            grant.trust_target,
            PathBuf::from("/Users/_owner/Developer/repo")
        );
        shown(verdict(&sub, &on("safe", roots)));
    }
    let notice = trusted(verdict(&sub, &on("all", "-"))).notice();
    assert_eq!(
        notice.message,
        "Trusted /Users/_owner/Developer/repo (the repository root of \
         /Users/_owner/Developer/repo/crates/x) under aterm's full power (approve=all)"
    );
    // A rooted folder keeps the root's rule under all too.
    let grant = trusted(verdict(&sub, &on("all", "~/Developer")));
    assert_eq!(grant.rule(), crate::trust::RULE);
    // Full power never reaches past the checks before it: untrust, the launch folder, a hold,
    // approve=none.
    let mut untrusted = sub.clone();
    untrusted.restricted = true;
    shown(verdict(&untrusted, &on("all", "-")));
    let mut other = sub.clone();
    other.launch_cwd = Some(PathBuf::from("/Users/_owner/Developer/elsewhere"));
    shown(verdict(&other, &on("all", "-")));
    shown(verdict(
        &sub,
        &policy("OK gen=3 effective=off why=hold approve=all trust_roots=-"),
    ));
    shown(verdict(&sub, &on("none", "-")));
}

/// `..` and a mid-path glob are dropped, never widened into a root that would hold the folder.
#[test]
fn widening_specs_are_dropped() {
    for (spec, folder) in [
        ("~/../etc", "/etc/app"),
        ("~/a/../..", "/Users/_app"),
        ("/Users/*/Developer", "/Users/_owner/Developer"),
        ("/Users/_owner/Dev*/app", "/Users/_owner/Developer/app"),
        ("/Users/_owner/*", "/Users/_owner/x"),
    ] {
        let launch = gate(folder, folder);
        let why = shown(verdict(&launch, &on("safe", spec)));
        assert!(why.contains("no trust root"), "{spec}: {why}");
    }
    // Positive control: the prefix glob in the last place holds.
    let launch = gate("/Users/_owner/Developer/app", "/Users/_owner/Developer/app");
    trusted(verdict(&launch, &on("safe", "~/Dev*")));
}

/// codex's trust in a Git subdirectory is written for the repository ROOT: a root that holds
/// the folder but not the repository root shows the screen.
#[test]
fn a_root_must_hold_the_repository_root_too() {
    let sub = gate(
        "/Users/_owner/Developer/repo/crates/x",
        "/Users/_owner/Developer/repo",
    );
    let why = shown(verdict(&sub, &on("safe", "~/Developer/repo/crates")));
    assert!(
        why.contains("written for /Users/_owner/Developer/repo"),
        "{why}"
    );
    let grant = trusted(verdict(&sub, &on("safe", "~/Developer/repo")));
    assert_eq!(
        grant.notice().message,
        "Trusted /Users/_owner/Developer/repo (the repository root of \
         /Users/_owner/Developer/repo/crates/x) under aterm's trust root ~/Developer/repo"
    );
}

/// A person's explicit untrust stands, under any policy.
#[test]
fn an_untrusted_project_shows_the_screen() {
    let mut untrusted = gate("/Users/_owner/Developer/app", "/Users/_owner/Developer/app");
    untrusted.restricted = true;
    assert!(shown(verdict(&untrusted, &on("all", "~/Developer"))).contains("untrusted"));
}

/// Only the launch folder: a `-C` folder, a resumed task's other folder, or no launch record
/// shows the screen.
#[test]
fn only_the_launch_folder_is_trusted() {
    let mut other = gate("/Users/_owner/Developer/b", "/Users/_owner/Developer/b");
    other.launch_cwd = Some(PathBuf::from("/Users/_owner/Developer/a"));
    let why = shown(verdict(&other, &on("all", "~/Developer")));
    assert!(why.contains("not this process's launch folder"), "{why}");
    other.launch_cwd = None;
    assert!(shown(verdict(&other, &on("all", "~/Developer"))).contains("no launch directory"));
    // Spelled with an empty or `.` component, it is the same folder.
    other.launch_cwd = Some(PathBuf::from("/Users/_owner//Developer/./b/"));
    trusted(verdict(&other, &on("all", "~/Developer")));
}

#[test]
fn a_connected_app_server_shows_the_screen() {
    let mut connected = gate("/Users/_owner/Developer/app", "/Users/_owner/Developer/app");
    connected.embedded = false;
    assert!(shown(verdict(&connected, &on("all", "~/Developer"))).contains("connected"));
}

/// A path with `..` or a pattern character is never judged.
#[test]
fn an_unreadable_folder_shows_the_screen() {
    for folder in ["/Users/_owner/Developer/../x", "/Users/_owner/Developer/a*"] {
        let launch = gate(folder, folder);
        shown(verdict(&launch, &on("all", "~")));
    }
}

/// A folder under a root whose symlinks lead out of every root shows the screen, judged both
/// as written and where it leads (a modelled link, then a real one on disk).
#[test]
fn a_symlink_out_of_a_root_shows_the_screen() {
    let launch = gate(
        "/Users/_owner/Developer/link",
        "/Users/_owner/Developer/link",
    );
    let out = |comps: &[String]| {
        Some(if comps.last().is_some_and(|c| c == "link") {
            vec!["etc".to_string()]
        } else {
            comps.to_vec()
        })
    };
    let why = shown(decide(
        &launch,
        &on("safe", "~/Developer"),
        Some(Path::new(HOME)),
        UID,
        out,
    ));
    assert!(why.contains("leads to /etc, under no trust root"), "{why}");
    // A link that cannot be resolved is no answer either.
    let dangling = |_: &[String]| None;
    let why = shown(decide(
        &launch,
        &on("safe", "~/Developer"),
        Some(Path::new(HOME)),
        UID,
        dangling,
    ));
    assert!(why.contains("cannot be resolved"), "{why}");

    let scratch = tempfile::tempdir().expect("scratch");
    let base = std::fs::canonicalize(scratch.path()).expect("canonical scratch");
    let root = base.join("root");
    let outside = base.join("outside");
    std::fs::create_dir_all(root.join("inner")).expect("inner");
    std::fs::create_dir_all(&outside).expect("outside");
    std::os::unix::fs::symlink(&outside, root.join("link")).expect("symlink");
    let root_spec = root.to_str().expect("utf-8").to_string();
    let roots = on("safe", &root_spec);
    let disk = |folder: &Path| {
        decide(
            &Gate {
                embedded: true,
                restricted: false,
                launch_cwd: Some(folder.to_path_buf()),
                folder: folder.to_path_buf(),
                trust_target: folder.to_path_buf(),
            },
            &roots,
            None,
            UID,
            real_components,
        )
    };
    let why = shown(disk(&root.join("link")));
    assert!(why.contains("under no trust root"), "{why}");
    trusted(disk(&root.join("inner")));
    // A link that stays inside a root is fine.
    std::os::unix::fs::symlink(root.join("inner"), root.join("near")).expect("symlink");
    trusted(disk(&root.join("near")));
}

async fn fake(mode: Mode) -> (tempfile::TempDir, FakeAterm) {
    let dir = tempfile::tempdir().expect("scratch dir");
    let fake = FakeAterm::start(dir.path(), mode)
        .await
        .expect("fake aterm");
    (dir, fake)
}

/// The policy read: a current aterm's answer, an older aterm's refusal, a stalled one bounded
/// by the deadline, and no aterm at all.
#[tokio::test]
async fn the_policy_read_fails_closed() {
    let (_dir, current) = fake(Mode::Current).await;
    current.set_policy_key("trust_roots", "~/Developer");
    let read = read_policy(current.endpoint(), Duration::from_secs(5))
        .await
        .expect("policy");
    assert_eq!(read.approve_level(), ApproveLevel::All);
    assert_eq!(read.trust_roots(), vec!["~/Developer"]);
    // Read only: no claim was made, and the connection was given back.
    assert_eq!(current.claim(), None);

    let (_old_dir, old) = fake(Mode::Old).await;
    let why = read_policy(old.endpoint(), Duration::from_secs(5))
        .await
        .expect_err("an older aterm");
    assert!(why.contains("no harness policy"), "{why}");

    current.stall("harness");
    let why = read_policy(current.endpoint(), Duration::from_millis(200))
        .await
        .expect_err("stalled");
    assert!(
        why.contains("did not answer") || why.contains("in time"),
        "{why}"
    );

    let launch = gate("/Users/_owner/Developer/app", "/Users/_owner/Developer/app");
    let why = shown(judge_with(&launch, None, Some(Path::new(HOME)), UID).await);
    assert!(why.contains("not inside an aterm session"), "{why}");
}

/// End to end against the fake: trusted under its policy, the screen under a hold.
#[tokio::test]
async fn judge_reads_the_sessions_policy() {
    let (_dir, aterm) = fake(Mode::Current).await;
    aterm.set_policy_key("trust_roots", "~/Developer");
    let launch = gate("/Users/_owner/Developer/app", "/Users/_owner/Developer/app");
    let grant =
        trusted(judge_with(&launch, Some(aterm.endpoint()), Some(Path::new(HOME)), UID).await);
    assert_eq!(grant.root.as_deref(), Some("~/Developer"));
    aterm.set_effective(false, "hold");
    let why = shown(judge_with(&launch, Some(aterm.endpoint()), Some(Path::new(HOME)), UID).await);
    assert!(why.contains("why=hold"), "{why}");
    // A launch-folder refusal is decided before anything connects.
    let before = aterm.requests().len();
    let mut other = launch.clone();
    other.launch_cwd = Some(PathBuf::from("/Users/_owner"));
    shown(judge_with(&other, Some(aterm.endpoint()), Some(Path::new(HOME)), UID).await);
    assert_eq!(aterm.requests().len(), before);
}

#[test]
fn the_notice_is_taken_once() {
    codex_alab_core::activate();
    let launch = gate("/Users/_owner/Developer/app", "/Users/_owner/Developer/app");
    let grant = trusted(verdict(&launch, &on("safe", "~/Developer")));
    crate::trust::note_trusted(&grant);
    assert_eq!(crate::trust::take_notice(), Some(grant.notice()));
    assert_eq!(crate::trust::take_notice(), None);
}

/// How many times `aterm` was asked for its harness policy.
fn policy_reads(aterm: &FakeAterm) -> usize {
    aterm
        .requests()
        .iter()
        .filter(|r| r.contains("harness policy"))
        .count()
}

/// A one-minute hang detector around a judgement, never a latency budget.
async fn bounded<F: std::future::Future>(run: F) -> F::Output {
    tokio::time::timeout(Duration::from_secs(60), run)
        .await
        .expect("the judgement hung")
}

/// A seamless update's commit suspends aterm's host for seconds: the policy is read again
/// until the host resumes, and the launch folder is trusted then, with no screen. NEGATIVE
/// CONTROLS: a person's hold and an older aterm's refusal are read once and show the screen
/// at once.
#[tokio::test]
async fn a_suspended_host_is_read_again_until_it_resumes() {
    let launch = gate("/Users/_owner/Developer/app", "/Users/_owner/Developer/app");
    let (_dir, aterm) = fake(Mode::Current).await;
    aterm.set_policy_key("trust_roots", "~/Developer");
    aterm.set_effective(false, "suspended");
    let (verdict, ()) = bounded(async {
        tokio::join!(
            judge_settled_with(
                &launch,
                Some(aterm.endpoint()),
                Some(Path::new(HOME)),
                UID,
                Duration::from_secs(50),
            ),
            async {
                while policy_reads(&aterm) < 2 {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                aterm.set_effective(true, "enabled");
            },
        )
    })
    .await;
    assert_eq!(trusted(verdict).root.as_deref(), Some("~/Developer"));
    assert!(policy_reads(&aterm) >= 3, "{:?}", aterm.requests());

    aterm.set_effective(false, "hold");
    let before = policy_reads(&aterm);
    let why = shown(
        bounded(judge_settled_with(
            &launch,
            Some(aterm.endpoint()),
            Some(Path::new(HOME)),
            UID,
            Duration::from_secs(50),
        ))
        .await,
    );
    assert!(why.contains("why=hold"), "{why}");
    assert_eq!(
        policy_reads(&aterm),
        before + 1,
        "CONTROL: a hold is read once"
    );

    let (_old_dir, old) = fake(Mode::Old).await;
    let why = shown(
        bounded(judge_settled_with(
            &launch,
            Some(old.endpoint()),
            Some(Path::new(HOME)),
            UID,
            Duration::from_secs(50),
        ))
        .await,
    );
    assert!(why.contains("no harness policy"), "{why}");
    assert_eq!(
        policy_reads(&old),
        1,
        "CONTROL: an older aterm is read once"
    );
}

/// A policy read aterm does not answer is made again, and trusts once aterm answers; one that
/// stays unanswered shows the screen when the settle bound has gone, its reason the stall's.
#[tokio::test]
async fn an_unanswered_read_is_made_again_within_the_bound() {
    let launch = gate("/Users/_owner/Developer/app", "/Users/_owner/Developer/app");
    let (_dir, aterm) = fake(Mode::Current).await;
    aterm.set_policy_key("trust_roots", "~/Developer");
    aterm.stall("harness");
    let (verdict, ()) = bounded(async {
        tokio::join!(
            judge_settled_with(
                &launch,
                Some(aterm.endpoint()),
                Some(Path::new(HOME)),
                UID,
                Duration::from_secs(50),
            ),
            async {
                while policy_reads(&aterm) < 1 {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                aterm.unstall("harness");
            },
        )
    })
    .await;
    assert_eq!(trusted(verdict).root.as_deref(), Some("~/Developer"));

    aterm.stall("harness");
    let why = shown(
        bounded(judge_settled_with(
            &launch,
            Some(aterm.endpoint()),
            Some(Path::new(HOME)),
            UID,
            Duration::from_millis(100),
        ))
        .await,
    );
    assert!(why.contains("did not answer"), "{why}");
}

/// aterm's admission refusal `ERR control server busy; retry` (every request lane taken, a
/// busy machine's many launches) is waited out, its `retry_ms=` the pause, and the launch
/// folder is trusted once aterm admits the read (review of d5e2fc5abf, finding 3). NEGATIVE
/// CONTROL: with no settle bound, the one read's refusal shows the screen, naming it.
#[tokio::test]
async fn a_busy_aterm_is_read_again_until_it_admits_the_read() {
    let launch = gate("/Users/_owner/Developer/app", "/Users/_owner/Developer/app");
    let (_dir, aterm) = fake(Mode::Current).await;
    aterm.set_policy_key("trust_roots", "~/Developer");
    aterm.busy_next(2);
    let grant = trusted(
        bounded(judge_settled_with(
            &launch,
            Some(aterm.endpoint()),
            Some(Path::new(HOME)),
            UID,
            Duration::from_secs(50),
        ))
        .await,
    );
    assert_eq!(grant.root.as_deref(), Some("~/Developer"));
    assert_eq!(
        (aterm.connections(), policy_reads(&aterm)),
        (3, 1),
        "two refused at admission, then one read"
    );

    aterm.busy_next(1);
    let why = shown(
        bounded(judge_settled_with(
            &launch,
            Some(aterm.endpoint()),
            Some(Path::new(HOME)),
            UID,
            Duration::ZERO,
        ))
        .await,
    );
    assert!(why.contains("busy"), "{why}");
}
