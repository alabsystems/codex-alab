// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Independent identities for the ALab package and the upstream source it includes.
//!
//! Upstream's main branch calls its workspace `0.0.0`. The final executable captures
//! ALab's package versions and upstream provenance, then installs that immutable
//! identity before calling libraries. Libraries keep their stable Cargo identities.
//! Neither a source build nor a source archive invents a release when those are absent.
//!
//! A package carries two versions (alab/README.md, Decision 8):
//!
//! * the **version**, codex-alab's own line (`VERSION`, restarted at `0.1.0`): what codex-alab
//!   says it is to a person and to aterm ([`product_version`], [`alab`]): `codex --version`,
//!   the banner, `/alab version`, the session record, the package's records. From the version
//!   ledger (`alab/versions.toml`) on it is COMPOUND: the upstream Codex baseline rides in its
//!   SemVer build metadata (`0.1.0-dev.<count>+codex.0.164.0.g<sha9>`), carried here verbatim
//!   and never parsed, so it reaches every reader whole;
//! * the **client version**, upstream's numbering, held at or above the next minor past the
//!   newest upstream stable the source merged ([`running_version`]): what upstream's code
//!   and OpenAI's servers see (`client_version`, the user agent, telemetry, the rollout's
//!   `cli_version`). The server picks models by it, so it must never call this build older
//!   than the upstream code it holds.
//!
//! A package from before codex-alab's own line has one version, the client version, and
//! [`product_version`] falls back to it.

use std::cmp::Ordering;
use std::fmt;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RunningIdentity {
    /// The client version: what upstream's code and OpenAI's servers see.
    version: &'static str,
    /// codex-alab's own version, when the package names one apart from the client version.
    product: Option<&'static str>,
    upstream_release: Option<&'static str>,
    upstream_commit: Option<&'static str>,
}

static RUNNING_IDENTITY: OnceLock<RunningIdentity> = OnceLock::new();

/// An ALab product is updated by atpkg regardless of its version spelling. This
/// uses the final executable's immutable identity, never environment or a selected
/// package's mutable metadata. Upstream/source executables keep their own policy.
pub fn vendor_updates_allowed() -> bool {
    vendor_updates_for(RUNNING_IDENTITY.get().copied())
}

fn vendor_updates_for(identity: Option<RunningIdentity>) -> bool {
    identity.is_none_or(|identity| identity.product.is_none())
}

#[cfg(test)]
mod update_ownership_tests {
    use super::*;

    #[test]
    fn regular_and_development_product_versions_have_the_same_update_owner() {
        assert!(vendor_updates_for(None));
        let mut identity = RunningIdentity {
            version: "0.168.0",
            product: None,
            upstream_release: None,
            upstream_commit: None,
        };
        assert!(vendor_updates_for(Some(identity)));
        for product in ["0.5.0", "0.4.0-dev.15849+codex.0.167.1.ge201a2003"] {
            identity.product = Some(product);
            assert!(!vendor_updates_for(Some(identity)));
        }
    }
}

/// Capture package identity in the final executable, leaving library inputs stable.
///
/// The expansion returns the result of [`initialize_package`]. `CODEX_ALAB_BUILD_VERSION`
/// is the client version (its meaning before codex-alab had a line of its own, so a
/// historical source given it still tells the server what it told it then);
/// `CODEX_ALAB_PRODUCT_VERSION` is codex-alab's own version. Source builds without the
/// package input keep their entry point's Cargo version for both.
#[macro_export]
macro_rules! initialize_running_build {
    () => {
        $crate::version::initialize_package(
            option_env!("CODEX_ALAB_BUILD_VERSION").unwrap_or(env!("CARGO_PKG_VERSION")),
            option_env!("CODEX_ALAB_PRODUCT_VERSION"),
            option_env!("CODEX_ALAB_UPSTREAM_VERSION"),
            option_env!("CODEX_ALAB_UPSTREAM_COMMIT"),
        )
    };
}

/// A different identity was already installed by this executable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IdentityConflict;

impl fmt::Display for IdentityConflict {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("the running build identity was already initialized differently")
    }
}

