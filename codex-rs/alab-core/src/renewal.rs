// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! One-shot, host-owned admission for a same-account renewal continuation.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

type AdmissionFuture = Pin<Box<dyn Future<Output = Result<(), String>> + Send>>;
type Admit = dyn Fn(String) -> AdmissionFuture + Send + Sync;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RequestAuth {
    ChatGpt { account: String, user: String },
    OpenAiApiKey,
}

/// File metadata only. Captured before the request; never contains credentials.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CredentialFileStat {
    pub dev: u64,
    pub ino: u64,
    pub size: u64,
    pub modified: Option<std::time::SystemTime>,
}

pub fn credential_file_stat(file: &std::path::Path) -> Option<CredentialFileStat> {
    let meta = std::fs::symlink_metadata(file).ok()?;
    if !meta.is_file() {
        return None;
    }
    #[cfg(unix)]
    let (dev, ino) = {
        use std::os::unix::fs::MetadataExt as _;
        (meta.dev(), meta.ino())
    };
    #[cfg(not(unix))]
    let (dev, ino) = (0, 0);
    Some(CredentialFileStat {
        dev,
        ino,
        size: meta.len(),
        modified: meta.modified().ok(),
    })
}

#[derive(Clone)]
pub struct AuthAttempt {
    pub auth: Option<RequestAuth>,
    pub owner_generation: u64,
    pub generation: u64,
    current: Arc<dyn Fn() -> bool + Send + Sync>,
    credentials_changed: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
}

impl AuthAttempt {
    /// Opaque in-memory comparison with the actual attempted credential.
    /// No credential value or fingerprint leaves the request setup closure.
    pub fn credentials_changed(&self) -> Option<bool> {
        self.credentials_changed.as_ref().map(|changed| changed())
    }
}

/// Volatile proof from the actual foreground request. A setup retry replaces
/// the attempted credential revision, while the first file observation remains
/// fixed. No credential or credential fingerprint is persisted.
pub struct AuthRequestScope {
    pub turn: String,
    attempt: std::sync::Mutex<AuthAttempt>,
    first_file: std::sync::OnceLock<Option<CredentialFileStat>>,
    owner: std::sync::OnceLock<Arc<dyn Send + Sync>>,
}

impl AuthRequestScope {
    pub fn new(
        turn: String,
        auth: Option<RequestAuth>,
        owner_generation: u64,
        generation: u64,
        current: impl Fn() -> bool + Send + Sync + 'static,
    ) -> Self {
        Self {
            turn,
            attempt: std::sync::Mutex::new(AuthAttempt {
                auth,
                owner_generation,
                generation,
                current: Arc::new(current),
                credentials_changed: None,
            }),
            first_file: std::sync::OnceLock::new(),
            owner: std::sync::OnceLock::new(),
        }
    }

    pub fn snapshot(&self) -> AuthAttempt {
        self.attempt
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub fn eligible(&self, turn: &str) -> bool {
        let attempt = self.snapshot();
        self.turn == turn && attempt.auth.is_some() && (attempt.current)()
    }

    pub fn first_file_stat(&self) -> Option<CredentialFileStat> {
        self.first_file.get().copied().flatten()
    }

    pub fn observe_file(&self, file: &std::path::Path) {
        let _ = self.first_file.set(credential_file_stat(file));
    }

    pub fn registered_to<T: Send + Sync + 'static>(&self, owner: &Arc<T>) -> bool {
        self.owner.get().is_some_and(|registered| {
            std::ptr::addr_eq(Arc::as_ptr(registered), Arc::as_ptr(owner))
        })
    }

    pub fn register<T: Send + Sync + 'static>(self: &Arc<Self>, owner: Arc<T>) {
        let key = Arc::as_ptr(&owner) as usize;
        // Keep even an ineligible manager alive, so its address cannot be
        // recycled while this turn's registry entry still exists.
        if self.owner.set(owner).is_err() {
            return;
        }
        let mut entries = request_scopes()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        entries.retain(|_, scope| scope.strong_count() != 0);
        entries.insert((key, self.turn.clone()), Arc::downgrade(self));
    }
}

type RequestScopes = std::collections::HashMap<(usize, String), std::sync::Weak<AuthRequestScope>>;
fn request_scopes() -> &'static std::sync::Mutex<RequestScopes> {
    static SCOPES: std::sync::OnceLock<std::sync::Mutex<RequestScopes>> =
        std::sync::OnceLock::new();
    SCOPES.get_or_init(Default::default)
}

/// Called only for a foreground attempt, after resolving its actual auth.
/// `owner` is an in-process manager address, never serialized or logged.
pub fn record_auth_attempt(
    owner: usize,
    turn: &str,
    auth: Option<RequestAuth>,
    owner_generation: u64,
    generation: u64,
    current: impl Fn() -> bool + Send + Sync + 'static,
    credentials_changed: impl Fn() -> bool + Send + Sync + 'static,
) {
    let scope = request_scopes()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&(owner, turn.to_owned()))
        .and_then(std::sync::Weak::upgrade);
    if let Some(scope) = scope {
        *scope
            .attempt
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = AuthAttempt {
            auth,
            owner_generation,
            generation,
            current: Arc::new(current),
            credentials_changed: Some(Arc::new(credentials_changed)),
        };
    }
}

