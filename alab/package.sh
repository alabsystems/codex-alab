#!/usr/bin/env bash
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
#
# alab/package.sh — build a codex-alab package directory (layout v1, what atpkg and aterm's
# harness read) from a clean commit, advancing this output's selected artifact channel.
#
#   alab/package.sh [--commit <rev>] [--out <dir>] [--keep <n>] [--any-commit] [--rebuild]
#                   [--no-promote]
#   alab/package.sh --version-only [--commit <rev>]   print the version and how it was found
#   alab/package.sh --client-version-only [--commit <rev>]
#                                                     print the client version (what OpenAI's
#                                                     servers are told) and how it was found
#   alab/package.sh --release-check [--commit <rev>]  print <rev>'s VERSION if a release may
#                                                     carry it, else refuse (exit 1)
#   alab/package.sh --release-version [--commit <rev>]
#                                                     the same check; print the version the
#                                                     release is (<VERSION>+codex.<X.Y.Z> once
#                                                     <rev> carries alab/versions.toml)
#   alab/package.sh --ledger-row [--commit <rev>]     print the alab/versions.toml row a
#                                                     release of <rev> appends, else refuse
#   alab/package.sh --upstream-base [--commit <rev>]  print the line a release's page names
#                                                     <rev>'s upstream base with
#
# <rev> defaults to HEAD of this checkout, which must be clean, and must be on origin/main
# (what ALab ships) unless --any-commit is given. Every mode reads <rev>'s history, which git
# must hold whole: a shallow clone cut below a commit with parents and a grafts file are
# refused, and replacement objects are ignored (GIT_NO_REPLACE_OBJECTS). Once <rev> carries
# alab/versions.toml, --release-check, --release-version and --ledger-row also fetch origin's
# main fast-forward only (no origin, or a rewritten main, refuses), and alab/versions.py
# checks the ledger along it, against this clone's memory of it (origin/main's reflog) and
# against every release the public release repository holds (ALAB_RELEASED_REPO, default
# https://github.com/alabsystems/codex-alab.git; publish/pre-promote names the engine's): a
# release is cut from <rev> on origin/main. A commit a package in <out>/codex already
# holds (its alab-build.json "source") is refused unless --rebuild is given; the new one
# is then staged beside it as <version>.rebuilt-<UTC time>. Steps:
#   1. two versions (alab/README.md, Decision 8), both MAJOR.MINOR.0-dev.<count>+g<sha9>
#      (~/publication/VERSIONING.md), <count> git rev-list --count <rev>:
#      - the version, codex-alab's own: MAJOR.MINOR.0 is <rev>'s VERSION (codex-alab's release
#        line, restarted at 0.1.0 and moved only by `pub bump`). It is what the package says
#        it is: codex --version, codex-package.json, alab-build.json "version", the package
#        directory, atpkg's pkg record and the release tag. It is compound once <rev>
#        carries the version ledger, alab/versions.toml:
#        MAJOR.MINOR.0-dev.<count>+codex.<X.Y.Z>.g<sha9>, X.Y.Z the upstream Codex baseline
#        (the upstream stable below, its patch kept; for a published snapshot, the ledger's
#        row for its VERSION), recorded again as alab-build.json "baseline";
#      - the client version, upstream's numbering: MAJOR.MINOR.0 is the higher of <rev>'s
#        alab/client-version and the floor: the next minor above the newest upstream stable
#        tag (refs/upstream-tags/rust-vX.Y.Z) whose branch point (merge-base with
#        upstream/main) <rev> has merged. It is what upstream's code and OpenAI's servers see
#        (client_version, the user agent, the rollout's cli_version), so a build never calls
#        itself older than the upstream code it holds (the server picks models by it), and
#        alab-build.json records it as "client_version". A release is refused while
#        alab/client-version is below the floor (--release-check).
#      A commit from before alab/client-version (VERSION was upstream's numbering then) has
#      one version, the client version, as it was packaged. upstream's tags and main are
#      fetched first (the remote's configured refspecs, fetch only). Beside the versions, never
#      in it: the upstream base, that stable and the upstream commit <rev> merged last (its
#      merge-base with upstream/main), read once for the binary (CODEX_ALAB_UPSTREAM_VERSION
#      and CODEX_ALAB_UPSTREAM_COMMIT), alab-build.json's "upstream" (with <n> and the
#      commit's date) and the line --upstream-base prints, which publish/release-body puts
#      on the source release's page: "Based on upstream openai/codex rust-vX.Y.Z + <n>
#      commits (<sha12>, <date>)", <n> the upstream commits past that stable's branch point;
#   2. a private scratch worktree at <rev>, ~/Developer/codex-alab-pkg-<sha9>.<nonce>, with
#      Cargo's source identities unchanged. The final executables capture the package
#      version from CODEX_ALAB_BUILD_VERSION. Older source without that input keeps the
#      legacy workspace/lock stamp (metadata, contained, writing only the lock). The
#      worktree is removed when the run ends (that worktree only: nothing else is pruned);
#   3. the compiler selected and leased by native atpkg-publish from <rev>'s
#      committed trust.lock (or its authenticated managed default when absent),
#      before any compiler-keyed cache. Explicit compiler overrides refuse.
#      An observational probe must match that selected execution root; contained
#      probes in codex-rs and the cache must record the same compiler;
#      then a release build of codex and codex-code-mode-host (targo --unverified, the
#      lockfile locked, STABLE_GIT_COMMIT=<rev>, CODEX_ALAB_BUILD_VERSION the client version,
#      CODEX_ALAB_PRODUCT_VERSION the version), with upstream's pinned rusty_v8 archives
#      (scripts/codex_package/v8.py, verified against third_party/v8/*_release_manifests.sha256,
#      read-only to the build), contained (lib.sh, alab_contained), with a persistent
#      rust-release-<target>-<compiler id>.noindex target under the locked private package
#      cache. Package versions no longer change library Cargo identities on capable
#      source; fresh source paths and mtimes can still cause recompilation. No target
#      is pruned by this run. Its
#      .rustc_info.json must record that compiler too. A record a contained step
#      wrote is compared as text and never names a program that runs;
#   4. rg built with Trust at the version upstream pins (scripts/codex_package/rg) with
#      PCRE2 (--features pcre2, statically linked, as the vendor rg), from the cache dir and
#      with a cargo home there (~/.cache/codex-alab/cargo-home: cargo install reads the config
#      above its cargo home) so no repo's .cargo/config.toml applies; cached in
#      ~/.cache/codex-alab/rg-<ver>-<compiler id>, rebuilt when either changes; writable only
#      while it is built;
#   5. upstream's scripts/build_codex_package.py stages the package (--strip all) into a
#      hidden <out>/codex/.<version>.partial.*, the only directory it can write (the rg and
#      the binaries it reads are read-only to it). It is copied without following links
#      into a hidden .<version>.staged.* no contained step is ever given. Symlinks, special
#      files and setuid/setgid/sticky or group/world-writable modes are refused;
#      LICENSE, NOTICE (from the commit's objects) and alab-build.json (source commit,
#      version, client version, the upstream base, target, the compiler and whether the store admits it,
#      ripgrep and its features, rusty_v8, the packager's own commit, built_at) are written
#      there;
#   6. a smoke with a scratch CODEX_HOME, each step contained, the staged package read-only:
#      codex --version (the version), the compiled TOML from debug packaged-defaults (no config
#      loading), bin/codex carrying the upstream commit when its ALab code compiles one in and
#      the client version when it is not the version,
#      rg --version (+pcre2),
#      codesign --verify --strict, arm64 and only system libraries on each Mach-O,
#      upstream's layout validator; then the staged package is renamed into place;
#   7. unless --no-promote, a selected ALab artifact in this same <out>/codex advances
#      through atpkg's conditional link, only onto a newer version whose source descends
#      from it on origin/main. Unrelated checkout links, incomplete or unadmitted packages,
#      older/divergent sources and a changed selection are never replaced. ALAB_ATERM names
#      the aterm executable (default: aterm on PATH); it must support --if-checkout and
#      --if-prefix or promotion is refused with the completed package kept. The running
#      session's native idle checks govern its live re-exec; no restart is requested.
#      It prints the link command for an unselected lane, and keeps the newest <n> (3) in
#      <out>/codex, never one atpkg links as codex, one a running process executes from,
#      or one a standing codex-alab session record names (a crash relaunch still owed)
#      (alab/prune.py; it prunes nothing when it cannot tell). The newest is always kept.
#      Records are read only under $CODEX_HOME (this run's), ~/.codex and each directory in
#      ALAB_CODEX_HOMES (colon-separated): aterm strips CODEX_* from the shells it starts, so
#      a session under any other CODEX_HOME is invisible to the pruner, and its package is
#      kept only while linked, running, or among the newest <n>.
# A package only appears once its smoke passed; no contained step, nor a process one leaves
# behind, can write the directory that is published, and no binary a contained step built
# runs outside the sandbox. Contained steps build with the clone's private cargo home
# (<git-common-dir>/alab-cargo-home, or ALAB_CARGO_HOME: lib.sh, alab_cargo_home). <out> defaults to ~/.local/share/aterm/dev-builds/artifacts. Builds
# need ALAB_MIN_FREE_GB (default 35) free on the worktree's volume; the run waits for it up
# to ALAB_DISK_WAIT_MINUTES (default 120). The guardian checks build/cache/output/temp
# volumes once a second and stops the owned process group nonzero below 20 GiB. Below 20 GiB
# it refuses before launch; it does not enter the 35 GiB admission wait.
# A resource stop does not remove the currently selected package.
# ALAB_ATPKG_PUBLISH names the qualified native compiler selector; caller
# ALAB_CARGO overrides are refused by that selector.
# ALAB_PYTHON a Python >=
# 3.11, ALAB_RELEASED_REPO the public release repository the ledger check reads,
# ALAB_PACKAGE_CACHE names an absolute private package cache (default
# ~/.cache/codex-alab), and ALAB_PKG_KEEP_WORKTREE=1 keeps the scratch source worktree
# for a person to look at. Compiler-keyed release targets remain in the cache.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=alab/lib.sh
. "${repo_root}/alab/lib.sh"

# Publication is reached only after every smoke check passed. A failed rename cannot
# attempt selection, and --no-promote leaves both the selected package and its link alone.
alab_publish_package() {
  local python="$1" staged="$2" published="$3" root="$4" prefix="$5" source="$6" promote="$7"
  # This boundary receives the fully smoke-validated package. Keep it even if the
  # destination appeared meanwhile or selection fails, without changing other cleanup.
  publication_staged="${staged}"
  "${python}" -I "${repo_root}/alab/publish_directory.py" \
    --staged "${staged}" --published "${published}" || return 1
  if [[ "${promote}" -eq 1 ]]; then
    "${python}" -I "${repo_root}/alab/promote.py" --repo "${repo_root}" --root "${root}" \
      --prefix "${prefix}" --package "${published}" --commit "${source}" || return 1
  else
    alab_say "local update: --no-promote; the selected build is unchanged"
  fi
}