impl std::error::Error for IdentityConflict {}

/// Install the version and provenance captured at the final executable's call site, for a
/// package with one version (the client version, also reported as codex-alab's own).
///
/// This does not activate ALab behavior. Repeating the same identity succeeds.
/// No runtime environment or selected-package metadata supplies these values.
///
/// # Errors
/// Returns [`IdentityConflict`] if a different identity is already installed.
pub fn initialize(
    version: &'static str,
    upstream_release: Option<&'static str>,
    upstream_commit: Option<&'static str>,
) -> Result<(), IdentityConflict> {
    initialize_package(version, None, upstream_release, upstream_commit)
}

/// Install the client version `client`, codex-alab's own version `product` (none, or empty:
/// the client version is reported for both) and the upstream provenance captured at the
/// final executable's call site.
///
/// # Errors
/// Returns [`IdentityConflict`] if a different identity is already installed.
pub fn initialize_package(
    client: &'static str,
    product: Option<&'static str>,
    upstream_release: Option<&'static str>,
    upstream_commit: Option<&'static str>,
) -> Result<(), IdentityConflict> {
    initialize_with(
        &RUNNING_IDENTITY,
        RunningIdentity {
            version: client,
            product: nonempty(product),
            upstream_release: nonempty(upstream_release),
            upstream_commit: nonempty(upstream_commit),
        },
    )
}

fn initialize_with(
    cell: &OnceLock<RunningIdentity>,
    identity: RunningIdentity,
) -> Result<(), IdentityConflict> {
    if let Err(identity) = cell.set(identity)
        && cell.get() != Some(&identity)
    {
        return Err(IdentityConflict);
    }
    Ok(())
}

