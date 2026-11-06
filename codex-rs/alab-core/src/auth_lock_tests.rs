// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! [`super::admit`] against a scratch login and an injected refresher: a counter stands in for
//! the token endpoint, and a second open of the lock file (its own open file description, as
//! another process's would be) stands in for aterm holding the lock. Time is tokio's paused
//! clock, so the waits below are the lock's own and take no wall time.

use super::Admission;
use super::AuthLock;
use super::WAIT;
use super::admit;
use super::lock_path;
use pretty_assertions::assert_eq;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

/// A hang detector for what must happen: one minute.
const HANG: Duration = Duration::from_secs(60);

/// A scratch directory holding one login's `auth.json`, removed on drop.
struct Scratch {
    dir: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir =
            std::env::temp_dir().join(format!("alab-auth-lock-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap_or_else(|err| panic!("{err}"));
        let scratch = Scratch { dir };
        scratch.write_refresh_token("refresh-1");
        scratch
    }

    fn auth_file(&self) -> PathBuf {
        self.dir.join("auth.json")
    }

    fn write_refresh_token(&self, token: &str) {
        std::fs::write(
            self.auth_file(),
            format!("{{\"tokens\":{{\"refresh_token\":\"{token}\"}}}}"),
        )
        .unwrap_or_else(|err| panic!("{err}"));
    }

    fn refresh_token(&self) -> String {
        let text = std::fs::read_to_string(self.auth_file()).unwrap_or_else(|err| panic!("{err}"));
        text.split("\"refresh_token\":\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .unwrap_or_else(|| panic!("no refresh token in {text}"))
            .to_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The lock beside `auth_file` through an open of its own, as another process would hold it.
fn hold_elsewhere(auth_file: &Path) -> AuthLock {
    AuthLock::try_take(auth_file)
        .unwrap_or_else(|err| panic!("{err}"))
        .unwrap_or_else(|| panic!("the lock was free"))
}

/// One refresh as codex makes it: admitted with a read of the file under the lock that says
/// whether its refresh token is still `sending`, then a request to the stand-in endpoint
/// (`sends`) and a write of the rotated token while the hold is kept.
async fn refresh(auth_file: PathBuf, sending: String, sends: Arc<AtomicUsize>) -> bool {
    let reread = auth_file.clone();
    let admission = admit(Some(&auth_file), WAIT, || async move {
        let text = std::fs::read_to_string(&reread).unwrap_or_default();
        !text.contains(&format!("\"refresh_token\":\"{sending}\""))
    })
    .await;
    match admission {
        Admission::Unavailable(err) => panic!("unexpected refresh admission failure: {err}"),
        Admission::Reused => false,
        Admission::Send(hold) => {
            sends.fetch_add(1, Ordering::SeqCst);
            std::fs::write(
                &auth_file,
                "{\"tokens\":{\"refresh_token\":\"refresh-sent\"}}",
            )
            .unwrap_or_else(|err| panic!("{err}"));
            hold.is_locked()
        }
    }
}

#[tokio::test(start_paused = true)]
async fn a_busy_account_does_not_delay_other_parallel_logins() {
    let scratch = Scratch::new("parallel-logins");
    let busy = scratch.auth_file();
    let holder = hold_elsewhere(&busy);
    let started = tokio::time::Instant::now();
    let mut holds = Vec::new();
    // Keep every admission alive: releasing each before the next would miss a
    // process-global lock accidentally replacing the per-login lock.
    for index in 0..32 {
        let auth = scratch.dir.join(format!("account-{index}.json"));
        let admission = admit(Some(&auth), WAIT, || async { false }).await;
        let Admission::Send(hold) = admission else {
            panic!("an independent login was treated as already refreshed");
        };
        assert!(
            hold.is_locked(),
            "account {index} lost refresh serialization"
        );
        assert!(
            AuthLock::try_take(&auth)
                .unwrap_or_else(|err| panic!("{err}"))
                .is_none()
        );
        holds.push((auth, hold));
    }
    assert_eq!(tokio::time::Instant::now(), started);
    assert!(
        AuthLock::try_take(&busy)
            .unwrap_or_else(|err| panic!("{err}"))
            .is_none()
    );
    assert_eq!(scratch.refresh_token(), "refresh-1");
    for (auth, hold) in holds {
        drop(hold);
        assert!(
            AuthLock::try_take(&auth)
                .unwrap_or_else(|err| panic!("{err}"))
                .is_some()
        );
    }
    drop(holder);
}

#[tokio::test(start_paused = true)]
async fn a_held_lock_defers_the_refresh_until_it_is_released() {
    let scratch = Scratch::new("held");
    let holder = hold_elsewhere(&scratch.auth_file());
    let sends = Arc::new(AtomicUsize::new(0));
    let task = tokio::spawn(refresh(
        scratch.auth_file(),
        "refresh-1".to_owned(),
        Arc::clone(&sends),
    ));

    // A third of the bound: hundreds of looks at the lock, every one of them refused.
    tokio::time::sleep(WAIT / 3).await;
    assert_eq!(
        sends.load(Ordering::SeqCst),
        0,
        "sent while the lock was held"
    );
    assert!(!task.is_finished());

    drop(holder);
    let locked = tokio::time::timeout(HANG, task)
        .await
        .unwrap_or_else(|_| panic!("the refresh never ran"))
        .unwrap_or_else(|err| panic!("{err}"));
    assert!(locked, "the refresh sent without the lock");
    assert_eq!(sends.load(Ordering::SeqCst), 1);
    assert_eq!(scratch.refresh_token(), "refresh-sent");
}

#[tokio::test(start_paused = true)]
async fn a_rotation_made_under_the_lock_is_reused_and_nothing_is_sent() {
    let scratch = Scratch::new("rotated");
    let holder = hold_elsewhere(&scratch.auth_file());
    let sends = Arc::new(AtomicUsize::new(0));
    // The refresh read `refresh-1` before it asked for the lock.
    let task = tokio::spawn(refresh(
        scratch.auth_file(),
        "refresh-1".to_owned(),
        Arc::clone(&sends),
    ));
    tokio::time::sleep(Duration::from_secs(1)).await;

    // The holder (aterm) spends `refresh-1` and writes its answer before letting go.
    scratch.write_refresh_token("refresh-2-from-aterm");
    drop(holder);
    let sent = tokio::time::timeout(HANG, task)
        .await
        .unwrap_or_else(|_| panic!("the refresh never ran"))
        .unwrap_or_else(|err| panic!("{err}"));
    assert!(!sent);
    assert_eq!(
        sends.load(Ordering::SeqCst),
        0,
        "sent a spent refresh token"
    );
    assert_eq!(scratch.refresh_token(), "refresh-2-from-aterm");
}

#[tokio::test(start_paused = true)]
async fn an_unchanged_login_is_sent_under_the_lock() {
    let scratch = Scratch::new("unchanged");
    let sends = Arc::new(AtomicUsize::new(0));
    let locked = refresh(
        scratch.auth_file(),
        "refresh-1".to_owned(),
        Arc::clone(&sends),
    )
    .await;
    assert!(locked);
    assert_eq!(sends.load(Ordering::SeqCst), 1);
    // The hold went with the refresh: the lock is free again.
    assert!(
        AuthLock::try_take(&scratch.auth_file())
            .unwrap_or_else(|err| panic!("{err}"))
            .is_some()
    );
}

#[tokio::test(start_paused = true)]
async fn a_lock_held_past_the_bound_defers_refresh_without_spending_the_token() {
    let scratch = Scratch::new("wedged");
    let wedged = hold_elsewhere(&scratch.auth_file());
    let started = tokio::time::Instant::now();
    let reread = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&reread);
    let admission = admit(Some(&scratch.auth_file()), WAIT, || async move {
        seen.fetch_add(1, Ordering::SeqCst);
        true
    })
    .await;
    assert!(started.elapsed() >= WAIT);
    assert!(matches!(admission, Admission::Unavailable(ref error)
        if error.kind() == std::io::ErrorKind::TimedOut));
    assert_eq!(
        reread.load(Ordering::SeqCst),
        0,
        "a busy login must not read outside its lock"
    );
    assert_eq!(scratch.refresh_token(), "refresh-1");
    drop(wedged);
    let sends = Arc::new(AtomicUsize::new(0));
    assert!(refresh(scratch.auth_file(), "refresh-1".into(), Arc::clone(&sends)).await);
    assert_eq!(sends.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn a_symlinked_lock_path_is_refused() {
    let scratch = Scratch::new("symlink");
    let elsewhere = scratch.dir.join("elsewhere");
    std::os::unix::fs::symlink(&elsewhere, lock_path(&scratch.auth_file()))
        .unwrap_or_else(|err| panic!("{err}"));

    let refused = AuthLock::try_take(&scratch.auth_file());
    assert!(refused.is_err(), "took a lock through a link: {refused:?}");
    assert!(!elsewhere.exists(), "the link's target was made");

    // A refused lock never admits an unlocked refresh.
    let admission = admit(Some(&scratch.auth_file()), WAIT, || async { true }).await;
    assert!(matches!(admission, Admission::Unavailable(_)));
    assert!(!elsewhere.exists());

    // A link to an existing file is refused too, and that file is left unlocked.
    std::fs::write(&elsewhere, "").unwrap_or_else(|err| panic!("{err}"));
    assert!(AuthLock::try_take(&scratch.auth_file()).is_err());
    let target = std::fs::File::open(&elsewhere).unwrap_or_else(|err| panic!("{err}"));
    assert!(target.try_lock().is_ok());
}

#[tokio::test(start_paused = true)]
async fn no_login_file_takes_no_lock() {
    let scratch = Scratch::new("none");
    let admission = admit(None, WAIT, || async { true }).await;
    assert!(matches!(admission, Admission::Send(ref hold) if !hold.is_locked()));
    assert!(!lock_path(&scratch.auth_file()).exists());
}

#[test]
fn the_lock_file_is_the_one_aterm_takes() {
    assert_eq!(
        lock_path(Path::new("/home/a/.codex/auth.json")),
        PathBuf::from("/home/a/.codex/auth.json.lock")
    );
    assert_eq!(
        lock_path(Path::new("/pool/acct-b.json")),
        PathBuf::from("/pool/acct-b.json.lock")
    );

    use std::os::unix::fs::PermissionsExt as _;
    let scratch = Scratch::new("mode");
    drop(hold_elsewhere(&scratch.auth_file()));
    let mode = std::fs::metadata(lock_path(&scratch.auth_file()))
        .unwrap_or_else(|err| panic!("{err}"))
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}