# The local artifact selector retains its explicit status field. Set it only
# after the observation matches the native-held execution root; pack-package
# independently authenticates source, signed payload and compiler bytes.
package_record_native_compiler() {
  "$1" -I -c '
import json, os, sys
path, expected = sys.argv[1:]
with open(path) as stream:
    record = json.load(stream)
observed = record.get("sysroot")
if not isinstance(observed, str) or not os.path.isabs(observed) or os.path.realpath(observed) != expected:
    raise SystemExit("the observed compiler differs from the native selected execution root")
record["admitted"] = True
record["admitted_why"] = "native project selection matched its held execution root"
with open(path, "w") as stream:
    json.dump(record, stream)
' "$2" "$3"
}

# alab_build_json <python> <out> <compiler.json> <source> <version> <target> <ripgrep>
#   <ripgrep_features> <rusty_v8> <packager> <built_at>: write alab-build.json to <out>. Its
# "upstream" block is what the release build was given: CODEX_ALAB_UPSTREAM_VERSION (the
# stable X.Y.Z, recorded as its tag rust-vX.Y.Z) and CODEX_ALAB_UPSTREAM_COMMIT (40 hex),
# with CODEX_ALAB_UPSTREAM_SINCE (the upstream commits past that stable's branch point, the
# <n> of the --upstream-base line, recorded as "commits_since_release") and
# CODEX_ALAB_UPSTREAM_DATE (that commit's committer date, UTC) when set. Neither of the first
# two set writes no block: a published snapshot has no history to name a base from, and a
# package made before the block carries none either, so a reader takes a missing block as
# unknown (and a block without a count or a date as one made before them). One without the
# other, or a malformed value, is refused. CODEX_ALAB_CLIENT_VERSION, when set, is the client
# version the release build compiled in (alab/README.md, Decision 8), recorded right after
# "version" as "client_version" in the same dev form and the same <count> and commit; a
# record without it (a commit from before codex-alab's own line) has one version, which is
# also its client version. CODEX_ALAB_BASELINE, when set, is the upstream Codex baseline a
# compound version names (<VERSION>-dev.<count>+codex.<X.Y.Z>.g<sha9>, Decision 8), recorded
# after "client_version" as "baseline": it must be the version's own +codex.<X.Y.Z> and, when
# the build compiled one in, CODEX_ALAB_UPSTREAM_VERSION; a compound version without it, or it
# without a compound version, is refused.
alab_build_json() {
  local python="$1"
  shift
  "${python}" -I -c '
import json, os, re, sys
out, compiler = sys.argv[1], json.load(open(sys.argv[2]))
keys = ["source", "version", "target", "ripgrep", "ripgrep_features", "rusty_v8", "packager", "built_at"]
d = dict(zip(keys, sys.argv[3:]))
release, commit, since, date = (os.environ.get("CODEX_ALAB_UPSTREAM_" + k, "")
                                for k in ("VERSION", "COMMIT", "SINCE", "DATE"))
number = r"(?:0|[1-9][0-9]{0,8})"
upstream = None
if release or commit:
    if not re.fullmatch(number + r"\." + number + r"\." + number, release):
        sys.exit(f"alab-build.json: CODEX_ALAB_UPSTREAM_VERSION {release!r} is not an upstream stable X.Y.Z")
    if not re.fullmatch(r"[0-9a-f]{40}", commit):
        sys.exit(f"alab-build.json: CODEX_ALAB_UPSTREAM_COMMIT {commit!r} is not a full commit sha")
    upstream = {"release": "rust-v" + release}
    if since:
        if not re.fullmatch(r"0|[1-9][0-9]*", since):
            sys.exit(f"alab-build.json: CODEX_ALAB_UPSTREAM_SINCE {since!r} is not a count of commits")
        upstream["commits_since_release"] = int(since)
    upstream["commit"] = commit
    if date:
        if not re.fullmatch(r"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z", date):
            sys.exit(f"alab-build.json: CODEX_ALAB_UPSTREAM_DATE {date!r} is not a UTC ISO-8601 time")
        upstream["commit_date"] = date
elif since or date:
    sys.exit("alab-build.json: CODEX_ALAB_UPSTREAM_SINCE or _DATE names no upstream commit")
dev = re.compile(number + r"\." + number + r"\.0-dev\.(" + number + r")\+g([0-9a-f]{9})")
compound = re.compile(number + r"\." + number + r"\.0-dev\.(" + number + r")\+codex\.(" + number
                      + r"\." + number + r"\." + number + r")\.g([0-9a-f]{9})")
client = os.environ.get("CODEX_ALAB_CLIENT_VERSION", "")
baseline = os.environ.get("CODEX_ALAB_BASELINE", "")
doc = {"source": d["source"], "version": d["version"]}
named = compound.fullmatch(d["version"])
if client:
    ours, theirs = (named or dev.fullmatch(d["version"])), dev.fullmatch(client)
    if not theirs:
        sys.exit(f"alab-build.json: CODEX_ALAB_CLIENT_VERSION {client!r} is not MAJOR.MINOR.0-dev.<count>+g<sha9>")
    if not ours or (ours[1], ours[ours.lastindex]) != theirs.groups():
        sys.exit(f"alab-build.json: the client version {client!r} does not name the count and commit of the version {d['version']!r}")
    doc["client_version"] = client
if baseline or named:
    if not named:
        sys.exit(f"alab-build.json: CODEX_ALAB_BASELINE {baseline!r} is set, but the version {d['version']!r} names no baseline (+codex.<X.Y.Z>)")
    if baseline != named[2]:
        sys.exit(f"alab-build.json: the version {d['version']!r} names the upstream baseline {named[2]!r}, but CODEX_ALAB_BASELINE is {baseline!r}")
    if not client:
        sys.exit("alab-build.json: a compound version needs its client version (CODEX_ALAB_CLIENT_VERSION)")
    if release and release != baseline:
        sys.exit(f"alab-build.json: the baseline {baseline!r} is not the upstream release {release!r} the build compiled in")
    doc["baseline"] = baseline
if upstream is not None:
    doc["upstream"] = upstream
doc.update({"target": d["target"], "compiler": compiler, "ripgrep": d["ripgrep"],
            "ripgrep_features": d["ripgrep_features"], "rusty_v8": d["rusty_v8"],
            "packager": d["packager"], "built_at": d["built_at"]})
with open(out, "w") as f:
    json.dump(doc, f, indent=2)
    print(file=f)' "$@"
}

# alab_upstream_carried <repo> <commit> <upstream commit> <binary>: the smoke's hold on the
# upstream base the session header names. When <commit>'s ALab code reads
# CODEX_ALAB_UPSTREAM_COMMIT (alab-core's version.rs), <binary> must carry that 40-hex
# commit, read as bytes and never run. A commit whose ALab code does not read it (one from
# before version.rs, packaged again) compiles none in, and its binary is held to none.
# Prints what it found; returns 1, saying why, when the binary lacks the commit or the
# source cannot be read.
alab_upstream_carried() {
  local repo="$1" commit="$2" upstream="$3" binary="$4" rc=0
  git -C "${repo}" grep -q -F CODEX_ALAB_UPSTREAM_COMMIT "${commit}" -- 'codex-rs/alab-*' || rc=$?
  case "${rc}" in
    0) ;;
    1)
      echo "smoke: ${commit:0:9}'s ALab code reads no CODEX_ALAB_UPSTREAM_COMMIT: bin/codex \
compiles in no upstream base, so none is looked for"
      return 0
      ;;
    *)
      alab_say "cannot read ${commit:0:9}'s ALab code for CODEX_ALAB_UPSTREAM_COMMIT (git grep \
exit ${rc})"
      return 1
      ;;
  esac
  if ! LC_ALL=C grep -a -q -F "${upstream}" "${binary}"; then
    alab_say "${binary} does not carry the upstream commit ${upstream} that \
${commit:0:9}'s ALab code compiles in (CODEX_ALAB_UPSTREAM_COMMIT)"
    return 1
  fi
  echo "smoke: bin/codex carries upstream ${upstream:0:12}"
}

# These packaged arm64 executables must each have a complete otool listing with at least
# one dependency, all supplied by macOS. Inspect before parsing: a failed inspector may
# still print plausible output, and an empty listing proves nothing about portability.
alab_system_libraries() {
  local binary="$1" listing rc=0
  listing="$(LC_ALL=C otool -L "${binary}")" || rc=$?
  if [[ "${rc}" != 0 ]]; then
    alab_say "smoke: otool -L ${binary} failed (exit ${rc})"
    return 1
  fi
  if ! printf '%s\n' "${listing}" | ALAB_PACKAGE_BINARY="${binary}" LC_ALL=C awk '
    BEGIN {
      version = "[0-9]+[.][0-9]+[.][0-9]+"
      trailer = " \\(compatibility version " version ", current version " version "(, (weak|reexport|upward|lazy))?\\)$"
    }
    NR == 1 {
      if ($0 != ENVIRON["ALAB_PACKAGE_BINARY"] ":") bad = 1
      next
    }
    {
      path = $0
      if (!sub(trailer, "", path) || path !~ /^[[:space:]]+\/(usr\/lib\/|System\/Library\/).+/ || path ~ /\/[.][.](\/|$)/) bad = 1
    }
    END { exit (bad || NR < 2) }'; then
    alab_say "smoke: ${binary}: library listing is empty, malformed or names non-system libraries: ${listing}"
    return 1
  fi
}

# upstream_py <writable-dir>... -- <python code>: run upstream's packaging Python
# read-only at this commit. Do not create bytecode beside the stable source inputs.
upstream_py() {
  local dirs=()
  while [[ "$1" != -- ]]; do
    dirs+=("$1")
    shift
  done
  shift
  (cd "${wt}" && package_contained "${dirs[@]}" -- /usr/bin/env CODEX_REPO_ROOT="${wt}" \
    "${python}" -I -B -c "import sys; sys.path.insert(0, sys.argv[1]); ${1}" "${scripts}" "${@:2}")
}

