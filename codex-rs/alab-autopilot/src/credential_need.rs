// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A CREDENTIAL IS NEVER A PERSON'S NEEDS YOU (owner, 2026-11-12: "credentials must never reach
//! the person as a NEEDS YOU at all"). A `NEEDS YOU:` line that asks for a credential
//! ([`wants_of`]) is taken off the person's list where every reader parses needs
//! ([`crate::needs::persons_needs`]): it is no need of the `harness_needs` set, holds no landing
//! and is no needs leg. codex-alab resolves it on the one path instead ([`resolve`]): each want is
//! looked up in this Mac's secrets registry (`aterm secrets ls --json`: names, pointers and
//! purposes, never a value), and one the registry holds no entry for is filed with `aterm secrets
//! need <want>`. The worker is told what was found or filed in one short line
//! ([`RULE_CREDENTIAL`], [`told_line`]), each fact once a session, and records the blocked step
//! and continues. A release is cut on the release machine, whose ring has no want to file.
//!
//! PROVENANCE: aterm main's `supervise/policy/credential_need.rs` (aterm `a43337d4056`,
//! `87984a01a2a`, `ccd161bbfc6`, `e60036fbe05`), its word rules and their test table verbatim;
//! [`entries_of`] mirrors the rows of aterm's need catalog (`aterm_secrets::need::catalog`,
//! `Want::entries`) for the wants [`wants_of`] names, as aterm's host resolution reads them
//! (`harness/credential_read.rs`).

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::Path;

pub use codex_alab_aterm::secrets::Entry as VaultEntry;

use crate::words::harness_mark;

/// The act that tells the worker how codex-alab resolved its credential line.
pub const RULE_CREDENTIAL: &str = "credential@v1";

/// The want a release, signing or notarized-build need names: the release machine's, never filed.
pub const RELEASE_WANT: &str = "release";

/// How one want resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// The registry holds these entries for it.
    Held(Vec<VaultEntry>),
    /// The registry holds none: codex-alab filed `aterm secrets need <want>`.
    Filed,
    /// Neither was possible, and why.
    Failed(String),
}

impl Resolution {
    /// Whether it stands for the session: a want found or filed is resolved once; a failure is
    /// tried again at the next point that names it.
    #[must_use]
    pub fn settled(&self) -> bool {
        !matches!(self, Self::Failed(_))
    }
}

/// WHAT CODEX-ALAB RESOLVED of the credential wants a point named (`ThreadState::credentials`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum CredentialRead {
    /// Nothing was asked: no credential line, or no want to resolve.
    #[default]
    NotAsked,
    /// Each want asked, with how it resolved.
    Resolved(Vec<(String, Resolution)>),
}

impl CredentialRead {
    fn of(&self, want: &str) -> Option<&Resolution> {
        match self {
            Self::NotAsked => None,
            Self::Resolved(all) => all.iter().find(|(w, _)| w == want).map(|(_, r)| r),
        }
    }
}

/// THE REGISTRY ENTRIES `want` ARRIVES AS (aterm's `Want::entries`), for each want [`wants_of`]
/// names but the release machine's; none for any other.
#[must_use]
pub fn entries_of(want: &str) -> &'static [&'static str] {
    match want {
        "notary" => &["release-notary"],
        "gh:dev" => &["gh_access_token"],
        "gh:release" => &["gh_access_token_alabsystems"],
        "site-deployer-key" => {
            &["~/company-life/config/local/service-account.alab-site-deployer.json"]
        }
        _ => &[],
    }
}

/// HOW `want` RESOLVES over `registry` (`None`: it could not be read, which is no empty one, so
/// the want is filed and the ledger answers from what this Mac holds): held where the registry
/// holds an entry [`entries_of`] names, else filed by `file` (or why it could not be).
pub async fn resolve_over<F, Fut>(
    registry: Option<&[VaultEntry]>,
    want: &str,
    file: F,
) -> Resolution
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    let names = entries_of(want);
    let held: Vec<VaultEntry> = registry
        .unwrap_or_default()
        .iter()
        .filter(|e| names.contains(&e.name.as_str()))
        .cloned()
        .collect();
    if !held.is_empty() {
        return Resolution::Held(held);
    }
    match file(want.to_string()).await {
        Ok(()) => Resolution::Filed,
        Err(why) => Resolution::Failed(why),
    }
}

/// THE ONE PATH: each of `wants` not settled in `known` (the session's resolutions) resolved
/// through `aterm` (`program`): the registry read once, each want missing from it filed. Answers
/// the read of all `wants`, `known` updated.
pub async fn resolve(
    program: &Path,
    wants: &[String],
    known: &mut BTreeMap<String, Resolution>,
) -> CredentialRead {
    let unsettled: Vec<&String> = wants
        .iter()
        .filter(|w| !known.get(*w).is_some_and(Resolution::settled))
        .collect();
    if !unsettled.is_empty() {
        let registry = codex_alab_aterm::secrets::registry(program).await.ok();
        for want in unsettled {
            let resolution = resolve_over(registry.as_deref(), want, |w| async move {
                codex_alab_aterm::secrets::need(program, &w).await
            })
            .await;
            known.insert(want.clone(), resolution);
        }
    }
    read_of(wants, known)
}

