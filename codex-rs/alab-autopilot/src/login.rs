// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! THE LOST LOGIN COMES BACK (CONTRACT §5, the login wall row; amended 2026-10-28 after the
//! parity sweep, gap 13 stage 1: a codex-alab tab stayed idle after its login was renewed
//! elsewhere): what codex-alab may say of a login wall's sign-in after the wall, read through
//! codex's own `AuthManager` only, never by parsing the login's file. aterm's counterpart is its
//! cross-tab login latch (`LoginShared::Back`), which keys Claude Code's logins only; codex-alab
//! reads its own process's.
//!
//! * [`Baseline`]: a live `Wall::Login` uses the final actual request attempt's credential
//!   generation and identity, and the first file [`Stat`] from before the request. An already
//!   tried internal reload cannot masquerade as recovery; an intervening file save is seen.
//!   A wall a relaunch
//!   restored takes the login as it stands at the wall's first decision as its baseline: its
//!   process loaded the login fresh, so only a later change says anything.
//! * [`Baseline::read`]: the login is [`Read::Back`] when the generation is past the baseline's,
//!   the manager caches a credential with no refresh refused for it, and its identity is the
//!   walled one. Another identity's login is [`Read::Other`], which types nothing. No token and
//!   no identity value is kept outside the baseline or said anywhere: a read says `same` or
//!   `other`, nothing more.
//! * [`Baseline::reload`]: codex reconciles an existing selected file store once when its watch starts,
//!   then reloads only where a look's stat moved or a watch event named the file
//!   ([`FolderWatch`]), never on a bare timer: a reload in keyring or auto
//!   mode reads the macOS keychain, which can put up a dialog. It takes only the walled
//!   identity's login (`AuthManager::alab_reload_if`), as codex's own 401 recovery reloads only
//!   the expected account's: another identity's login, or none, stays in the store, and this
//!   process keeps the walled one. So [`Read::Other`] comes only from a change codex made to
//!   the login in this process (its own sign-in).
//! * [`Stat`]: a look stats the file (device, inode, size, modification time) and never opens
//!   it.
//!
//! AN API KEY NAMES NO ONE: an API key's identity (a Bedrock key's too) is its auth mode alone,
//! for it names no user and no account. So any key written to the login's file is the walled
//! identity's, a key of another organization or project included: it reads back, the line is
//! typed, and the session goes on on that key. Nothing in a key says whose it is.
//!
//! WHAT CANNOT BE SEEN: a renewal elsewhere of a login in ephemeral storage (in memory, which
//! no other process writes), and of a keyring or auto-mode login with no file (nothing moves
//! where a look reads). There the ask stands, as it did, unless codex changes the login in
//! this process, which the watch hears. aterm's renewal of a roster login into a new folder is
//! stage 2: this process's `CODEX_AUTH_PATH` still names the old file, which nothing renews.

use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use codex_file_watcher::FileWatcher;
use codex_file_watcher::FileWatcherSubscriber;
use codex_file_watcher::Receiver;
use codex_file_watcher::WatchPath;
use codex_file_watcher::WatchRegistration;
use codex_login::AuthManager;
use codex_login::CodexAuth;
use codex_protocol::auth::AuthMode;

/// WHO A LOGIN SIGNS IN: its auth mode, and the user and account it names (ChatGPT's; `None`
/// for an API key). Never printed: it has no `Debug`, and nothing says it but [`Read`].
#[derive(Clone, PartialEq, Eq)]
struct Identity {
    mode: AuthMode,
    user: Option<String>,
    account: Option<String>,
}

impl Identity {
    fn of(auth: &CodexAuth) -> Identity {
        Identity {
            mode: auth.auth_mode(),
            user: auth.get_chatgpt_user_id(),
            account: auth.get_account_id(),
        }
    }
}