/// The executable's full CLIENT version (upstream's numbering: what upstream's code and
/// OpenAI's servers see), or the caller's Cargo version before initialization.
///
/// Reading the fallback never initializes the identity. Tests and unstamped entry
/// points retain their caller's version without preventing later initialization.
pub fn running_version(fallback: &'static str) -> &'static str {
    version_with(&RUNNING_IDENTITY, fallback)
}

fn version_with(cell: &OnceLock<RunningIdentity>, fallback: &'static str) -> &'static str {
    cell.get().map_or(fallback, |identity| identity.version)
}

/// The executable's full version as codex-alab reports itself (`codex --version`, the
/// session record): its own line when the package names one, else the client version, or
/// the caller's Cargo version before initialization. Never sent to OpenAI's servers.
pub fn product_version(fallback: &'static str) -> &'static str {
    product_with(&RUNNING_IDENTITY, fallback)
}

fn product_with(cell: &OnceLock<RunningIdentity>, fallback: &'static str) -> &'static str {
    cell.get().map_or(fallback, |identity| {
        identity.product.unwrap_or(identity.version)
    })
}

/// The running CLI's numeric version: `raw` cut at its first `-` or `+`
/// (`1.4.13-R6996.1` → `1.4.13`, `0.165.0-dev.4+g1a2b3c4d5` → `0.165.0`), without a leading `v`;
/// `None` where what is left is not dot-joined numbers (`2.1.`, `2..1`, `latest`).
#[must_use]
pub fn cli_version_core(raw: &str) -> Option<&str> {
    let raw = raw.trim();
    let raw = raw.strip_prefix('v').unwrap_or(raw);
    let core = raw.split(['-', '+']).next().unwrap_or("");
    core.split('.')
        .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
        .then_some(core)
}

/// What a CLI version names past its numbers ([`cli_version_core`]): its pre-release part, cut
/// at its build metadata (`1.4.10-R6500.1` → `R6500.1`, `0.164.0-dev.12521+g1a2b3c4d5` →
/// `dev.12521`); `None` where it has none.
#[must_use]
pub fn cli_version_build(raw: &str) -> Option<&str> {
    cli_version_core(raw)?;
    let raw = raw.trim();
    let (_, rest) = raw.split_once('-')?;
    if raw
        .find('+')
        .is_some_and(|plus| plus < raw.len() - rest.len())
    {
        return None;
    }
    let build = rest.split('+').next().unwrap_or("");
    (!build.is_empty()).then_some(build)
}

/// Whether a move from CLI build `from` onto `to` is one a top rule's ⬆️ offers: a later core
/// ([`cli_version_later`]), or the same core with a later build past it ([`cli_build_later`]: a
/// later Muse revision, a later dev count); never an earlier core, an earlier build under the
/// same core, the same build, or a side that is not a version. It allocates nothing.
#[must_use]
pub fn cli_version_offered(to: &str, from: &str) -> bool {
    if cli_version_core(to).is_none() || cli_version_core(from).is_none() {
        return false;
    }
    cli_version_later(to, from) || (!cli_version_later(from, to) && cli_build_later(to, from))
}

/// Whether CLI version `to`'s build past its core ([`cli_version_build`]) is later than
/// `from`'s: the two are read as runs of digits and runs of other characters and compared run
/// by run, digits as numbers and the others alike or not ordered at all, a build that goes on
/// past the other's end the later (`R6500.1` after `R6427.1`, `R6427.1` after `R6427`,
/// `dev.12521` after `dev.12365`); `false` for builds of other shapes (`R6500.1` against
/// `dev.3`) and where either side has none. It allocates nothing.
#[must_use]
pub fn cli_build_later(to: &str, from: &str) -> bool {
    let (Some(to), Some(from)) = (cli_version_build(to), cli_version_build(from)) else {
        return false;
    };
    let (mut a, mut b) = (Runs(to.as_bytes()), Runs(from.as_bytes()));
    let mut order = Ordering::Equal;
    loop {
        let (x, y) = match (a.next(), b.next()) {
            (None, _) => return order == Ordering::Greater,
            (Some(_), None) => return order != Ordering::Less,
            (Some(x), Some(y)) => (x, y),
        };
        match (x[0].is_ascii_digit(), y[0].is_ascii_digit()) {
            (true, true) => {
                // A run of digits as a number of any size: its digits past the leading zeros,
                // ordered by their count, then as text.
                fn number(run: &[u8]) -> (usize, &[u8]) {
                    let lead = run.iter().take_while(|&&d| d == b'0').count();
                    let rest = &run[lead..];
                    (rest.len(), rest)
                }
                if order == Ordering::Equal {
                    order = number(x).cmp(&number(y));
                }
            }
            (false, false) if x == y => {}
            _ => return false,
        }
    }
}

/// A build's maximal runs of digits and of other bytes, in order ([`cli_build_later`]).
struct Runs<'a>(&'a [u8]);

impl<'a> Runs<'a> {
    fn next(&mut self) -> Option<&'a [u8]> {
        let first = *self.0.first()?;
        let digit = first.is_ascii_digit();
        let len = self
            .0
            .iter()
            .take_while(|b| b.is_ascii_digit() == digit)
            .count();
        let (run, rest) = self.0.split_at(len);
        self.0 = rest;
        Some(run)
    }
}

/// Whether CLI version `to` is numerically later than `from` (their [`cli_version_core`]s
/// compared part by part, a missing part reading 0); `false` where either is not a version.
/// It allocates nothing: a top rule asks it on every frame that draws one.
#[must_use]
pub fn cli_version_later(to: &str, from: &str) -> bool {
    fn number(part: Option<&str>) -> (usize, &str) {
        let stripped = part.unwrap_or("").trim_start_matches('0');
        (stripped.len(), stripped)
    }
    let (Some(to), Some(from)) = (cli_version_core(to), cli_version_core(from)) else {
        return false;
    };
    let (mut a, mut b) = (to.split('.'), from.split('.'));
    loop {
        let (x, y) = (a.next(), b.next());
        if x.is_none() && y.is_none() {
            return false;
        }
        match number(x).cmp(&number(y)) {
            Ordering::Equal => {}
            later => return later == Ordering::Greater,
        }
    }
}

/// The running ALab package's own version ([`product_version`]), or `dev` for an unstamped
/// source build.
pub fn alab() -> &'static str {
    match product_version(env!("CARGO_PKG_VERSION")) {
        "0.0.0" => "dev",
        version => version,
    }
}

/// The line `codex exec` opens its report to a person with, in place of upstream's `OpenAI Codex
/// v<version>`: codex-alab's own version ([`product_version`]) and the upstream release it is
/// based on, as the TUI's title names them (Decision 8: the client version is never what
/// codex-alab calls itself). Where ALab is not active, upstream's words with codex-alab's own
/// version, as the TUI's inactive header says it.
pub fn exec_title(fallback: &'static str) -> String {
    let version = product_version(fallback);
    if crate::active() {
        format!(
            "codex-alab v{version} · based on OpenAI Codex {}",
            upstream_release().unwrap_or("unreleased")
        )
    } else {
        format!("OpenAI Codex v{version}")
    }
}

/// The newest upstream stable release whose branch point this build includes.
/// This is a source baseline, not a claim that upstream main equals that release.
pub fn upstream_release() -> Option<&'static str> {
    RUNNING_IDENTITY
        .get()
        .and_then(|identity| identity.upstream_release)
}