# New source captures its package stamp only in the final executable actions. Older
# --commit sources still need their workspace stamp to satisfy the same binary smoke.
# Inspect committed ALab code, never execute source to discover this capability. A commit
# with codex-alab's own line (alab/client-version) must report it: its version.rs reads
# CODEX_ALAB_PRODUCT_VERSION, else the package is refused here, before any build (its codex
# --version would name the client version, not the version its records say).
package_source_version() {
  local manifest="${wt}/codex-rs/Cargo.toml" rc=0 moved own="${own_line:-0}"
  local client="${client_version:-${version}}"
  package_leaf_versions=0
  [[ -f "${manifest}" ]] || alab_die "no codex-rs/Cargo.toml at ${sha9}"
  if [[ "${own}" -eq 1 ]]; then
    git -C "${repo_root}" grep -q -F CODEX_ALAB_PRODUCT_VERSION "${commit}" -- \
      codex-rs/alab-core/src/version.rs || rc=$?
    case "${rc}" in
      0) ;;
      1) alab_die "${sha9} names codex-alab's own line (alab/client-version) but its \
codex-rs/alab-core/src/version.rs reads no CODEX_ALAB_PRODUCT_VERSION: its codex could not \
report the version ${version} its package records say (alab/README.md, Decision 8)" ;;
      *) alab_die "cannot read ${sha9}'s product version capability (git grep exit ${rc})" ;;
    esac
  fi
  git -C "${repo_root}" grep -q -F CODEX_ALAB_BUILD_VERSION "${commit}" -- \
    codex-rs/alab-core/src/version.rs || rc=$?
  case "${rc}" in
    0)
      package_leaf_versions=1
      if [[ "${own}" -eq 1 ]]; then
        alab_say "package version ${version} and client version ${client} supplied to the final executables; source manifest and lock unchanged"
      else
        alab_say "package version ${version} supplied to the final executables; source manifest and lock unchanged"
      fi
      return 0
      ;;
    1) ;;
    *) alab_die "cannot read ${sha9}'s package version capability (git grep exit ${rc})" ;;
  esac
  awk -v v="${client}" '
    /^\[workspace\.package\][[:space:]]*$/ { in_pkg = 1; print; next }
    in_pkg && /^\[/ { in_pkg = 0 }
    in_pkg && !done && /^version[[:space:]]*=/ { print "version = \"" v "\""; done = 1; next }
    { print }
    END { exit !done }' "${manifest}" >"${manifest}.alab" \
    || alab_die "no version in [workspace.package] of codex-rs/Cargo.toml at ${sha9}"
  mv "${manifest}.alab" "${manifest}"
  (cd "${wt}/codex-rs" && package_contained "${wt}/codex-rs/Cargo.lock" "${CARGO_TARGET_DIR}" -- \
    "${cargo}" metadata --format-version 1 >/dev/null) \
    || alab_die "${cargo} metadata could not resolve codex-rs at ${sha9}"
  moved="$(git -C "${wt}" -c core.fsmonitor=false -c core.untrackedCache=false diff --name-only | grep -v -x -e codex-rs/Cargo.toml -e codex-rs/Cargo.lock || true)"
  [[ -z "${moved}" ]] || alab_die "stamping the version changed more than the manifest and lock: ${moved}"
  moved="$(git -C "${wt}" -c core.fsmonitor=false -c core.untrackedCache=false diff -U0 -- codex-rs/Cargo.lock | grep '^[-+]' | grep -v -e '^+++ ' -e '^--- ' \
    | grep -v -e '^[-+]version = ' || true)"
  [[ -z "${moved}" ]] || alab_die "refreshing the lock moved more than the workspace versions:
${moved}"
  if [[ -n "${wt_binding:-}" ]]; then
    if moved="$("${python}" -I -B "${repo_root}/alab/package_source.py" stamp-fresh \
      --cache "${cache}" --commit "${commit}" --binding "${wt_binding}" --version "${client}")"; then
      wt_binding="${moved}"
    else
      wt_binding=""
      alab_die "legacy source validation failed; kept ${wt}: its exact tool-owned stamp could not be established"
    fi
  fi
  alab_say "legacy source: stamped ${client} in ${wt} (not committed)"
}

# Select inputs from the target commit's packager, including historical --commit
# builds. Supplying every supported prebuilt input prevents upstream Python from
# starting its own Cargo build with another compiler or target directory.
package_binary_inputs() {
  local rc=0 binary
  release="${CARGO_TARGET_DIR}/release"
  package_first_party=(codex codex-code-mode-host)
  package_cargo_args=(-p codex-cli --bin codex -p codex-code-mode-host --bin codex-code-mode-host)
  package_prebuilt_args=(--entrypoint-bin "${release}/codex"
    --code-mode-host-bin "${release}/codex-code-mode-host")
  package_exec_server=0
  git -C "${repo_root}" grep -q -F -e '--exec-server-bin' "${commit}" -- \
    scripts/codex_package/cli.py || rc=$?
  case "${rc}" in
    0)
      git -C "${repo_root}" cat-file -e "${commit}:codex-rs/exec-server-cli/Cargo.toml" \
        || alab_die "${sha9}'s packager requires an executor but its source crate is absent"
      package_exec_server=1
      package_first_party+=(codex-exec-server)
      package_cargo_args+=(-p codex-exec-server-cli --bin codex-exec-server)
      package_prebuilt_args+=(--exec-server-bin "${release}/codex-exec-server")
      ;;
    1) ;;
    *) alab_die "cannot read ${sha9}'s executor package capability (git grep exit ${rc})" ;;
  esac
  package_input_reads=()
  for binary in "${package_first_party[@]}"; do
    package_input_reads+=(--read "${release}/${binary}")
  done
}

# The driver may be a canonical checkout outside the target worktree's readable
# roots. Admit its owned smoke explicitly, as for the V8 resolver below.
package_executor_smoke() (
  cd "${wt}" || return 1
  package_contained --read "${repo_root}/alab" --read "${staged}" "${codex_home}" -- \
    "${python}" -I "${repo_root}/alab/executor_smoke.py" --package "${staged}" \
    --home "${codex_home}" --client-version "${client_version}"
)

# Sourcing this file defines the body only. The executable entrypoint below runs it
# under the cache's kernel lock, without an environment marker that could skip it.
package_cleanup_allowed() {
  "${python}" -I "${repo_root}/alab/retention.py" --repo "${repo_root}"
}

package_worktree_path() {
  # A killed run may leave files or a registration whose files are gone. Never
  # take over either: this attempt owns a fresh path, under the cache lock.
  mkdir -p "$(dirname "$1")" || return
  mktemp -d "$1.XXXXXX"
}

# The existing cache guardian owns this compiler-keyed target through child cleanup.
# Keep it outside the disposable source worktree and never put it in scratch_paths.
package_rust_target() {
  local account_home release_target admitted_target native_status=69
  account_home="$(alab_account_home)" || return 1
  release_target="$("${python}" -I "${repo_root}/alab/target_cache.py" --cache "${cache}" \
    --compiler "${compiler_id}" --target "${target_triple}" --path-only \
    --home "${HOME:?}" --home "${account_home}")" || return 1
  if [[ -n "${cargo:-}" ]]; then
    if admitted_target="$("${python}" -I -B "${repo_root}/alab/package_cache.py" admit --cache "${cache}" \
      --target-dir "${release_target}" --manifest-path "${wt}/codex-rs/Cargo.toml" \
      --compiler-driver "${cargo}" --print-path)"; then
      native_status=0
    else
      native_status=$?
    fi
  fi
  case "${native_status}" in
    0)
      # Native policy admission created/validated this clone-scoped generation.
      printf '%s\n' "${admitted_target}"
      return 0
      ;;
    69)
      # Older hosts and legacy targets retain pre-creation registration and reuse.
      alab_build_target_ready "${wt}/codex-rs" "${release_target}" >/dev/null || return
      ;;
    *) return "${native_status}" ;;
  esac
  "${python}" -I "${repo_root}/alab/target_cache.py" --cache "${cache}" \
    --compiler "${compiler_id}" --target "${target_triple}" \
    --home "${HOME:?}" --home "${account_home}"
}

# Only current version-capable sources can stay clean through packaging. Historical
# sources retain their fresh worktree and legacy manifest/lock stamp unchanged.
package_prepare_source() {
  local state rc=0
  wt_managed=0
  wt_binding=""
  git -C "${repo_root}" grep -q -F CODEX_ALAB_BUILD_VERSION "${commit}" -- \
    codex-rs/alab-core/src/version.rs || rc=$?
  [[ "${rc}" -eq 0 || "${rc}" -eq 1 ]] || alab_die "cannot inspect package source capability"
  if [[ "${rc}" -eq 0 && "${ALAB_PKG_KEEP_WORKTREE:-0}" != 1 ]] && package_cleanup_allowed; then
    if state="$("${python}" -I -B "${repo_root}/alab/package_source.py" acquire \
      --cache "${cache}" --repo "${repo_root}" --commit "${commit}" \
      --compiler "${compiler_id}" --target "${target_triple}")"; then
      wt="$(printf '%s' "${state}" | "${python}" -I -c 'import json,sys; print(json.load(sys.stdin)["path"])')" \
        || alab_die "cannot read admitted package source"
      wt_binding="$(printf '%s' "${state}" | "${python}" -I -c 'import json,sys; print(json.dumps(json.load(sys.stdin)["binding"]))')" \
        || alab_die "cannot read checked managed source binding"
      wt_managed=1
      wt_made=1
      alab_say "managed package source ${wt}"
      return 0
    fi
    alab_say "stable package source unavailable; preserving it and using a fresh worktree"
  fi
  wt="$(package_worktree_path "${HOME}/Developer/codex-alab-pkg-${sha9}")" \
    || alab_die "cannot reserve this run's package worktree (above)"
  wt_binding="$("${python}" -I -B "${repo_root}/alab/package_source.py" create-fresh \
    --cache "${cache}" --repo "${repo_root}" --source "${wt}" --commit "${commit}")" \
    || alab_die "fresh source validation failed; kept ${wt}: its exact source binding could not be established"
  wt="$(printf '%s' "${wt_binding}" | "${python}" -I -c 'import json,sys; print(json.load(sys.stdin)["source"])')" \
    || alab_die "cannot read this run's checked package source"
  wt_made=1
}

package_validate_source() {
  [[ -n "${wt_binding:-}" ]] || alab_die "no checked source binding before compilation"
  "${python}" -I -B "${repo_root}/alab/package_source.py" validate-source \
    --cache "${cache}" --commit "${commit}" --binding "${wt_binding}" \
    || alab_die "package source changed before compilation; kept ${wt}"
}

package_cleanup() {
  local p
  if ! package_cleanup_allowed; then
    alab_say "the clone's cleanup policy keeps this run's scratch paths and worktree ${wt}"
    return 0
  fi
  for p in ${scratch_paths[@]+"${scratch_paths[@]}"}; do
    if [[ "${p}" == "${copy_failed_source:-}" || "${p}" == "${copy_failed_staging:-}" ]]; then
      alab_say "kept unfinished package copy ${p}"
      continue
    fi
    if [[ "${p}" == "${publication_staged:-}" ]]; then
      if [[ -e "${p}" || -L "${p}" ]]; then
        alab_say "kept the smoke-validated staging package ${p} after unfinished publication"
      fi
      continue
    fi
    rm -rf "${p:?}"
  done
  if [[ -z "${wt_made:-}" ]]; then
    # An interrupted/failed registration has no exact source binding. Preserve
    # even an empty reservation rather than adopting a possible replacement.
    [[ -z "${wt:-}" ]] || alab_say "kept unfinished package source reservation ${wt}"
    return 0
  fi
  if [[ "${wt_managed:-0}" == 1 ]]; then
    # No stamp is restored and no source is removed. Only the outer cache guardian,
    # after killing/reaping its child group, can publish this exact clean source Ready.
    "${python}" -I -B "${repo_root}/alab/package_source.py" release \
      --cache "${cache}" --source "${wt}" --commit "${commit}" \
      || alab_say "kept managed package source ${wt}; its return was refused"
    return 0
  fi
  if [[ "${ALAB_PKG_KEEP_WORKTREE:-0}" == 1 ]]; then
    alab_say "kept the scratch worktree ${wt} (ALAB_PKG_KEEP_WORKTREE=1); inspect it before removing it"
  else
    # The guardian performs ordinary clean removal only after group retirement.
    # It restores only an exact unchanged legacy stamp, never unknown source work.
    if [[ -n "${wt_binding:-}" ]]; then
      "${python}" -I -B "${repo_root}/alab/package_source.py" release-fresh \
        --cache "${cache}" --commit "${commit}" --binding "${wt_binding}" \
        || alab_say "kept fresh package source ${wt}; its return was refused"
    else
      alab_say "kept fresh package source ${wt}; no exact cleanup binding"
    fi
  fi
}