/// THE LOGIN FILE AS A LOOK SEES IT, from `stat(2)` alone (the file is never opened): a save
/// renames a new file over it (a new inode), and any other write moves its size or its time.
pub use codex_alab_core::renewal::CredentialFileStat as Stat;
pub use codex_alab_core::renewal::credential_file_stat as stat;

/// What a read of the login says against its wall's [`Baseline`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Read {
    /// The walled identity's login, renewed since the wall and usable: its turn is delivered
    /// again once ([`crate::words::RULE_LOGIN_BACK`]).
    Back,
    /// A login renewed since the wall that signs in another identity: nothing is typed.
    Other,
    /// Nothing renewed since the wall, or what was is refused too: the ask stands.
    Stands,
}

impl Read {
    /// The read in words a journal may hold (`same`/`other`, never a value).
    pub fn words(self) -> &'static str {
        match self {
            Read::Back => "the same identity's login is back",
            Read::Other => "another identity's login: nothing is typed",
            Read::Stands => "no new login",
        }
    }
}

/// A LOGIN WALL'S BASELINE: the manager's change generation, the walled identity and the login
/// file's stat, read together ([`Baseline::of`]).
#[derive(Clone)]
pub struct Baseline {
    generation: u64,
    identity: Option<Identity>,
    file: PathBuf,
    stat: Option<Stat>,
    attempt: Option<codex_alab_core::renewal::AuthAttempt>,
}

/// Unknown identity is observation only; only a positive mismatch revokes it.
pub(crate) fn request_profile(
    manager: &AuthManager,
    expected: &codex_alab_core::renewal::RequestAuth,
) -> Option<bool> {
    use codex_alab_core::renewal::RequestAuth;
    let auth = manager.auth_cached()?;
    match expected {
        RequestAuth::ChatGpt { account, user } => {
            if auth.auth_mode() != AuthMode::Chatgpt {
                return Some(false);
            }
            let current_account = auth
                .get_account_id()
                .filter(|value| !value.trim().is_empty())?;
            let current_user = auth
                .get_chatgpt_user_id()
                .filter(|value| !value.trim().is_empty())?;
            Some(current_account == *account && current_user == *user)
        }
        RequestAuth::OpenAiApiKey => Some(auth.is_api_key_auth()),
    }
}

impl Baseline {
    /// The login as `manager` holds it now, with its file (`codex_alab_core::home::auth_file`
    /// of the manager's home: the admitted `CODEX_AUTH_PATH`, else `auth.json` there) as a look
    /// sees it.
    pub fn of(manager: &AuthManager) -> Baseline {
        let generation = manager.auth_change_state_receiver().borrow().generation;
        let identity = manager.auth_cached().as_ref().map(Identity::of);
        let file = codex_alab_core::home::auth_file(&manager.runtime_config().codex_home);
        let stat = stat(&file);
        Baseline {
            generation,
            identity,
            file,
            stat,
            attempt: None,
        }
    }

    /// The final attempted credential revision, rather than an observation
    /// after the request failed. An intervening file save remains observable.
    pub fn from_request(
        manager: &AuthManager,
        scope: &codex_alab_core::renewal::AuthRequestScope,
    ) -> Baseline {
        Self::from_attempt(manager, scope, &scope.snapshot())
    }

    pub(crate) fn from_attempt(
        manager: &AuthManager,
        scope: &codex_alab_core::renewal::AuthRequestScope,
        attempt: &codex_alab_core::renewal::AuthAttempt,
    ) -> Baseline {
        use codex_alab_core::renewal::RequestAuth;
        let identity = attempt.auth.clone().map(|auth| match auth {
            RequestAuth::ChatGpt { account, user } => Identity {
                mode: AuthMode::Chatgpt,
                user: Some(user),
                account: Some(account),
            },
            RequestAuth::OpenAiApiKey => Identity {
                mode: AuthMode::ApiKey,
                user: None,
                account: None,
            },
        });
        Baseline {
            generation: attempt.generation,
            identity,
            file: codex_alab_core::home::auth_file(&manager.runtime_config().codex_home),
            stat: scope.first_file_stat(),
            attempt: Some(attempt.clone()),
        }
    }