/// WHAT `known` (the session's resolutions) SAYS OF `wants`, nothing looked up: each want it
/// resolved; none asked where `wants` is empty.
#[must_use]
pub fn read_of(wants: &[String], known: &BTreeMap<String, Resolution>) -> CredentialRead {
    if wants.is_empty() {
        return CredentialRead::NotAsked;
    }
    CredentialRead::Resolved(
        wants
            .iter()
            .filter_map(|w| known.get(w).map(|r| (w.clone(), r.clone())))
            .collect(),
    )
}

/// Words that make a line ask for a credential. A vendor sign-in (log in, sign in) is a person's
/// act at a window, not a vault entry, so it is not among them.
const CREDENTIAL_WORDS: &[&str] = &[
    "credential",
    "credentials",
    "token",
    "tokens",
    "keychain",
    "password",
    "passwords",
    "passphrase",
    "passphrases",
    "passcode",
    "passcodes",
    "secret",
    "secrets",
    "certificate",
    "certificates",
    "cert",
    "certs",
    "devid",
    "pat",
];

/// A key is a credential only when its noun phrase says what kind: a keyboard key or a
/// security-key gesture still needs the person. Possessives such as "deployer's key" count.
const KEY_KINDS: &[&str] = &[
    "api",
    "ssh",
    "private",
    "public",
    "signing",
    "codesign",
    "codesigning",
    "gpg",
    "github",
    "access",
    "secret",
    "deploy",
    "deployer",
    "deployment",
    "notary",
    "release",
    "account",
    "auth",
    "authentication",
    "encryption",
    "decryption",
    "cryptographic",
];

const CREDENTIAL_PHRASES: &[&str] = &["app specific", "access token", "developer id"];

const NOTARY_WORDS: &[&str] = &[
    "notary",
    "notarize",
    "notarise",
    "notarized",
    "notarised",
    "notarization",
    "notarisation",
    "notarytool",
];

const SIGNING_WORDS: &[&str] = &[
    "signing",
    "codesign",
    "codesigning",
    "signature",
    "devid",
    "certificate",
    "certificates",
    "cert",
    "certs",
];

const RELEASE_WORDS: &[&str] = &["release", "releases", "alabsystems"];

const GITHUB_WORDS: &[&str] = &["github", "agh"];

const DEPLOY_WORDS: &[&str] = &["deploy", "deployer", "deployment", "deploying"];

/// The line's words, lower-cased, each run of letters and digits one word, padded with a space
/// on each side so a phrase is matched whole.
fn words(line: &str) -> String {
    let mut out = String::with_capacity(line.len() + 2);
    out.push(' ');
    for c in line.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.ends_with(' ') {
            out.push(' ');
        }
    }
    if !out.ends_with(' ') {
        out.push(' ');
    }
    out
}

fn has_word(words: &str, list: &[&str]) -> bool {
    list.iter().any(|w| words.contains(&format!(" {w} ")))
}

fn credential_key(words: &str) -> bool {
    let mut before = "";
    let mut previous = "";
    for word in words.split_whitespace() {
        let kind = if previous == "s" { before } else { previous };
        if matches!(word, "key" | "keys") && KEY_KINDS.contains(&kind) {
            return true;
        }
        before = previous;
        previous = word;
    }
    false
}

/// THE WANTS A CREDENTIAL LINE NAMES, as `aterm secrets need` spells them, and [`RELEASE_WANT`]
/// for the release machine's: `None` where `line` asks for no credential (a decision, a grant, a
/// vendor sign-in), `Some` of none where it asks for one the vocabulary cannot name.
#[must_use]
pub fn wants_of(line: &str) -> Option<Vec<&'static str>> {
    let w = words(line);
    let notary = has_word(&w, NOTARY_WORDS) || w.contains(" app specific ");
    let credential = notary
        || has_word(&w, CREDENTIAL_WORDS)
        || credential_key(&w)
        || CREDENTIAL_PHRASES
            .iter()
            .any(|p| w.contains(&format!(" {p} ")));
    if !credential {
        return None;
    }
    let release = has_word(&w, RELEASE_WORDS);
    let github = has_word(&w, GITHUB_WORDS);
    let signing = has_word(&w, SIGNING_WORDS) || w.contains(" developer id ");
    let mut wants = Vec::new();
    if notary {
        wants.push("notary");
    }
    if github {
        wants.push(if release { "gh:release" } else { "gh:dev" });
    }
    if signing || (release && !github) {
        wants.push(RELEASE_WANT);
    }
    if has_word(&w, DEPLOY_WORDS) || w.contains(" service account ") {
        wants.push("site-deployer-key");
    }
    Some(wants)
}

/// Whether `line` asks for a credential ([`wants_of`]).
#[must_use]
pub fn is_credential(line: &str) -> bool {
    wants_of(line).is_some()
}