# Copy a contained step's package into a fresh private directory. Retention also keeps
# the original staging tree, which may contain useful diagnostics from that step.
package_copy_out() {
  # Arm retention before starting the copier, including an interrupted invocation.
  copy_failed_source="$1"
  copy_failed_staging="$2"
  if ! "${python}" -I "${repo_root}/alab/copy_package.py" --source "$1" --destination "$2"; then
    alab_die "could not copy the staged package out of $1; source and staging kept"
  fi
  copy_failed_source=""
  copy_failed_staging=""
  if package_cleanup_allowed; then
    rm -rf "${1:?}"
  fi
}

# The resolver keeps upstream in a separate contained interpreter, then checks
# the tree's manifest pin and both files independently in ALab's process.
package_v8_env() {
  local resolved
  resolved="$(cd "$1" && package_contained --read "${repo_root}/alab" "$3" -- \
    "${python}" -I "${repo_root}/alab/v8.py" --repo "$1" --target "$2" --cache "$3")" || return
  "${python}" -I -c 'import json, sys
env = json.load(sys.stdin)
for key in ("version", "RUSTY_V8_ARCHIVE", "RUSTY_V8_SRC_BINDING_PATH"):
    print(env[key])' <<<"${resolved}"
}

# Every version this script mints, and every ledger check, reads the commit's history: its
# count orders builds, its merged branch points name the upstream base, and the version ledger
# is checked along it. Git must hold that history whole. A shallow clone cut below a commit
# with parents counts too few commits (a depth-1 clone of main reads as a one-commit published
# snapshot) and hides every ledger edit before its boundary; a grafts file rewires it; a
# replacement object stands in for a commit (ignored: GIT_NO_REPLACE_OBJECTS, exported by the
# caller). A shallow clone of a published snapshot cuts nothing (its one commit has no
# parent) and is read as it is.
package_whole_history() {
  local shallow cut header grafts
  shallow="$(git rev-parse --is-shallow-repository)" \
    || alab_die "cannot tell whether this clone is shallow"
  if [[ "${shallow}" == true ]]; then
    shallow="$(git rev-parse --git-path shallow)" || alab_die "cannot locate this clone's shallow file"
    while read -r cut; do
      [[ -n "${cut}" ]] || continue
      header="$(git cat-file commit "${cut}")" || alab_die "cannot read the shallow boundary ${cut}"
      header="${header%%$'\n\n'*}"
      [[ "${header}" != *$'\nparent '* ]] || alab_die "this clone is shallow: its history is cut \
at ${cut:0:9}, whose parents are not here, so its versions and the version ledger's history \
cannot be read (a commit count from a cut history is too low, and a ledger edit before the \
cut reads as the original). git fetch --unshallow origin, then rerun"
    done <"${shallow}"
  fi
  grafts="$(git rev-parse --git-path info/grafts)" || alab_die "cannot locate this clone's grafts file"
  [[ ! -e "${grafts}" && ! -L "${grafts}" ]] || alab_die "this clone has a grafts file \
(${grafts}): it rewires the history versions are read from. Remove it (git replace \
--convert-graft-file converts it), then rerun"
}

# The version ledger is checked along origin/main as origin has it now, never along a stale or
# missing remote-tracking ref (which would check the commit's own history alone): fetch it
# fast-forward only, so a rewritten origin main is refused here rather than taken.
package_origin_main() {
  git remote get-url origin >/dev/null 2>&1 || alab_die "no 'origin' remote here: the version \
ledger is checked along origin/main as origin has it now (alab/README.md, Decision 8)"
  alab_say "fetching origin's main (refs/heads/main:refs/remotes/origin/main, fast-forward only)"
  "${ALAB_PYTHON:-/usr/bin/python3}" -I -B "${repo_root}/alab/command_guard.py" \
    --inherited-group-timeout 120 -- "${ALAB_PYTHON:-/usr/bin/python3}" -I -B \
    "${repo_root}/alab/github.py" --repo "${repo_root}" fetch origin \
    refs/heads/main:refs/remotes/origin/main || alab_die "fetching origin's main failed \
(above): the version ledger is checked along it. If git refused a non-fast-forward, origin's \
main was rewritten (or this clone's origin/main is ahead of it): a release never follows a \
rewritten main, and a person decides"
  git rev-parse -q --verify refs/remotes/origin/main >/dev/null \
    || alab_die "no origin/main after fetching origin"
}

package_main() {
publication_staged=""
copy_failed_source=""
copy_failed_staging=""
rev=HEAD
out="${HOME}/.local/share/aterm/dev-builds/artifacts"
keep=3
version_only=0
client_only=0
release_check=0
release_version=0
ledger_row=0
upstream_only=0
any_commit=0
rebuild=0
promote=1
while [[ $# -gt 0 ]]; do
  case "$1" in
    -h | --help)
      sed -n '5,/^set -euo pipefail$/p' "${repo_root}/alab/package.sh" | sed -e '$d' -e 's/^# \{0,1\}//'
      exit 0
      ;;
    --commit)
      [[ $# -ge 2 ]] || alab_die "--commit needs a revision"
      rev="$2"
      shift
      ;;
    --out)
      [[ $# -ge 2 ]] || alab_die "--out needs a directory"
      out="$2"
      shift
      ;;
    --keep)
      [[ $# -ge 2 && "$2" =~ ^[1-9][0-9]*$ ]] || alab_die "--keep needs a positive number"
      keep="$2"
      shift
      ;;
    --version-only) version_only=1 ;;
    --client-version-only) client_only=1 ;;
    --release-check) release_check=1 ;;
    --release-version) release_check=1 release_version=1 ;;
    --ledger-row) ledger_row=1 ;;
    --upstream-base) upstream_only=1 ;;
    --any-commit) any_commit=1 ;;
    --rebuild) rebuild=1 ;;
    --no-promote) promote=0 ;;
    *) alab_die "unknown argument '$1' (see --help)" ;;
  esac
  shift
done

# The outer native controller selected the exact source's compiler before
# entering the compiler-keyed cache. Read-only version modes need no compiler.
pkg_prefix="$(dirname "${ATPKG_BIN:-${HOME}/Library/Application Support/aterm/pkg/bin}")"
cargo="${ALAB_CARGO:-}"
cargo_said="the native source-selected Trust"
# Contained steps build with the clone's private cargo home (lib.sh, alab_cargo_home), named
# here because the rg build runs outside any checkout.
ALAB_CARGO_HOME="${ALAB_CARGO_HOME:-$(git -C "${repo_root}" rev-parse --path-format=absolute --git-common-dir)/alab-cargo-home}"
export ALAB_CARGO_HOME
target_triple=aarch64-apple-darwin
cd "${repo_root}"
alab_upstream_ready

# ---- 1. the commit and its version ---------------------------------------------------------
[[ -z "$(git -c core.fsmonitor=false -c core.untrackedCache=false status --porcelain)" ]] || alab_die "this checkout (${repo_root}) is not clean: a \
version is never minted from a dirty tree (~/publication/VERSIONING.md). Commit or remove the changes."
export GIT_NO_REPLACE_OBJECTS=1
package_whole_history
commit="$(git rev-parse -q --verify "${rev}^{commit}")" || alab_die "no commit '${rev}'"
sha9="$(git rev-parse --short=9 "${commit}")"
count="$(git rev-list --count "${commit}")"
# The version rule reads upstream's release tags and main: fetch both first, by the
# refspecs alab/setup.sh configured (fetch only; no '+' on main, so a rewrite is refused),
# so a stale tag ref never mints an old line. Never the tags into refs/tags.
spec_tags="$(git config --get-all remote.upstream.fetch | grep -m1 ':refs/upstream-tags/' || true)"
spec_main="$(git config --get-all remote.upstream.fetch | grep -m1 ':refs/remotes/upstream/main$' || true)"
[[ -n "${spec_tags}" && -n "${spec_main}" ]] || alab_die "the upstream remote has no refspec for \
its release tags (refs/upstream-tags/) or main: run alab/setup.sh"
alab_say "fetching upstream's release tags and main (${spec_tags}, ${spec_main})"
# Keep Git and its helpers in the existing guardian's leased group. A timeout
# fails this transaction immediately, so that guardian stops the remaining group
# before any cache unlock; compilation itself has no added deadline.
"${ALAB_PYTHON:-/usr/bin/python3}" -I -B "${repo_root}/alab/command_guard.py" \
  --inherited-group-timeout 120 -- "${ALAB_PYTHON:-/usr/bin/python3}" -I -B \
  "${repo_root}/alab/github.py" --repo "${repo_root}" --anonymous fetch upstream \
  "${spec_tags}" "${spec_main}" || alab_die "fetching upstream \
failed (above): the version rule needs its current tags and main. If git refused a \
non-fast-forward of upstream/main, upstream rewrote its history: a person decides (alab/README.md)"
git rev-parse -q --verify refs/remotes/upstream/main >/dev/null \
  || alab_die "no upstream/main here: run alab/setup.sh, then git fetch upstream"

# The newest stable upstream tag (rust-vX.Y.Z, no pre-release) whose branch point the commit
# has merged. A release tag is never an ancestor of upstream/main (alab/README.md, "What
# upstream is like"), so its branch point is its merge-base with upstream/main.
base_tag="" base_bp=""
while read -r stable; do
  [[ "${stable}" =~ ^(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})$ ]] || continue
  bp="$(git merge-base "refs/upstream-tags/rust-v${stable}" refs/remotes/upstream/main)" || continue
  if git merge-base --is-ancestor "${bp}" "${commit}"; then
    base_tag="${stable}"
    base_bp="${bp}"
    break
  fi
done < <(git for-each-ref --format='%(refname:strip=2)' 'refs/upstream-tags/rust-v*' \
  | sed -n 's/^rust-v\([0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*\)$/\1/p' \
  | sort -t. -k1,1nr -k2,2nr -k3,3nr)
# Two lines (alab/README.md, Decision 8). The commit's own VERSION (MAJOR.MINOR.0) is
# codex-alab's release line, restarted at 0.1.0 and moved only by `pub bump codex-alab`: the
# version the package says it is. Its alab/client-version (MAJOR.MINOR.0) is the client line,
# upstream's numbering: the floor is the next minor above that stable, since a build that
# holds upstream's X.Y code must never call itself older to upstream's code and OpenAI's
# servers (the server picks models by client_version). A dev package's client line takes the
# higher of the two, so packaging never waits on an edit: upstream cuts a stable every few
# days. A release is refused while alab/client-version is below the floor instead
# (--release-check, the engine's publish/pre-promote), because a published snapshot is one
# commit with no history: it has merged no branch point to read, so its alab/client-version,
# held to the floor when it was cut, is its client line. A commit from before
# alab/client-version has one line, VERSION, which was upstream's numbering then: its
# version is its client version, held to the floor as it was packaged; a commit from before
# VERSION takes the floor.
committed="$(git show "${commit}:VERSION" 2>/dev/null | tr -d '[:space:]')" || committed=""
if [[ -n "${committed}" ]]; then
  [[ "${committed}" =~ ^(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})\.0$ ]] || alab_die "${sha9}'s VERSION is not of the \
form MAJOR.MINOR.0 (found '${committed}'): it names codex-alab's release line (alab/README.md, \
Decision 8)"
fi
client_committed="$(git show "${commit}:alab/client-version" 2>/dev/null | tr -d '[:space:]')" \
  || client_committed=""