/// The exact merged upstream source commit, independently of the ALab package commit.
pub fn upstream_commit() -> Option<&'static str> {
    RUNNING_IDENTITY
        .get()
        .and_then(|identity| identity.upstream_commit)
}

/// Compact upstream provenance. The commit suffix distinguishes main from a release.
pub fn upstream() -> String {
    let baseline = upstream_release().unwrap_or("unreleased");
    match upstream_commit() {
        Some(commit) => format!(
            "{baseline}+g{}",
            commit.chars().take(12).collect::<String>()
        ),
        None => baseline.to_owned(),
    }
}

fn nonempty(value: Option<&'static str>) -> Option<&'static str> {
    value.filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fallback_read_cannot_commit_the_running_build_identity() {
        let cell = OnceLock::new();
        assert_eq!(version_with(&cell, "0.0.0"), "0.0.0");
        assert_eq!(version_with(&cell, "1.2.3"), "1.2.3");
        assert!(cell.get().is_none());
        let identity = RunningIdentity {
            version: "0.165.0-dev.1+gaaaaaaaaa",
            product: None,
            upstream_release: Some("0.164.0"),
            upstream_commit: Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        };
        assert_eq!(initialize_with(&cell, identity), Ok(()));
        assert_eq!(version_with(&cell, "0.0.0"), identity.version);
        assert_eq!(cell.get(), Some(&identity));
    }

    #[test]
    fn the_full_running_identity_is_idempotent_and_refuses_conflicting_replacement() {
        let cell = OnceLock::new();
        let first = RunningIdentity {
            version: "0.165.0-dev.1+gaaaaaaaaa",
            product: None,
            upstream_release: Some("0.164.0"),
            upstream_commit: Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        };
        assert_eq!(initialize_with(&cell, first), Ok(()));
        assert_eq!(initialize_with(&cell, first), Ok(()));
        for replacement in [
            RunningIdentity {
                version: "0.165.0-dev.2+gbbbbbbbbb",
                ..first
            },
            RunningIdentity {
                upstream_release: Some("0.165.0"),
                ..first
            },
            RunningIdentity {
                upstream_commit: Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
                ..first
            },
        ] {
            assert_eq!(initialize_with(&cell, replacement), Err(IdentityConflict));
            assert_eq!(cell.get(), Some(&first));
        }
    }

    /// Decision 8: codex-alab's own version is what it reports as itself, while the client
    /// version, upstream's numbering, is what every server-facing reader takes. Neither
    /// read may return the other's line.
    #[test]
    fn the_product_version_is_reported_and_the_client_version_is_sent() {
        let cell = OnceLock::new();
        assert_eq!(product_with(&cell, "0.0.0"), "0.0.0");
        let identity = RunningIdentity {
            version: "0.165.0-dev.13600+gaaaaaaaaa",
            product: Some("0.1.0-dev.13600+gaaaaaaaaa"),
            upstream_release: Some("0.164.0"),
            upstream_commit: None,
        };
        assert_eq!(initialize_with(&cell, identity), Ok(()));
        assert_eq!(version_with(&cell, "0.0.0"), "0.165.0-dev.13600+gaaaaaaaaa");
        assert_eq!(product_with(&cell, "0.0.0"), "0.1.0-dev.13600+gaaaaaaaaa");
        // The same client version with another (or no) product is another identity.
        for replacement in [
            RunningIdentity {
                product: Some("0.2.0-dev.13600+gaaaaaaaaa"),
                ..identity
            },
            RunningIdentity {
                product: None,
                ..identity
            },
        ] {
            assert_eq!(initialize_with(&cell, replacement), Err(IdentityConflict));
        }
        assert_eq!(product_with(&cell, "0.0.0"), "0.1.0-dev.13600+gaaaaaaaaa");
    }

    /// Decision 8, amended 2026-10-24: a compound version (the upstream Codex baseline in the
    /// build metadata) is reported whole, and the client version beside it is sent as it was.
    #[test]
    fn a_compound_version_is_reported_whole_and_the_client_version_is_sent_unchanged() {
        let cell = OnceLock::new();
        let identity = RunningIdentity {
            version: "0.165.0-dev.13600+gaaaaaaaaa",
            product: Some("0.1.0-dev.13600+codex.0.164.0.gaaaaaaaaa"),
            upstream_release: Some("0.164.0"),
            upstream_commit: None,
        };
        assert_eq!(initialize_with(&cell, identity), Ok(()));
        assert_eq!(
            product_with(&cell, "0.0.0"),
            "0.1.0-dev.13600+codex.0.164.0.gaaaaaaaaa"
        );
        assert_eq!(version_with(&cell, "0.0.0"), "0.165.0-dev.13600+gaaaaaaaaa");
    }

    /// A package from before codex-alab's own line names one version: it is reported, and
    /// sent, as it was.
    #[test]
    fn a_package_with_one_version_reports_its_client_version() {
        let cell = OnceLock::new();
        let identity = RunningIdentity {
            version: "0.165.0-dev.13595+ga109d7cc6",
            product: nonempty(Some("")),
            upstream_release: None,
            upstream_commit: None,
        };
        assert_eq!(identity.product, None);
        assert_eq!(initialize_with(&cell, identity), Ok(()));
        assert_eq!(product_with(&cell, "0.0.0"), "0.165.0-dev.13595+ga109d7cc6");
        assert_eq!(version_with(&cell, "0.0.0"), "0.165.0-dev.13595+ga109d7cc6");
    }

    #[test]
    fn a_cli_version_is_cut_to_its_numbers() {
        for (raw, core) in [
            ("2.1.305", Some("2.1.305")),
            ("v2.1.310", Some("2.1.310")),
            ("1.4.13-R6996.1", Some("1.4.13")),
            ("0.165.0-dev.4+g1a2b3c4d5", Some("0.165.0")),
            ("0.1.0-dev.3+codex.0.165.0.g1a2b3c4d5", Some("0.1.0")),
            (" 2.1.305 ", Some("2.1.305")),
            ("", None),
            ("?", None),
            ("latest", None),
            ("-1", None),
            ("2.1.", None),
            ("2..1", None),
            (".2.1", None),
            ("2.1.305.", None),
            ("v", None),
        ] {
            assert_eq!(cli_version_core(raw), core, "{raw:?}");
        }
        assert!(
            !cli_version_later("2.1.", "2.1.0") && !cli_version_later("2.1.0", "2.1."),
            "a malformed core is no version on either side"
        );
        assert!(cli_version_later("2.1.310", "2.1.305"));
        assert!(cli_version_later("2.1.310", "2.1.31"), "numeric, not text");
        assert!(cli_version_later("2.2", "2.1.999"));
        assert!(cli_version_later("1.4.14-R7000.1", "1.4.13-R6996.1"));
        assert!(!cli_version_later("2.1.305", "2.1.305"));
        assert!(!cli_version_later("2.1.305", "2.1.310"));
        assert!(!cli_version_later("2.1.305.0", "2.1.305"));
        assert!(!cli_version_later("latest", "2.1.305"));
        assert!(!cli_version_later("2.1.310", ""));
    }

    #[test]
    fn a_revision_or_dev_build_under_one_core_is_offered_never_a_step_back() {
        for (to, from) in [
            ("2.1.310", "2.1.305"),
            ("1.4.10-R6500.1", "1.4.10-R6427.1"),
            ("1.4.10-R6427.1", "1.4.10-R6427"),
            ("1.4.10-R6427.10", "1.4.10-R6427.9"),
            ("0.164.0-dev.12521+gabc", "0.164.0-dev.12365+gdef"),
            (
                "0.1.0-dev.13765+codex.0.164.0.g5d4c3b2a1",
                "0.1.0-dev.13764+codex.0.164.0.g4b3a367a0",
            ),
            ("1.4.11-R6500.1", "1.4.10-R6427.1"),
        ] {
            assert!(cli_version_offered(to, from), "{from} -> {to}");
        }
        for (to, from) in [
            ("2.1.305", "2.1.305"),
            ("v2.1.305", "2.1.305"),
            ("2.1.305", "2.1.310"),
            ("1.4.9-R6999.1", "1.4.10-R6427.1"),
            ("1.4.10-R6427.1", "1.4.10-R6500.1"),
            ("1.4.10-R6427", "1.4.10-R6427.1"),
            ("1.4.10-R06427.1", "1.4.10-R6427.1"),
            ("0.164.0-dev.12365+gdef", "0.164.0-dev.12521+gabc"),
            (
                "0.1.0-dev.13764+codex.0.164.0.g4b3a367a0",
                "0.1.0-dev.13765+codex.0.164.0.g5d4c3b2a1",
            ),
            (
                "0.1.0-dev.13764+codex.0.165.0.g5d4c3b2a1",
                "0.1.0-dev.13764+codex.0.164.0.g4b3a367a0",
            ),
            ("1.4.10-R6500.1", "1.4.10-dev.3"),
            ("1.4.10-R6500.1", "1.4.10"),
            ("latest", "2.1.305"),
            ("2.1.310", ""),
            ("2.1.", "2.1.0"),
        ] {
            assert!(!cli_version_offered(to, from), "{from} -> {to}");
        }
        assert!(
            !cli_version_later("1.4.10-R6500.1", "1.4.10-R6427.1"),
            "CONTROL: the cores alone are equal"
        );
        assert!(cli_version_later(
            "1.4.99999999999999999999999999",
            "1.4.99999999999999999999999998"
        ));
        assert!(!cli_version_offered(
            "1.4.99999999999999999999999998-R6500.1",
            "1.4.99999999999999999999999999-R6427.1"
        ));
        for (raw, build) in [
            ("1.4.10-R6500.1", Some("R6500.1")),
            ("0.164.0-dev.12521+g1a2b3c4d5", Some("dev.12521")),
            ("0.1.0-dev.3+codex.0.165.0.g1a2", Some("dev.3")),
            ("2.1.305", None),
            ("2.1.305+gabc-1", None),
            ("1.4.10-", None),
            ("latest-1", None),
        ] {
            assert_eq!(cli_version_build(raw), build, "{raw:?}");
        }
    }

    #[test]
    fn a_numeric_build_difference_still_checks_the_remaining_paired_shape() {
        for (to, from) in [
            ("1.4.10-R6500.foo", "1.4.10-R6400.bar"),
            ("1.4.10-R6500.2-extra", "1.4.10-R6400.1-other"),
            ("0.1.0-dev.13765.alpha", "0.1.0-dev.13764.beta"),
        ] {
            assert!(
                !cli_version_offered(to, from),
                "different later shape: {from} -> {to}"
            );
            assert!(
                !cli_version_offered(from, to),
                "different earlier shape: {to} -> {from}"
            );
        }
        for (to, from) in [
            ("1.4.10-R6500.1", "1.4.10-R6427.9"),
            ("1.4.10-R6500", "1.4.10-R6427.9"),
        ] {
            assert!(
                cli_version_offered(to, from),
                "compatible shape: {from} -> {to}"
            );
            assert!(!cli_version_offered(from, to), "CONTROL: reverse ordering");
        }
    }
}