/// The wants every line of `lines` names, each once, [`RELEASE_WANT`] left out: the ones
/// codex-alab resolves.
#[must_use]
pub fn host_wants(lines: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for want in lines.iter().filter_map(|l| wants_of(l)).flatten() {
        if want != RELEASE_WANT && !out.iter().any(|w| w == want) {
            out.push(want.to_owned());
        }
    }
    out
}

/// One thing the worker is told, keyed so it is told once a session.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Fact {
    Held(VaultEntry),
    Release,
    Filed(String),
    Failed(String, String),
    /// A want codex-alab did not resolve, or the vocabulary cannot name (`<want>`).
    File(String),
}

impl Fact {
    fn key(&self) -> String {
        match self {
            Self::Held(entry) => format!("held:{}", entry.name),
            Self::Release => RELEASE_WANT.to_owned(),
            Self::Filed(want) => format!("filed:{want}"),
            Self::Failed(want, _) => format!("failed:{want}"),
            Self::File(want) => format!("file:{want}"),
        }
    }
}

/// The most UTF-16 code units one entry's description takes.
const ENTRY_UNITS: usize = 110;

/// The most entries one line names.
const NAMED_ENTRIES: usize = 4;

fn described(entry: &VaultEntry) -> String {
    let text = match entry.pointer.as_deref().filter(|p| *p != entry.name) {
        Some(pointer) => format!("`{}` at `{pointer}` ({})", entry.name, entry.purpose),
        None => format!("`{}` ({})", entry.name, entry.purpose),
    };
    crate::words::fit_units(&text, ENTRY_UNITS).into_owned()
}

/// The line's opening.
pub const CREDENTIAL_HEAD: &str = concat!(
    harness_mark!(),
    " A credential is no NEEDS YOU, so codex-alab took that line off the person's list."
);

/// The line's close.
pub const CREDENTIAL_TAIL: &str = "Record any step it blocks and continue.";

/// The clause for a release, signing or notarized-build need.
pub const RELEASE_MACHINE: &str = "Releases are cut on the release machine.";

/// THE LINE THAT TELLS THE WORKER how its credential `lines` resolved by `read`, saying only what
/// `told` does not hold yet, and the keys it tells; `None` where every fact was told.
#[must_use]
pub fn told_line(
    lines: &[String],
    read: &CredentialRead,
    told: &BTreeSet<String>,
) -> Option<(String, Vec<String>)> {
    let mut facts: Vec<Fact> = Vec::new();
    let mut add = |fact: Fact| {
        if !told.contains(&fact.key()) && !facts.contains(&fact) {
            facts.push(fact);
        }
    };
    for line in lines {
        let Some(wants) = wants_of(line) else {
            continue;
        };
        if wants.is_empty() {
            add(Fact::File("<want>".to_owned()));
        }
        for want in wants {
            if want == RELEASE_WANT {
                add(Fact::Release);
                continue;
            }
            match read.of(want) {
                Some(Resolution::Held(entries)) if !entries.is_empty() => {
                    for entry in entries {
                        add(Fact::Held(entry.clone()));
                    }
                }
                Some(Resolution::Filed) => add(Fact::Filed(want.to_owned())),
                Some(Resolution::Failed(why)) => add(Fact::Failed(want.to_owned(), why.clone())),
                Some(Resolution::Held(_)) | None => add(Fact::File(want.to_owned())),
            }
        }
    }
    if facts.is_empty() {
        return None;
    }
    let keys = facts.iter().map(Fact::key).collect();
    let held: Vec<String> = facts
        .iter()
        .filter_map(|f| match f {
            Fact::Held(e) => Some(described(e)),
            _ => None,
        })
        .collect();
    let mut parts: Vec<String> = Vec::new();
    if !held.is_empty() {
        let more = held.len().saturating_sub(NAMED_ENTRIES);
        let mut named = held.into_iter().take(NAMED_ENTRIES).collect::<Vec<_>>();
        if more > 0 {
            named.push(format!("{more} more (`aterm secrets ls --describe`)"));
        }
        parts.push(format!("The vault holds {}.", named.join("; ")));
    }
    if facts.contains(&Fact::Release) {
        parts.push(RELEASE_MACHINE.to_owned());
    }
    let filed: Vec<&str> = facts
        .iter()
        .filter_map(|f| match f {
            Fact::Filed(w) => Some(w.as_str()),
            _ => None,
        })
        .collect();
    if !filed.is_empty() {
        parts.push(format!(
            "codex-alab filed `aterm secrets need {}`.",
            filed.join("`, `aterm secrets need ")
        ));
    }
    for fact in &facts {
        match fact {
            Fact::Failed(want, why) => parts.push(format!(
                "codex-alab could not file `aterm secrets need {want}` ({}).",
                crate::words::fit_units(why.trim(), 120)
            )),
            Fact::File(want) => parts.push(format!("File it: `aterm secrets need {want}`.")),
            _ => {}
        }
    }
    parts.push(CREDENTIAL_TAIL.to_owned());
    Some((format!("{CREDENTIAL_HEAD} {}", parts.join(" ")), keys))
}

#[cfg(test)]
#[path = "credential_need_tests.rs"]
mod tests;