own_line=0
client_file=VERSION
client_held="${committed}"
if [[ -n "${client_committed}" ]]; then
  own_line=1
  client_file=alab/client-version
  client_held="${client_committed}"
  [[ -n "${committed}" ]] || alab_die "${sha9} has an alab/client-version but no VERSION: \
nothing names codex-alab's own line (alab/README.md, Decision 8)"
  [[ "${client_committed}" =~ ^(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})\.0$ ]] || alab_die "${sha9}'s alab/client-version \
is not of the form MAJOR.MINOR.0 (found '${client_committed}'): it names the client line, \
upstream's numbering (alab/README.md, Decision 8)"
fi
if [[ -n "${client_held}" ]]; then
  [[ "${client_held}" =~ ^(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})\.0$ ]]
  v_major="${BASH_REMATCH[1]}" v_minor="${BASH_REMATCH[2]}"
fi
# The compound line (alab/README.md, Decision 8): a commit that carries the version ledger,
# alab/versions.toml, names the upstream Codex baseline it is built on in its version's build
# metadata. A commit from before the ledger keeps the version it was packaged with.
compound=0
if git cat-file -e "${commit}:alab/versions.toml" 2>/dev/null; then
  compound=1
  [[ "${own_line}" -eq 1 ]] || alab_die "${sha9} carries alab/versions.toml but no \
alab/client-version: the ledger maps codex-alab's own line, which a commit names with both \
(alab/README.md, Decision 8)"
fi
versions_py=("${ALAB_PYTHON:-/usr/bin/python3}" -I "${repo_root}/alab/versions.py")
# The public release repository: the record of every release (each a snapshot naming the dev
# commit it was cut from), which the ledger check reads (alab/versions.py, released).
# publish/pre-promote names the engine's release repository here.
released_repo="${ALAB_RELEASED_REPO:-https://github.com/alabsystems/codex-alab.git}"
behind=0
if [[ -n "${base_tag}" ]]; then
  major="${base_tag%%.*}"
  minor="${base_tag#*.}"
  minor="${minor%%.*}"
  floor="${major}.$((minor + 1)).0"
  floor_why="the floor ${floor}, the next minor above rust-v${base_tag}, the newest upstream
alab:     stable whose branch point ${base_bp:0:9} (merge-base with upstream/main) ${sha9} has merged"
  if [[ -z "${client_held}" ]]; then
    line="${floor}"
    how="${floor_why} (${sha9} predates VERSION)"
  elif (( v_major < major || (v_major == major && v_minor <= minor) )); then
    line="${floor}"
    behind=1
    if [[ "${own_line}" -eq 1 ]]; then
      how="${floor_why}; alab/client-version ${client_committed} is behind it, so it is due
alab:     an edit to ${floor} before the next release (--release-check refuses until then)"
    else
      how="${floor_why}; VERSION ${committed} is behind it, so \`pub bump codex-alab\` is due
alab:     before the next release (--release-check refuses until then)"
    fi
  else
    line="${client_held}"
    how="${client_file} at ${sha9}, at least ${floor_why}"
  fi
elif [[ "${count}" -eq 1 ]]; then
  [[ -n "${client_held}" ]] || alab_die "${sha9} is a published snapshot (one commit) with no \
VERSION: nothing names its line"
  line="${client_held}"
  how="${client_file} at ${sha9}, a published snapshot (one commit, no history): no branch point to hold it to"
else
  alab_die "${sha9} has merged the branch point of no upstream stable tag \
(refs/upstream-tags/rust-v*): run alab/setup.sh to fetch them"
fi
# The baseline: the newest upstream stable whose branch point the commit has merged
# (base_tag above, its real patch number kept), and the commit its tag names. A published
# snapshot has no history to read one from: its ledger's row for its VERSION says it (the
# release that cut the snapshot carried that row), and with no row it has none.
baseline="" baseline_commit=""
if [[ "${compound}" -eq 1 ]]; then
  if [[ -n "${base_tag}" ]]; then
    baseline="${base_tag}"
    baseline_commit="$(git rev-parse -q --verify "refs/upstream-tags/rust-v${base_tag}^{commit}")" \
      || alab_die "cannot read the commit rust-v${base_tag} names (refs/upstream-tags)"
  elif [[ "${count}" -eq 1 ]]; then
    baseline="$("${versions_py[@]}" baseline --repo "${repo_root}" --commit "${commit}" \
      --version "${committed}")" || alab_die "${sha9} is a published snapshot (one commit, no \
history) whose alab/versions.toml records no baseline for its VERSION ${committed}: nothing names \
the upstream Codex it is built on (alab/README.md, Decision 8)"
  fi
  [[ -n "${baseline}" ]] || alab_die "${sha9} names no upstream baseline (Decision 8)"
fi
if [[ "${ledger_row}" -eq 1 ]]; then
  [[ "${compound}" -eq 1 ]] || alab_die "${sha9} carries no alab/versions.toml: there is no \
ledger to append to (alab/README.md, Decision 8)"
  [[ -n "${baseline_commit}" ]] || alab_die "${sha9} has no upstream history to derive a \
baseline from: a row is recorded from a commit on origin/main, never a snapshot"
  package_origin_main
  "${versions_py[@]}" row --repo "${repo_root}" --commit "${commit}" --version "${committed}" \
    --baseline "${baseline}" --baseline-commit "${baseline_commit}" \
    --main refs/remotes/origin/main --released "${released_repo}" --date "$(date -u +%Y-%m-%d)" \
    || alab_die "no row for ${committed} can be appended at ${sha9} (above)"
  exit 0
fi
if [[ "${release_check}" -eq 1 ]]; then
  [[ -n "${committed}" ]] || alab_die "${sha9} has no VERSION: a release needs one (Decision 8)"
  if [[ "${behind}" -eq 1 && "${own_line}" -eq 1 ]]; then
    alab_die "alab/client-version ${client_committed} at ${sha9} is below the floor ${floor}, the \
next minor above rust-v${base_tag} (whose branch point ${base_bp:0:9} it has merged): a release \
built from its snapshot would tell OpenAI's servers it is older than the upstream code it holds. \
Set alab/client-version to ${floor} in a commit on origin/main (it is upstream's numbering, never \
codex-alab's own VERSION, which \`pub bump\` moves), push it, then release"
  fi
  if [[ "${behind}" -eq 1 ]]; then
    alab_die "VERSION ${committed} at ${sha9} is below the floor ${floor}, the next minor above \
rust-v${base_tag} (whose branch point ${base_bp:0:9} it has merged): a release would call itself \
older than the upstream code it holds. Move the line with the publication engine, never by hand: \
PUBLISH_REPO_PATH=<a clean checkout of origin/main> pub bump codex-alab --commit (one minor per \
run) until it is at least ${floor}, push it, then release"
  fi
  if [[ "${own_line}" -eq 1 && "${compound}" -eq 0 ]]; then
    alab_die "${sha9} names codex-alab's own line but carries no alab/versions.toml: a release of \
its line is recorded in the version ledger, beside the upstream baseline it is built on \
(alab/README.md, Decision 8). Release from a commit on origin/main that carries it"
  fi
  release="${committed}"
  if [[ "${compound}" -eq 1 ]]; then
    [[ -n "${baseline_commit}" ]] || alab_die "${sha9} has no upstream history to derive its \
baseline from: a release is checked from its commit on origin/main, never from a snapshot"
    package_origin_main
    release="$("${versions_py[@]}" check --repo "${repo_root}" --commit "${commit}" \
      --version "${committed}" --baseline "${baseline}" --baseline-commit "${baseline_commit}" \
      --main refs/remotes/origin/main --released "${released_repo}")" || alab_die "the version ledger refuses releasing \
${committed} from ${sha9} (above; alab/README.md, Decision 8)"
    alab_say "release ${release}: alab/versions.toml maps ${committed} to the upstream baseline \
rust-v${baseline} (${baseline_commit:0:12})"
  fi
  if [[ "${release_version}" -eq 1 ]]; then
    printf '%s\n' "${release}"
  else
    printf '%s\n' "${committed}"
  fi
  exit 0
fi
client_version="${line}-dev.${count}+g${sha9}"
if [[ "${compound}" -eq 1 ]]; then
  version="${committed}-dev.${count}+codex.${baseline}.g${sha9}"
  version_how="VERSION ${committed} at ${sha9}, codex-alab's own release line"
elif [[ "${own_line}" -eq 1 ]]; then
  version="${committed}-dev.${count}+g${sha9}"
  version_how="VERSION ${committed} at ${sha9}, codex-alab's own release line"
else
  version="${client_version}"
  version_how="${sha9} predates alab/client-version: one line, its client line"
fi
# The upstream base: provenance beside the version, never part of it (Decision 8). Read
# once, for the binary (the release build's CODEX_ALAB_UPSTREAM_VERSION and _COMMIT, which
# alab-core's version.rs compiles in for the session header), alab-build.json's "upstream"
# and the release line (--upstream-base). <n> counts the upstream commits past the stable's
# branch point, which the merged commit has as an ancestor (upstream's history is linear).
upstream_at="$(git merge-base "${commit}" refs/remotes/upstream/main 2>/dev/null)" || upstream_at=""
upstream_date="" upstream_base=""
if [[ -n "${upstream_at}" ]]; then
  upstream_date="$(TZ=UTC git show -s --date=format-local:%Y-%m-%dT%H:%M:%SZ --format=%cd \
    "${upstream_at}")" || alab_die "cannot read the date of upstream ${upstream_at:0:12}"
  upstream_said="openai/codex@${upstream_at:0:12} (${upstream_date}), the upstream commit \
${sha9} merged last"
  if [[ -n "${base_tag}" ]]; then
    since="$(git rev-list --count "${base_bp}..${upstream_at}")" \
      || alab_die "cannot count upstream's commits from ${base_bp:0:9} to ${upstream_at:0:9}"
    commits=commits
    [[ "${since}" != 1 ]] || commits=commit
    upstream_base="Based on upstream openai/codex rust-v${base_tag} + ${since} ${commits} \
(${upstream_at:0:12}, ${upstream_date%%T*})"
  fi
else
  upstream_said="none in the history of ${sha9}"
fi
baseline_how=""
identity_said="+g${sha9}"
if [[ "${compound}" -eq 1 ]]; then
  identity_said=".g${sha9}"
  if [[ -n "${base_tag}" ]]; then
    baseline_how="alab:   +codex.${baseline}: the upstream Codex baseline, rust-v${baseline}, the newest
alab:     stable whose branch point ${base_bp:0:9} ${sha9} has merged (build metadata: it never orders)"
  else
    baseline_how="alab:   +codex.${baseline}: the upstream Codex baseline alab/versions.toml records