    /// The login's file, which the watch and the looks read.
    pub fn file(&self) -> &Path {
        &self.file
    }

    /// The file's stat at the wall: what the first look compares with.
    pub fn stat(&self) -> Option<Stat> {
        self.stat
    }

    /// WHETHER THE LOGIN IS BACK, read from `manager` as it holds the login now (its cache: this
    /// reads no store). A wall whose identity was unknown (no credential cached at it) has no
    /// login of its own to come back.
    pub fn read(&self, manager: &AuthManager) -> Read {
        let before = *manager.auth_change_state_receiver().borrow();
        let read = (|| {
            if before.generation <= self.generation {
                return Read::Stands;
            }
            let Some(auth) = manager.auth_cached() else {
                return Read::Stands;
            };
            if self.identity.as_ref() != Some(&Identity::of(&auth)) {
                return Read::Other;
            }
            if self
                .attempt
                .as_ref()
                .and_then(codex_alab_core::renewal::AuthAttempt::credentials_changed)
                == Some(false)
                || manager.refresh_failure_for_auth(&auth).is_some()
            {
                return Read::Stands;
            }
            Read::Back
        })();
        // The opaque credential comparison also reads the cache. Never combine
        // one identity snapshot with another publication's changed bearer.
        if before != *manager.auth_change_state_receiver().borrow() {
            Read::Stands
        } else {
            read
        }
    }

    /// CODEX RELOADS THE LOGIN, TAKING ONLY THE WALLED IDENTITY'S
    /// (`AuthManager::alab_reload_if`): whether it took the login the store holds. Another
    /// identity's login, or none, is left in the store, and `manager` keeps the login it holds,
    /// as codex's own 401 recovery keeps it (`reload_if_account_id_matches`). A wall whose
    /// identity was unknown takes none.
    pub async fn reload(&self, manager: &AuthManager) -> bool {
        let Some(walled) = &self.identity else {
            return false;
        };
        manager
            .alab_reload_if(|auth| Identity::of(auth) == *walled)
            .await
            .is_some()
    }
}

/// A NON-RECURSIVE WATCH OF THE LOGIN FILE'S FOLDER, built as the launch path's watch is
/// (`Autopilot::watch_launch`): a save is a temporary file renamed over the login's
/// (`codex_alab_core::home::save_auth_bytes`), so the folder is watched, not the file. Its
/// events are hints: the looks go on without them.
pub struct FolderWatch {
    events: Receiver,
    name: OsString,
    _watch: (Arc<FileWatcher>, FileWatcherSubscriber, WatchRegistration),
}

impl FolderWatch {
    /// A watch of `file`'s folder; `None` where it cannot be watched. Built on the current
    /// tokio runtime, whose task the watcher's event loop runs on.
    pub fn of(file: &Path) -> Option<FolderWatch> {
        let dir = file.parent()?.to_path_buf();
        let name = file.file_name()?.to_os_string();
        let watcher = Arc::new(FileWatcher::new().ok()?);
        let (subscriber, events) = watcher.add_subscriber();
        let registration = subscriber.register_paths(vec![WatchPath {
            path: dir,
            recursive: false,
        }]);
        if !registration.registration_succeeded() {
            return None;
        }
        Some(FolderWatch {
            events,
            name,
            _watch: (watcher, subscriber, registration),
        })
    }

    /// The folder's next change: whether it named the login's file; `None` once the watch
    /// ended.
    pub async fn next(&mut self) -> Option<bool> {
        let event = self.events.recv().await?;
        Some(
            event
                .paths
                .iter()
                .any(|path| path.file_name() == Some(self.name.as_os_str())),
        )
    }
}

#[cfg(test)]
#[path = "login_fixtures.rs"]
pub(crate) mod fixtures;

#[cfg(test)]
#[path = "login_tests.rs"]
mod tests;