#[cfg(test)]
mod scope_tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;

    #[test]
    fn attempt_updates_are_bound_to_the_registered_manager_and_native_turn() {
        let manager = Arc::new(());
        let other = Arc::new(());
        let key = Arc::as_ptr(&manager) as usize;
        let scope = Arc::new(AuthRequestScope::new(
            "attempt-origin".into(),
            Some(RequestAuth::OpenAiApiKey),
            1,
            4,
            || true,
        ));
        scope.register(Arc::clone(&manager));
        assert!(scope.registered_to(&manager));
        assert!(!scope.registered_to(&other));
        let observed = AuthObservation::new(&scope, &scope.snapshot(), || true);
        assert!(observed.proves_cause(&scope, "attempt-origin"));
        assert_eq!(scope.snapshot().credentials_changed(), None);
        assert!(!observed.proves_cause(&scope, "other-origin"));
        let replacement = Arc::new(AuthRequestScope::new(
            "attempt-origin".into(),
            None,
            0,
            0,
            || false,
        ));
        assert!(
            !observed.proves_cause(&replacement, "attempt-origin"),
            "proof cannot move to a replacement request scope"
        );

        record_auth_attempt(
            Arc::as_ptr(&other) as usize,
            "attempt-origin",
            None,
            9,
            9,
            || false,
            || false,
        );
        record_auth_attempt(key, "other-origin", None, 9, 9, || false, || false);
        assert_eq!(scope.snapshot().generation, 4);
        record_auth_attempt(
            key,
            "attempt-origin",
            Some(RequestAuth::OpenAiApiKey),
            2,
            8,
            || true,
            || true,
        );
        assert_eq!(scope.snapshot().generation, 8);
        assert_eq!(scope.snapshot().credentials_changed(), Some(true));
        assert!(
            !observed.proves_cause(&scope, "attempt-origin"),
            "passive cause cannot cross an actual attempt"
        );
        assert_eq!(scope.snapshot().owner_generation, 2);
        assert!(scope.eligible("attempt-origin"));
        let weak = Arc::downgrade(&manager);
        drop(manager);
        assert!(
            weak.upgrade().is_some(),
            "registered manager cannot have its address recycled"
        );
        drop(scope);
        assert!(
            weak.upgrade().is_none(),
            "registry weak entry does not retain a completed scope"
        );
    }

    #[test]
    fn request_scope_cannot_cross_turns_or_a_revoked_owner() {
        let owner = Arc::new(AtomicBool::new(true));
        let live = Arc::clone(&owner);
        let scope = AuthRequestScope::new(
            "original".into(),
            Some(RequestAuth::OpenAiApiKey),
            7,
            11,
            move || live.load(Ordering::SeqCst),
        );
        assert!(scope.eligible("original"));
        assert!(!scope.eligible("later"));
        owner.store(false, Ordering::SeqCst);
        assert!(!scope.eligible("original"));
        let unsupported = AuthRequestScope::new("original".into(), None, 0, 0, || true);
        assert!(!unsupported.eligible("original"));
    }
}

/// Proven cause of an actual typed login refusal. The original profile can
/// remain known or temporarily unknown. This retains only original wall/goal
/// custody; it grants no login, refresh, reset, or model admission authority.
pub struct AuthObservation {
    turn: String,
    scope: std::sync::Weak<AuthRequestScope>,
    auth: Option<RequestAuth>,
    owner_generation: u64,
    generation: u64,
    current: Arc<dyn Fn() -> bool + Send + Sync>,
}

impl AuthObservation {
    pub fn new(
        scope: &Arc<AuthRequestScope>,
        attempt: &AuthAttempt,
        current: impl Fn() -> bool + Send + Sync + 'static,
    ) -> Self {
        Self {
            turn: scope.turn.clone(),
            scope: Arc::downgrade(scope),
            auth: attempt.auth.clone(),
            owner_generation: attempt.owner_generation,
            generation: attempt.generation,
            current: Arc::new(current),
        }
    }

    pub fn proves_cause(&self, scope: &Arc<AuthRequestScope>, turn: &str) -> bool {
        let attempt = scope.snapshot();
        self.auth.is_some()
            && self.turn == turn
            && self
                .scope
                .upgrade()
                .is_some_and(|bound| Arc::ptr_eq(&bound, scope))
            && attempt.auth == self.auth
            && attempt.owner_generation == self.owner_generation
            && attempt.generation == self.generation
            && (self.current)()
    }
}

/// Captured before a turn starts; later goal completion cannot reclassify it.
pub struct GoalAtTurnStart {
    pub goal_id: Option<String>,
}

pub struct TurnAdmission {
    admit: Arc<Admit>,
    admitted: tokio::sync::OnceCell<String>,
}