alab:     for ${committed} (a published snapshot has no history to read it from)"
  fi
  recorded_on="$("${versions_py[@]}" baseline --repo "${repo_root}" --commit "${commit}" \
    --version "${committed}" 2>/dev/null)" || recorded_on=""
  if [[ -n "${recorded_on}" && "${recorded_on}" != "${baseline}" ]]; then
    baseline_how="${baseline_how}
alab:     alab/versions.toml records ${committed} on ${recorded_on}: \`pub bump codex-alab\` is due
alab:     before the next release (--release-check refuses until then)"
  fi
fi
cat >&2 <<EOF
alab: version ${version}
alab:   line ${version%%-dev.*}: ${version_how}
alab:   -dev.${count}: git rev-list --count ${sha9} (ordering only)
${baseline_how:+${baseline_how}
}alab:   ${identity_said}: the commit (identity)
alab: client version ${client_version} (what upstream's code and OpenAI's servers see)
alab:   line ${line}: ${how}
alab:   upstream: ${upstream_said}${upstream_base:+
alab:     ${upstream_base}}
EOF
if [[ "${version_only}" -eq 1 ]]; then
  printf '%s\n' "${version}"
  exit 0
fi
if [[ "${client_only}" -eq 1 ]]; then
  printf '%s\n' "${client_version}"
  exit 0
fi
if [[ "${upstream_only}" -eq 1 ]]; then
  [[ -n "${upstream_base}" ]] || alab_die "${sha9} names no upstream base: ${upstream_said} (a \
published snapshot carries no history to read one from)"
  printf '%s\n' "${upstream_base}"
  exit 0
fi

if [[ "${any_commit}" -eq 0 ]]; then
  git rev-parse -q --verify refs/remotes/origin/main >/dev/null \
    || alab_die "no origin/main here: git fetch origin (or pass --any-commit)"
  git merge-base --is-ancestor "${commit}" refs/remotes/origin/main || alab_die "${sha9} is not \
on origin/main: ALab ships from main (alab/README.md, Decision 4). git fetch origin if it landed \
since, or pass --any-commit to package it anyway"
fi

[[ "$(uname -s)-$(uname -m)" == Darwin-arm64 ]] || alab_die "packaging builds ${target_triple} \
on an arm64 Mac only (the one target Trust ships for)"
alab_project_selected || alab_die "packaging requires the native source-selected compiler"
cargo="${ALAB_CARGO}"
command -v "${cargo}" >/dev/null || alab_die "no selected compiler front ${cargo}"
command -v otool >/dev/null || alab_die "no otool (the Xcode command line tools: xcode-select --install)"
python="${ALAB_PYTHON:-}"
if [[ -z "${python}" ]]; then
  for p in /opt/homebrew/bin/python3.14 /opt/homebrew/bin/python3.13 /opt/homebrew/bin/python3.12 \
    /opt/homebrew/bin/python3.11 \
    "${pkg_prefix}/abrew/bin/python3.14" "${pkg_prefix}/abrew/bin/python3.13" \
    "${pkg_prefix}/abrew/bin/python3.12" "${pkg_prefix}/abrew/bin/python3.11" \
    "${HOME}/.local/share/uv/python"/cpython-3.*/bin/python3.[0-9]* \
    python3; do
    [[ "${p}" != *-config ]] || continue
    if command -v "${p}" >/dev/null && "${p}" -I -c 'import sys; sys.exit(sys.version_info < (3, 11))' 2>/dev/null; then
      python="${p}"
      break
    fi
  done
fi
[[ -n "${python}" ]] && "${python}" -I -c 'import sys; sys.exit(sys.version_info < (3, 11))' 2>/dev/null \
  || alab_die "no Python >= 3.11 (upstream's packager needs tomllib): set ALAB_PYTHON"

mkdir -p "${out}"
out="$(cd "${out}" && pwd -P)"
pkg_root="${out}/codex"
mkdir -p "${pkg_root}"
# "Already packaged" is the source commit, not the version: the version's line moves when
# upstream cuts a stable, and the same commit must not then be packaged twice by accident.
packaged="$(grep -l -F "\"source\": \"${commit}\"" "${pkg_root}"/*/alab-build.json 2>/dev/null || true)"
pkgdir="${pkg_root}/${version}"
if [[ -n "${packaged}" ]]; then
  [[ "${rebuild}" -eq 1 ]] || alab_die "${sha9} is packaged already: $(dirname "${packaged%%$'\n'*}") \
(pass --rebuild to package it again beside it, or remove it, unless aterm links it)"
  alab_say "${sha9} is packaged already ($(printf '%s\n' "${packaged}" | sed 's,/alab-build.json$,,' \
    | tr '\n' ' ')): --rebuild, so a new one is staged beside it"
fi
if [[ -e "${pkgdir}" ]]; then
  [[ "${rebuild}" -eq 1 && -f "${pkgdir}/alab-build.json" ]] || alab_die "${pkgdir} exists and \
is not a package of ${sha9}: remove it"
  pkgdir="${pkg_root}/${version}.rebuilt-$(date -u +%Y%m%dT%H%M%SZ)"
  [[ ! -e "${pkgdir}" ]] || alab_die "${pkgdir} exists"
fi
cache="${ALAB_PACKAGE_CACHE:-${HOME}/.cache/codex-alab}"
[[ "${cache}" == /* ]] || alab_die "ALAB_PACKAGE_CACHE must be an absolute path, not '${cache}'"
v8_cache="${cache}/v8"
mkdir -p "${v8_cache}"
packager="$(git rev-parse HEAD)"

# need_disk: wait, up to ALAB_DISK_WAIT_MINUTES, for ALAB_MIN_FREE_GB free on the volume of <dir>.
need_disk() {
  local dir="$1" need="${ALAB_MIN_FREE_GB:-35}" limit=$((${ALAB_DISK_WAIT_MINUTES:-120} * 60))
  local waited=0 free
  mkdir -p "${dir}" || alab_die "disk: cannot make ${dir}"
  while :; do
    free="$(df -k "${dir}" | awk 'NR == 2 { print int($4 / 1048576) }')" \
      || alab_die "disk: df cannot read the volume of ${dir} (above)"
    [[ "${free}" =~ ^[0-9]+$ ]] || alab_die "disk: df gave no free space for ${dir}"
    if [[ "${free}" -ge "${need}" ]]; then
      alab_say "disk: ${free} GB free where ${dir} is (${need} GB needed)"
      return 0
    fi
    [[ "${waited}" -lt "${limit}" ]] || alab_die "disk: ${free} GB free where ${dir} is, ${need} GB \
needed, after waiting $((waited / 60)) minutes: free space (each release target.noindex/ is 12 to 15 GB), then rerun"
    alab_say "disk: ${free} GB free where ${dir} is, ${need} GB needed: checking again in 5 minutes"
    sleep 300
    waited=$((waited + 300))
  done
}

# ---- 2. the scratch worktree ---------------------------------------------------------------
wt=""
# This run's scratch paths (the partial and staged package, the rg being built, the compiler
# probes, the smoke's CODEX_HOME), removed when it ends unless preservation applies.
scratch_paths=()
trap package_cleanup EXIT
# scratch_dir <var> <template>: set <var> to a new private directory, removed when the run
# ends (called in this shell, never in $(...), so the cleanup sees it).
scratch_dir() {
  local d
  d="$(mktemp -d "$2")" || alab_die "cannot make a scratch directory from $2"
  scratch_paths+=("${d}")
  printf -v "$1" '%s' "${d}"
}
# The compiler comes from the toolchain itself, before any contained step, and nothing a
# contained step can write ever names a program that runs outside the sandbox: a contained
# build can write its own target.noindex/.rustc_info.json (and a process it leaves behind can keep
# writing it), so that record is only compared, as text, with the trusted one.
# trusted_compiler <json-out>: build a one-file probe crate, outside the sandbox (only the
# toolchain's own code runs: no dependency, no build script), in a new directory no
# contained step is ever given; take the -vV and sysroot its .rustc_info.json records, check
# that sysroot's own trustc -vV says the same and binary: trustc, and write the compiler as
# JSON with a digest of its binaries (the trustc executable and librustc_driver). Sets
# compiler_id to its id: a digest of all of it, so a rebuilt compiler at the same path is a
# new id. Called in this shell, never in $(...), so the cleanup sees its probe directory.
trusted_compiler() {
  local p
  scratch_dir p "${cache}/.sysroot-probe.XXXXXX"
  mkdir -p "${p}/src"
  printf '[package]\nname = "alab-probe"\nversion = "0.0.0"\nedition = "2021"\n\n[workspace]\n' \
    >"${p}/Cargo.toml"
  printf 'fn main() {}\n' >"${p}/src/main.rs"
  (cd "${p}" && CARGO_TARGET_DIR="${p}/target.noindex" "${cargo}" --unverified build --quiet \
    --manifest-path "${p}/Cargo.toml") >&2 || alab_die "${cargo} could not build a probe crate in ${p} (above)"
  compiler_id="$("${python}" -I -c '
import glob, hashlib, json, os, subprocess, sys
info, out = sys.argv[1], sys.argv[2]
try:
    outputs = json.load(open(info))["outputs"].values()
except (OSError, ValueError, KeyError) as e:
    sys.exit(f"no compiler record in {info}: {e}")
vv = sysroot = None
for o in outputs:
    text = o.get("stdout", "")
    if text.startswith("rustc ") and "\nbinary: " in text:
        vv = text
        continue
    for line in text.splitlines():
        if line.startswith("/") and os.path.isdir(os.path.join(line, "lib", "rustlib")):
            sysroot = line
if vv is None or sysroot is None:
    sys.exit(f"{info} names no rustc -vV and sysroot")
binary = next((l.split(": ", 1)[1] for l in vv.splitlines() if l.startswith("binary: ")), "")
if binary != "trustc":
    sys.exit(f"the compiler is not Trust: rustc -vV says binary: {binary} ({vv.splitlines()[0]}, "
             f"sysroot {sysroot}); packaging builds with Trust only (aterm help rust)")
exe = next((p for p in (os.path.join(sysroot, "bin", n) for n in ("trustc", "rustc")) if os.path.isfile(p)), None)
if exe is None:
    sys.exit(f"no trustc in {sysroot}/bin")
real = subprocess.run([exe, "-vV"], capture_output=True, text=True).stdout
if real != vv:
    sys.exit(f"{exe} -vV does not say what {info} records")
h = hashlib.sha256()
for f in [exe] + sorted(glob.glob(os.path.join(sysroot, "lib", "librustc_driver-*.dylib"))):
    d = hashlib.sha256()
    with open(f, "rb") as fh:
        for chunk in iter(lambda: fh.read(1 << 20), b""):
            d.update(chunk)
    h.update(os.path.basename(f).encode() + b"\0" + d.digest())
bins = h.hexdigest()
cid = hashlib.sha256("\0".join([vv, sysroot, bins]).encode()).hexdigest()[:16]
json.dump({"rustc": vv.splitlines()[0], "rustc_vV": vv, "sysroot": sysroot,
           "binaries_sha256": bins, "id": cid}, open(out, "w"))
print(cid)' "${p}/target.noindex/.rustc_info.json" "$1")" || alab_die "refusing the compiler ${cargo} builds with (above)"
}
# same_compiler <target-dir> <what>: <target-dir>/.rustc_info.json, which a contained step
# wrote, records the trusted compiler: its rustc -vV is the trusted text byte for byte, and
# every sysroot it names is the trusted sysroot (`targo install` records the -vV only). Read
# as data only: nothing named in it is run, or even looked at on disk.
same_compiler() {
  "${python}" -I -c '
import json, sys
info, trusted, what = sys.argv[1], json.load(open(sys.argv[2])), sys.argv[3]
try:
    outputs = json.load(open(info))["outputs"].values()
except (OSError, ValueError, KeyError, AttributeError) as e:
    sys.exit(f"{what}: no compiler record in {info}: {e}")
vvs, roots = [], set()
for o in outputs:
    text = o.get("stdout", "") if isinstance(o, dict) else ""
    if not isinstance(text, str):
        continue
    if text.startswith("rustc ") and "\nbinary: " in text:
        vvs.append(text)
        continue
    roots.update(l for l in text.splitlines() if l.startswith("/"))
if not vvs:
    sys.exit(f"{what}: {info} records no rustc -vV")
bad = [v.splitlines()[0] for v in vvs if v != trusted["rustc_vV"]]
if bad:
    sys.exit(f"{what}: built with {bad[0]!r}, not the trusted compiler ({trusted['rustc']}, {trusted['sysroot']})")
if roots - {trusted["sysroot"]}:
    sys.exit(f"{what}: records the sysroot {sorted(roots - {trusted['sysroot']})[0]!r}, not {trusted['sysroot']!r}")' \
    "$1/.rustc_info.json" "${cfacts}/trusted.json" "$2"
}
# probe_compiler <cwd>: a probe crate built by targo when run in <cwd> (whose
# .cargo/config.toml it reads, and so may pick another compiler), contained, must record the
# trusted compiler: a stock toolchain stops the run here, not after the release build.
probe_compiler() {
  local p
  scratch_dir p "${cache}/.probe.XXXXXX"
  mkdir -p "${p}/src"
  printf '[package]\nname = "alab-probe"\nversion = "0.0.0"\nedition = "2021"\n\n[workspace]\n' \
    >"${p}/Cargo.toml"
  printf 'fn main() {}\n' >"${p}/src/main.rs"
  (cd "$1" && package_contained "${p}" -- /usr/bin/env CARGO_TARGET_DIR="${p}/target.noindex" \
    "${cargo}" --unverified build --quiet --manifest-path "${p}/Cargo.toml") >&2 \
    || alab_die "${cargo} could not build a probe crate in $1 (above)"
  same_compiler "${p}/target.noindex" "targo in $1" || alab_die "refusing the compiler ${cargo} builds with in $1 (above)"
}
scratch_dir cfacts "${cache}/.compiler.XXXXXX"
trusted_compiler "${cfacts}/trusted.json"
compiler_root="$("${python}" -I -c 'import json, sys; print(json.load(open(sys.argv[1]))["sysroot"])' \
  "${cfacts}/trusted.json")"
# Read the native-selected execution view (which may be a validated compat
# view of the signed managed payload), including its compiler libraries, without
# adding a writable root or relying on PATH's bin directory.
package_contained() {
  # The selected wrapper belongs to the caller, outside the packaged source tree.
  alab_contained --read "${ATPKG_PROJECT_FRONT}" --read "${compiler_root}" --read "${cargo}" "$@"
}
alab_say "compiler ${compiler_id}: $(sed -n 's/.*"rustc": "\([^"]*\)".*/\1/p' "${cfacts}/trusted.json"), \
$(sed -n 's/.*"sysroot": "\([^"]*\)".*/\1/p' "${cfacts}/trusted.json") (${cargo_said}: ${cargo})"
package_record_native_compiler "${python}" "${cfacts}/trusted.json" "${ATPKG_PROJECT_EXECUTION}" \
  || alab_die "cannot retain the native compiler observation"

need_disk "${HOME}/Developer"
package_prepare_source

export CARGO_INCREMENTAL=0
export CARGO_TARGET_DIR
CARGO_TARGET_DIR="$(package_rust_target)" || alab_die "cannot admit the private release target (above)"
package_source_version
package_binary_inputs
package_validate_source

# ---- 3. the compiler, then the target's first-party executables --------------------------
probe_compiler "${wt}/codex-rs"
probe_compiler "${cache}"

scripts="${wt}/scripts"

# Upstream resolves its pinned pair; shared ALab code independently verifies it.
v8_env="$(package_v8_env "${wt}" "${target_triple}" "${v8_cache}")" \
  || alab_die "could not fetch and verify upstream's rusty_v8 archives (above)"
v8_version="$(printf '%s\n' "${v8_env}" | sed -n 1p)"
v8_archive="$(printf '%s\n' "${v8_env}" | sed -n 2p)"
v8_binding="$(printf '%s\n' "${v8_env}" | sed -n 3p)"
v8_trusted="${wt}/third_party/v8/rusty_v8_${v8_version//./_}_release_manifests.sha256"
alab_say "rusty_v8 ${v8_version}: $(basename "${v8_archive}") verified against $(basename "${v8_trusted}")"

need_disk "${CARGO_TARGET_DIR}"
alab_say "building ${package_first_party[*]} (release) at ${sha9}"
(cd "${wt}/codex-rs" && package_contained --read "${v8_cache}" "${CARGO_TARGET_DIR}" -- /usr/bin/env \
  STABLE_GIT_COMMIT="${commit}" CODEX_ALAB_BUILD_VERSION="${client_version}" \
  CODEX_ALAB_PRODUCT_VERSION="${version}" \
  CODEX_ALAB_UPSTREAM_VERSION="${base_tag}" CODEX_ALAB_UPSTREAM_COMMIT="${upstream_at}" \
  RUSTY_V8_ARCHIVE="${v8_archive}" RUSTY_V8_SRC_BINDING_PATH="${v8_binding}" \
  "${cargo}" --unverified build --release --locked "${package_cargo_args[@]}") \
  || alab_die "the release build failed (above)"
same_compiler "${CARGO_TARGET_DIR}" "the release build" || alab_die "refusing the release build (above)"

# ---- 4. rg, built with Trust ---------------------------------------------------------------
rg_version="$(sed -n 's/.*"path": "ripgrep-\([0-9][0-9.]*\)-aarch64-apple-darwin\/rg".*/\1/p' \
  "${scripts}/codex_package/rg")"
[[ "${rg_version}" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || alab_die "no ripgrep version for \
${target_triple} in scripts/codex_package/rg"
rg_features=pcre2
# Keyed by the version and the compiler: either changing builds it again.
rg_root="${cache}/rg-${rg_version}-${compiler_id}"
rg_bin="${rg_root}/bin/rg"
# rg_ok <rg>: it says it is ripgrep <rg_version> with PCRE2. It runs contained and read-only,
# once: rg_said keeps what it said (and its exit status), for the message when it is refused.
# A binary a contained step built never runs outside the sandbox, not even for a message.
rg_ok() {
  local rc=0
  rg_said="$(cd "${wt}" && package_contained --read "$(dirname "$1")" -- "$1" --version 2>&1)" || rc=$?
  [[ "${rc}" -eq 0 ]] || {
    rg_said="exit ${rc}: ${rg_said}"
    return 1
  }
  printf '%s\n' "${rg_said}" | head -1 | grep -q "^ripgrep ${rg_version}\( \|$\)" \
    && printf '%s\n' "${rg_said}" | grep -q '^features:.*+pcre2' \
    && printf '%s\n' "${rg_said}" | grep -q '^PCRE2 .* is available'
}
if [[ -x "${rg_bin}" ]] && rg_ok "${rg_bin}"; then
  alab_say "rg ${rg_version} (+${rg_features}, compiler ${compiler_id}): cached at ${rg_bin}"
else
  need_disk "${cache}"
  alab_say "rg ${rg_version}: building with Trust (+${rg_features}) into ${rg_root}"
  scratch_dir rg_partial "${cache}/.rg-${rg_version}-${compiler_id}.partial.XXXXXX"
  # From the cache dir, and with a cargo home there, so no repo's .cargo/config.toml applies:
  # `cargo install` of a registry crate takes its config from the directories above its cargo
  # home, not the working directory's, and the clone's private one sits in the clone's git
  # dir, under the main checkout's own .cargo/config.toml (outside the sandbox's readable
  # set: "could not load Cargo configuration"). PCRE2 from its vendored source, statically,
  # never a Homebrew libpcre2 that pkg-config finds.
  (cd "${cache}" && ALAB_CARGO_HOME="${cache}/cargo-home" package_contained "${rg_partial}" -- \
    /usr/bin/env CARGO_TARGET_DIR="${rg_partial}/target.noindex" \
    PCRE2_SYS_STATIC=1 "${cargo}" --unverified install --locked --features "${rg_features}" \
    "ripgrep@${rg_version}" --root "${rg_partial}") || alab_die "building ripgrep ${rg_version} failed (above)"
  same_compiler "${rg_partial}/target.noindex" "the rg build" || alab_die "refusing the rg build (above)"
  if package_cleanup_allowed; then
    rm -rf "${rg_partial:?}/target.noindex"
  fi
  rg_ok "${rg_partial}/bin/rg" || alab_die "the rg just built is not ripgrep ${rg_version} with PCRE2: \
$(printf '%s\n' "${rg_said}" | tr '\n' ' ')"
  if package_cleanup_allowed; then
    rm -rf "${rg_root:?}"
    mv "${rg_partial}" "${rg_root}"
  elif [[ -e "${rg_root}" || -L "${rg_root}" ]]; then
    # Keep an existing cache intact; the new private result remains usable this run.
    rg_root="${rg_partial}"
    rg_bin="${rg_root}/bin/rg"
  else
    mv "${rg_partial}" "${rg_root}"
  fi
fi

# ---- 5. the package ------------------------------------------------------------------------
# upstream's packager stages it in a new hidden directory, the only one it may write; the
# binaries and rg it reads are read-only to it. A process it leaves behind can keep writing
# there, so nothing more is written there and nothing there is published: the package is
# copied through held directory descriptors without following links into a directory no
# contained step is ever given, refused if it holds anything but regular files and
# directories, or a setuid, setgid, sticky, group- or world-writable one; ALab's files are
# added there, the smoke reads it read-only, and that copy is what is renamed into place.
scratch_dir partial "${pkg_root}/.${version}.partial.XXXXXX"
upstream_py "${package_input_reads[@]}" --read "${rg_root}" \
  "${partial}" -- '
from codex_package.cli import main
sys.argv = ["build_codex_package.py"] + sys.argv[2:]
sys.exit(main())' --target "${target_triple}" --variant codex --package-version "${version}" \
  --package-dir "${partial}" "${package_prebuilt_args[@]}" --rg-bin "${rg_bin}" --strip all \
  || alab_die "scripts/build_codex_package.py failed (above)"
# package_copy_out <from> <to>: copy the package <from> (which a contained step could write) into
# the new, empty <to>, which none could, and remove <from> unless preserved; refuse nonregular
# and directories, and a setuid, setgid, sticky, group- or world-writable one.
scratch_dir staged "${pkg_root}/.${version}.staged.XXXXXX"
package_copy_out "${partial}" "${staged}"
# LICENSE and NOTICE (Apache-2.0) from the commit's own objects, as regular files.
for f in LICENSE NOTICE; do
  [[ "$(git -C "${repo_root}" ls-tree "${commit}" -- "${f}" | awk '{ print $1, $2 }')" == "100644 blob" ]] \
    || alab_die "${f} at ${sha9} is not a regular file"
  git -C "${repo_root}" cat-file blob "${commit}:${f}" >"${staged}/${f}"
done
# The compiler is the trusted one (resolved from the toolchain before any contained step;
# every build's record matched it), never a trustc from PATH. The upstream base is the one
# the release build compiled in.
recorded_client=""
[[ "${own_line}" -eq 0 ]] || recorded_client="${client_version}"
CODEX_ALAB_UPSTREAM_VERSION="${base_tag}" CODEX_ALAB_UPSTREAM_COMMIT="${upstream_at}" \
  CODEX_ALAB_UPSTREAM_SINCE="${since:-}" CODEX_ALAB_UPSTREAM_DATE="${upstream_date}" \
  CODEX_ALAB_CLIENT_VERSION="${recorded_client}" CODEX_ALAB_BASELINE="${baseline}" \
  alab_build_json "${python}" "${staged}/alab-build.json" \
  "${cfacts}/trusted.json" "${commit}" "${version}" "${target_triple}" "${rg_version}" \
  "+${rg_features}" "${v8_version}" "${packager}" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  || alab_die "could not write alab-build.json (above)"

# ---- 6. smoke ------------------------------------------------------------------------------
# Each step contained, the package read-only, writing only a scratch CODEX_HOME.
scratch_dir codex_home "${TMPDIR:-/tmp}/alab-package-home.XXXXXX"
smoke_fail() {
  alab_die "smoke: $*"
}
smoke_codex() {
  (cd "${wt}" && package_contained --read "${staged}" "${codex_home}" -- /usr/bin/env CODEX_HOME="${codex_home}" \
    "${staged}/bin/codex" "$@")
}
said="$(smoke_codex --version)" || smoke_fail "bin/codex --version failed"
[[ "${said}" == "codex-cli ${version}" ]] || smoke_fail "bin/codex --version said '${said}', not 'codex-cli ${version}'"
echo "smoke: bin/codex --version: ${said}"
if [[ "${package_leaf_versions}" -eq 1 ]]; then
  for f in "${package_first_party[@]:1}"; do
    said="$(cd "${wt}" && package_contained --read "${staged}" "${codex_home}" -- \
      /usr/bin/env CODEX_HOME="${codex_home}" "${staged}/bin/${f}" --version)" \
      || smoke_fail "bin/${f} --version failed"
    [[ "${said}" == "${f} ${version}" ]] \
      || smoke_fail "bin/${f} --version said '${said}', not '${f} ${version}'"
    echo "smoke: bin/${f} --version: ${said}"
  done
fi
if [[ "${package_exec_server}" -eq 1 ]]; then
  package_executor_smoke \
    || smoke_fail "the standalone executor did not report its client identity"
fi
defaults="$(smoke_codex debug packaged-defaults)" || smoke_fail "bin/codex debug packaged-defaults failed"
printf '%s\n' "${defaults}" | "${python}" -I -c '
import sys, tomllib
features = tomllib.loads(sys.stdin.read()).get("features", {})
for name in ("daemon_auto_start", "plugins"):
    if features.get(name) is not False:
        sys.exit(f"compiled defaults: features.{name} is not false")
    print(f"smoke: compiled defaults: features.{name} false")' \
  || smoke_fail "the compiled defaults are not ALab's (above)"
# The client version codex tells upstream's code and OpenAI's servers, when it is not the
# version codex --version said: read as bytes, never run outside the sandbox.
if [[ "${own_line}" -eq 1 ]]; then
  LC_ALL=C grep -a -q -F "${client_version}" "${staged}/bin/codex" \
    || smoke_fail "bin/codex does not carry the client version ${client_version} (CODEX_ALAB_BUILD_VERSION): it would tell OpenAI's servers another"
  echo "smoke: bin/codex carries the client version ${client_version}"
fi
# The session header names the upstream base from the commit compiled in. Read as bytes,
# never run outside the sandbox.
if [[ -n "${upstream_at}" ]]; then
  alab_upstream_carried "${repo_root}" "${commit}" "${upstream_at}" "${staged}/bin/codex" \
    || smoke_fail "bin/codex's upstream commit (above): its session header would name no upstream base"
fi
said="$(cd "${wt}" && package_contained --read "${staged}" "${codex_home}" -- "${staged}/codex-path/rg" --version)" \
  || smoke_fail "codex-path/rg --version failed"
printf '%s\n' "${said}" | grep -q '^features:.*+pcre2' && printf '%s\n' "${said}" | grep -q '^PCRE2 .* is available' \
  || smoke_fail "codex-path/rg has no PCRE2: ${said}"
printf '%s\n' "${said}" | sed '/^$/d; s/^/smoke: codex-path\/rg --version: /'
for f in "${package_first_party[@]/#/bin/}" codex-path/rg; do
  codesign --verify --strict "${staged}/${f}" 2>&1 || alab_die "smoke: codesign --verify --strict ${f} failed"
  kind="$(file -b "${staged}/${f}")"
  [[ "${kind}" == *arm64* ]] || alab_die "smoke: ${f} is not arm64: ${kind}"
  # Only the system's libraries: a Homebrew dylib (libpcre2, say) is absent on another Mac.
  alab_system_libraries "${staged}/${f}" || alab_die "smoke: ${f}: system libraries not verified"
  echo "smoke: ${f}: codesign --verify --strict ok; system libraries only; ${kind}"
done
upstream_py --read "${staged}" "${codex_home}" -- '
import json
from pathlib import Path
from codex_package.layout import validate_package_dir
from codex_package.targets import PACKAGE_VARIANTS, TARGET_SPECS
d = Path(sys.argv[2])
validate_package_dir(d, PACKAGE_VARIANTS["codex"], TARGET_SPECS[sys.argv[3]])
v = json.load(open(d / "codex-package.json"))["version"]
sys.exit(0 if v == sys.argv[4] else "codex-package.json says version " + v)' \
  "${staged}" "${target_triple}" "${version}" || alab_die "smoke: the package layout validator refused it"
echo "smoke: layout validator: ok (codex-package.json version ${version})"
alab_publish_package "${python}" "${staged}" "${pkgdir}" "${pkg_root}" "${pkg_prefix}" \
  "${commit}" "${promote}" || alab_die "package publication/promotion did not finish; the \
smoke-validated package is kept at ${staged} or ${pkgdir} (above)"

# ---- 7. link command, and pruning ----------------------------------------------------------
# alab/prune.py keeps the newest <n> and every package in use: the one atpkg links as codex
# (its link marker <prefix>/links/codex, the bin/ shim and agents/ twin, read without ever
# following a symlink into a binary, and aterm pkg which), any a running process executes
# from (proc_pidpath, ps comm), and any a standing codex-alab session record's resume line
# names (a crash relaunch still owed). It prunes nothing when it cannot tell.
"${python}" -I "${repo_root}/alab/prune.py" --root "${pkg_root}" --keep "${keep}" --prefix "${pkg_prefix}" --repo "${repo_root}" \
  || alab_say "pruning stopped (above); what it had not reached is kept"

echo "package: ${pkgdir} ($(du -sh "${pkgdir}" | awk '{ print $1 }'))"
echo "link it: aterm pkg link codex '${pkgdir}' bin/codex"
# aterm reads a codex build's session record only once it has measured that build (its client
# version, matched whole): until an aterm that did ships, this build's tabs log no turns.
echo "measure it for aterm (${client_version}): in an aterm worktree, targo --unverified lab codex-measure --package '${pkgdir}', then land what it writes"
}

package_entry() {
  local arg value=0 read_only=0 cache revision=HEAD resolved
  local arguments=("$@")
  local output="${HOME}/.local/share/aterm/dev-builds/artifacts"
  # These modes never consume the package cache. Skip option values so a revision
  # or output path that happens to spell an option cannot select a different mode.
  for arg in "$@"; do
    if [[ "${value}" != 0 ]]; then
      [[ "${value}" != out ]] || output="${arg}"
      [[ "${value}" != commit ]] || revision="${arg}"
      value=0
      continue
    fi
    case "${arg}" in
      --out) value=out ;;
      --commit) value=commit ;;
      --keep) value=1 ;;
      -h | --help | --version-only | --client-version-only | --release-check | --release-version \
        | --ledger-row | --upstream-base) read_only=1 ;;
    esac
  done
  if [[ "${read_only}" -eq 1 ]]; then
    # These modes fetch upstream too. Own their children without touching a cache.
    "${ALAB_PYTHON:-/usr/bin/python3}" -I -B "${repo_root}/alab/command_guard.py" -- /bin/bash -c \
      '. "$1"; shift; package_main "$@"' alab-package "${repo_root}/alab/package.sh" "$@"
    return
  fi
  # This is metadata only: source authority precedes compiler probes and the
  # compiler-keyed source pool. Bind moving revision spellings to one full SHA.
  resolved="$(alab_project_source "${repo_root}" "${revision}")" || return $?
  [[ "${resolved}" =~ ^[0-9a-f]{40}$ ]] || alab_die "source metadata named no full commit"
  alab_project_run "${repo_root}" "${resolved}" /bin/bash -c \
    '. "$1"; shift; package_selected_entry "$@"' alab-package \
    "${repo_root}/alab/package.sh" "${arguments[@]}" --commit "${resolved}"
}

# An existing cache guardian (local_update.py) uses the same native selection
# without taking its already held cache lock again.
package_locked_entry() {
  local source="$1"
  shift
  alab_project_run "${repo_root}" "${source}" /bin/bash -c \
    '. "$1"; shift; alab_project_selected || exit $?; package_main "$@"' \
    alab-package "${repo_root}/alab/package.sh" "$@" --commit "${source}"
}

package_selected_entry() {
  alab_project_selected || return $?
  local cache output="${HOME}/.local/share/aterm/dev-builds/artifacts" arg value=0
  for arg in "$@"; do
    if [[ "${value}" != 0 ]]; then
      [[ "${value}" != out ]] || output="${arg}"
      value=0
      continue
    fi
    case "${arg}" in
      --out) value=out ;;
      --commit | --keep) value=skip ;;
    esac
  done
  cache="${ALAB_PACKAGE_CACHE:-${HOME}/.cache/codex-alab}"
  [[ "${output}" == /* ]] || output="${repo_root}/${output}"
  # The command guardian owns the cache's BSD lock through child-group cleanup:
  # V8's temporaries, rg's replacement and package staging share this lifetime.
  alab_package_cache_locked "${cache}" --build-disk "${output}" "${repo_root}" /bin/bash -c \
    '. "$1"; shift; package_main "$@"' alab-package "${repo_root}/alab/package.sh" "$@"
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  package_entry "$@"
fi