impl TurnAdmission {
    pub fn new(admit: impl Fn(String) -> AdmissionFuture + Send + Sync + 'static) -> Self {
        Self {
            admit: Arc::new(admit),
            admitted: tokio::sync::OnceCell::new(),
        }
    }

    /// Failed persistence grants nothing. A successful admission is consumed
    /// once for the same stable ID. Callers remove the startup attachment before
    /// future ordinary turns inherit it.
    pub async fn admit(&self, turn: String) -> Result<(), String> {
        let expected = turn.clone();
        let admitted = self
            .admitted
            .get_or_try_init(|| async {
                (self.admit)(expected.clone()).await?;
                Ok::<String, String>(expected)
            })
            .await?;
        if admitted == &turn {
            Ok(())
        } else {
            Err("renewal admission belongs to another turn".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    #[test]
    fn first_request_file_observation_rejects_non_regular_paths_and_stays_first()
    -> std::io::Result<()> {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(std::io::Error::other)?
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("alab-request-file-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&root)?;
        let scope = || {
            AuthRequestScope::new(
                "actual-request".into(),
                Some(RequestAuth::OpenAiApiKey),
                0,
                0,
                || true,
            )
        };
        let path = root.join("auth.json");
        std::fs::create_dir(&path)?;
        let first = scope();
        first.observe_file(&path);
        assert_eq!(
            first.first_file_stat(),
            None,
            "a directory is not a credential-file witness"
        );
        std::fs::remove_dir(&path)?;
        std::fs::write(&path, b"{}\n")?;
        first.observe_file(&path);
        assert_eq!(
            first.first_file_stat(),
            None,
            "later arrival cannot replace the first request observation"
        );
        let fresh = scope();
        fresh.observe_file(&path);
        assert!(
            fresh.first_file_stat().is_some(),
            "a new request observes the real regular file"
        );
        #[cfg(unix)]
        {
            let link = root.join("linked-auth.json");
            std::os::unix::fs::symlink(&path, &link)?;
            let linked = scope();
            linked.observe_file(&link);
            assert_eq!(
                linked.first_file_stat(),
                None,
                "request ownership never follows a credential symlink"
            );
        }
        std::fs::remove_dir_all(root)
    }

    #[tokio::test]
    async fn failed_persistence_can_retry_but_success_is_bound_to_one_id() {
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        let admission = TurnAdmission::new(move |_| {
            let failed = seen.fetch_add(1, Ordering::SeqCst) == 0;
            Box::pin(async move {
                if failed {
                    Err("disk unavailable".into())
                } else {
                    Ok(())
                }
            })
        });
        assert!(admission.admit("same".into()).await.is_err());
        assert!(admission.admit("same".into()).await.is_ok());
        assert!(admission.admit("same".into()).await.is_ok());
        assert!(admission.admit("different".into()).await.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn concurrent_attempts_initialize_only_one_native_id() {
        let calls = Arc::new(AtomicUsize::new(0));
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let admission = Arc::new(TurnAdmission::new({
            let calls = Arc::clone(&calls);
            let entered = Arc::clone(&entered);
            let release = Arc::clone(&release);
            move |_| {
                calls.fetch_add(1, Ordering::SeqCst);
                let entered = Arc::clone(&entered);
                let release = Arc::clone(&release);
                Box::pin(async move {
                    entered.notify_one();
                    release.notified().await;
                    Ok(())
                })
            }
        }));
        let first = tokio::spawn({
            let admission = Arc::clone(&admission);
            async move { admission.admit("first".into()).await }
        });
        entered.notified().await;
        let mut other = Box::pin(admission.admit("other".into()));
        std::future::poll_fn(|cx| {
            assert!(other.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        release.notify_one();
        assert!(first.await.expect("first task").is_ok());
        assert!(other.await.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn cancelled_initializer_grants_nothing_and_same_id_can_retry() {
        let calls = Arc::new(AtomicUsize::new(0));
        let entered = Arc::new(tokio::sync::Notify::new());
        let blocked = Arc::new(tokio::sync::Notify::new());
        let admission = Arc::new(TurnAdmission::new({
            let calls = Arc::clone(&calls);
            let entered = Arc::clone(&entered);
            move |_| {
                let first = calls.fetch_add(1, Ordering::SeqCst) == 0;
                let entered = Arc::clone(&entered);
                let blocked = Arc::clone(&blocked);
                Box::pin(async move {
                    if first {
                        entered.notify_one();
                        blocked.notified().await;
                    }
                    Ok(())
                })
            }
        }));
        let cancelled = tokio::spawn({
            let admission = Arc::clone(&admission);
            async move { admission.admit("original".into()).await }
        });
        entered.notified().await;
        cancelled.abort();
        assert!(
            cancelled
                .await
                .expect_err("initializer was cancelled")
                .is_cancelled()
        );
        assert!(admission.admit("original".into()).await.is_ok());
        assert!(admission.admit("original".into()).await.is_ok());
        assert!(admission.admit("different".into()).await.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
}
