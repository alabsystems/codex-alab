#!/usr/bin/env bash
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
#
# alab/package.sh — build a codex-alab package directory (layout v1, what atpkg and aterm's
# harness read) from a clean commit, for `aterm pkg link codex <pkgdir> bin/codex`.
#
#   alab/package.sh [--commit <rev>] [--out <dir>] [--keep <n>] [--any-commit] [--rebuild]
#   alab/package.sh --version-only [--commit <rev>]   print the version and how it was found
#   alab/package.sh --release-check [--commit <rev>]  print <rev>'s VERSION if a release may
#                                                     carry it, else refuse (exit 1)
#
# <rev> defaults to HEAD of this checkout, which must be clean, and must be on origin/main
# (what ALab ships) unless --any-commit is given. A commit a package in <out>/codex already
# holds (its alab-build.json "source") is refused unless --rebuild is given; the new one
# is then staged beside it as <version>.rebuilt-<UTC time>. Steps:
#   1. the version: MAJOR.MINOR.0-dev.<count>+g<sha9> (~/publication/VERSIONING.md), where
#      MAJOR.MINOR.0 is the higher of <rev>'s VERSION (the public release line, moved only by
#      `pub bump`) and the floor: the next minor above the newest upstream stable tag
#      (refs/upstream-tags/rust-vX.Y.Z) whose branch point (merge-base with upstream/main)
#      <rev> has merged. A release is refused while VERSION is below the floor
#      (--release-check). <count> is git rev-list --count <rev>. upstream's tags and main are
#      fetched first (the remote's configured refspecs, fetch only);
#   2. a scratch worktree at <rev>, ~/Developer/codex-alab-pkg-<sha9>, with the version
#      stamped in [workspace.package] and the lock's workspace entries refreshed (targo
#      metadata, contained, writing only the lock). Never committed; it and its target/ are
#      removed when the run ends (that worktree only: nothing else is pruned);
#   3. the compiler, before any contained step: the atpkg store's admitted Trust (the targo
#      in <prefix>/store/trust/current/bin, resolved physically; <prefix> is ATPKG_BIN's
#      parent), the compiler atpkg-publish pack-package holds a package to, unless ALAB_CARGO
#      names another targo. From a probe crate that targo builds, outside the
#      sandbox, in a new directory no contained step is ever given, whose sysroot's own
#      trustc -vV must agree and say binary: trustc; that sysroot must be the store build's
#      root and its -vV the store trustc's, else the run stops (with ALAB_CARGO: it goes on,
#      labelled unpackable for the channel, alab-build.json compiler.admitted false); then
#      probe crates built contained in codex-rs and in the neutral cache dir must record
#      that compiler, else the run stops;
#      then a release build of codex and codex-code-mode-host (targo --unverified, the
#      lockfile locked, STABLE_GIT_COMMIT=<rev>), with upstream's pinned rusty_v8 archives
#      (scripts/codex_package/v8.py, verified against third_party/v8/*_release_manifests.sha256,
#      read-only to the build), contained (lib.sh, alab_contained), whose
#      target/.rustc_info.json must record that compiler too. A record a contained step
#      wrote is compared as text and never names a program that runs;
#   4. rg built with Trust at the version upstream pins (scripts/codex_package/rg) with
#      PCRE2 (--features pcre2, statically linked, as the vendor rg), from the cache dir and
#      with a cargo home there (~/.cache/codex-alab/cargo-home: cargo install reads the config
#      above its cargo home) so no repo's .cargo/config.toml applies; cached in
#      ~/.cache/codex-alab/rg-<ver>-<compiler id>, rebuilt when either changes; writable only
#      while it is built;
#   5. upstream's scripts/build_codex_package.py stages the package (--strip all) into a
#      hidden <out>/codex/.<version>.partial.*, the only directory it can write (the rg and
#      the binaries it reads are read-only to it). That is copied (cp -RP) into a new
#      hidden .<version>.staged.* no contained step is ever given, refused if it holds a
#      symlink, a special file, or a setuid/setgid/sticky or group/world-writable file;
#      LICENSE, NOTICE (from the commit's objects) and alab-build.json (source commit,
#      version, target, the compiler and whether the store admits it, ripgrep and its
#      features, rusty_v8, the packager's own commit, built_at) are written there;
#   6. a smoke with a scratch CODEX_HOME, each step contained, the staged package read-only:
#      codex --version, the ALab defaults in features list, rg --version (+pcre2),
#      codesign --verify --strict, arm64 and only system libraries on each Mach-O,
#      upstream's layout validator; then the staged package is renamed into place;
#   7. it prints the link command, and keeps the newest <n> (default 3) packages in
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
# to ALAB_DISK_WAIT_MINUTES (default 120). ALAB_CARGO names a targo other than the store's
# (its package is unpackable for the channel unless it builds with the store's compiler),
# ALAB_PYTHON a Python >=
# 3.11, ALAB_PKG_KEEP_WORKTREE=1 keeps the scratch worktree (and its target/) for a person
# to look at.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=alab/lib.sh
. "${repo_root}/alab/lib.sh"

rev=HEAD
out="${HOME}/.local/share/aterm/dev-builds/artifacts"
keep=3
version_only=0
release_check=0
any_commit=0
rebuild=0
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
    --release-check) release_check=1 ;;
    --any-commit) any_commit=1 ;;
    --rebuild) rebuild=1 ;;
    *) alab_die "unknown argument '$1' (see --help)" ;;
  esac
  shift
done

# The toolchain: the atpkg store's admitted Trust, the one `atpkg-publish pack-package`
# holds a package to (aterm crates/atpkg-publish/src/artifact/toolchain.rs, resolve: the
# store's <prefix>/store/trust/current/bin, resolved physically, holding targo and trustc;
# no PATH rung). The targo PATH gives may be rustup's `trust` link to a Trust checkout's
# stage2 (`-vV` commit-hash unknown), whose package the channel refuses. ALAB_CARGO names
# another targo explicitly; a package it builds with any other compiler is labelled
# unpackable for the channel (alab-build.json compiler.admitted false), never refused.
pkg_prefix="$(dirname "${ATPKG_BIN:-${HOME}/Library/Application Support/aterm/pkg/bin}")"
if [[ -z "${ATPKG_BIN:-}" && -d "${pkg_prefix}/bin" ]]; then
  # The sandbox reads the store through ATPKG_BIN (lib.sh, alab_contain_profile).
  ATPKG_BIN="${pkg_prefix}/bin"
  export ATPKG_BIN
fi
store_bin=""
if store_bin="$(cd "${pkg_prefix}/store/trust/current/bin" 2>/dev/null && pwd -P)" \
  && [[ -x "${store_bin}/targo" && -x "${store_bin}/trustc" ]]; then
  :
else
  store_bin=""
fi
if [[ -n "${ALAB_CARGO:-}" ]]; then
  cargo="${ALAB_CARGO}"
  cargo_said="ALAB_CARGO"
elif [[ -n "${store_bin}" ]]; then
  cargo="${store_bin}/targo"
  cargo_said="the atpkg store's admitted Trust"
else
  cargo=""
  cargo_said=""
fi
# Contained steps build with the clone's private cargo home (lib.sh, alab_cargo_home), named
# here because the rg build runs outside any checkout.
ALAB_CARGO_HOME="${ALAB_CARGO_HOME:-$(git -C "${repo_root}" rev-parse --path-format=absolute --git-common-dir)/alab-cargo-home}"
export ALAB_CARGO_HOME
target_triple=aarch64-apple-darwin
cd "${repo_root}"
alab_upstream_ready

# ---- 1. the commit and its version ---------------------------------------------------------
[[ -z "$(git status --porcelain)" ]] || alab_die "this checkout (${repo_root}) is not clean: a \
version is never minted from a dirty tree (~/publication/VERSIONING.md). Commit or remove the changes."
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
git fetch --quiet --no-tags upstream "${spec_tags}" "${spec_main}" || alab_die "fetching upstream \
failed (above): the version rule needs its current tags and main. If git refused a \
non-fast-forward of upstream/main, upstream rewrote its history: a person decides (alab/README.md)"
git rev-parse -q --verify refs/remotes/upstream/main >/dev/null \
  || alab_die "no upstream/main here: run alab/setup.sh, then git fetch upstream"

# The newest stable upstream tag (rust-vX.Y.Z, no pre-release) whose branch point the commit
# has merged. A release tag is never an ancestor of upstream/main (alab/README.md, "What
# upstream is like"), so its branch point is its merge-base with upstream/main.
base_tag="" base_bp=""
while read -r stable; do
  [[ -n "${stable}" ]] || continue
  bp="$(git merge-base "refs/upstream-tags/rust-v${stable}" refs/remotes/upstream/main)" || continue
  if git merge-base --is-ancestor "${bp}" "${commit}"; then
    base_tag="${stable}"
    base_bp="${bp}"
    break
  fi
done < <(git for-each-ref --format='%(refname:strip=2)' 'refs/upstream-tags/rust-v*' \
  | sed -n 's/^rust-v\([0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*\)$/\1/p' \
  | sort -t. -k1,1nr -k2,2nr -k3,3nr)
# The line. The commit's own VERSION (MAJOR.MINOR.0) is the public release line, moved only by
# `pub bump codex-alab`. The floor is the next minor above that stable: a build that holds
# upstream's X.Y code must never call itself older (the server picks models by
# client_version). A dev package takes the higher of the two, so packaging never waits on a
# bump: upstream cuts a stable every few days, and VERSION moves only when a release is cut.
# A release is refused below the floor instead (--release-check, the engine's
# publish/pre-promote). A commit from before VERSION existed takes the floor, the rule it was
# packaged under. A published snapshot is one commit with no history, so it has merged no
# branch point to read: its VERSION was held to the floor when it was cut.
committed="$(git show "${commit}:VERSION" 2>/dev/null | tr -d '[:space:]')" || committed=""
if [[ -n "${committed}" ]]; then
  [[ "${committed}" =~ ^([0-9]+)\.([0-9]+)\.0$ ]] || alab_die "${sha9}'s VERSION is not of the \
form MAJOR.MINOR.0 (found '${committed}'): it names codex-alab's release line (alab/README.md, \
Decision 8)"
  v_major="${BASH_REMATCH[1]}" v_minor="${BASH_REMATCH[2]}"
fi
behind=0
if [[ -n "${base_tag}" ]]; then
  major="${base_tag%%.*}"
  minor="${base_tag#*.}"
  minor="${minor%%.*}"
  floor="${major}.$((minor + 1)).0"
  floor_why="the floor ${floor}, the next minor above rust-v${base_tag}, the newest upstream
alab:     stable whose branch point ${base_bp:0:9} (merge-base with upstream/main) ${sha9} has merged"
  if [[ -z "${committed}" ]]; then
    line="${floor}"
    how="${floor_why} (${sha9} predates VERSION)"
  elif (( v_major < major || (v_major == major && v_minor <= minor) )); then
    line="${floor}"
    behind=1
    how="${floor_why}; VERSION ${committed} is behind it, so \`pub bump codex-alab\` is due
alab:     before the next release (--release-check refuses until then)"
  else
    line="${committed}"
    how="VERSION at ${sha9}, at least ${floor_why}"
  fi
elif [[ "${count}" -eq 1 ]]; then
  [[ -n "${committed}" ]] || alab_die "${sha9} is a published snapshot (one commit) with no \
VERSION: nothing names its line"
  line="${committed}"
  how="VERSION at ${sha9}, a published snapshot (one commit, no history): no branch point to hold it to"
else
  alab_die "${sha9} has merged the branch point of no upstream stable tag \
(refs/upstream-tags/rust-v*): run alab/setup.sh to fetch them"
fi
if [[ "${release_check}" -eq 1 ]]; then
  [[ -n "${committed}" ]] || alab_die "${sha9} has no VERSION: a release needs one (Decision 8)"
  if [[ "${behind}" -eq 1 ]]; then
    alab_die "VERSION ${committed} at ${sha9} is below the floor ${floor}, the next minor above \
rust-v${base_tag} (whose branch point ${base_bp:0:9} it has merged): a release would call itself \
older than the upstream code it holds. Move the line with the publication engine, never by hand: \
PUBLISH_REPO_PATH=<a clean checkout of origin/main> pub bump codex-alab --commit (one minor per \
run) until it is at least ${floor}, push it, then release"
  fi
  printf '%s\n' "${committed}"
  exit 0
fi
version="${line}-dev.${count}+g${sha9}"
# The upstream alignment: provenance beside the version, never part of it (Decision 8).
upstream_at="$(git merge-base "${commit}" refs/remotes/upstream/main 2>/dev/null)" || upstream_at=""
if [[ -n "${upstream_at}" ]]; then
  upstream_said="openai/codex@${upstream_at:0:12}, the upstream commit ${sha9} merged last"
else
  upstream_said="none in the history of ${sha9}"
fi
cat >&2 <<EOF
alab: version ${version}
alab:   line ${line}: ${how}
alab:   -dev.${count}: git rev-list --count ${sha9} (ordering only)
alab:   +g${sha9}: the commit (identity)
alab:   upstream: ${upstream_said}
EOF
if [[ "${version_only}" -eq 1 ]]; then
  printf '%s\n' "${version}"
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
[[ -n "${cargo}" ]] || alab_die "no Trust in the atpkg store: ${pkg_prefix}/store/trust/current/bin \
holds no targo and trustc. Install it (aterm pkg install trust, then aterm pkg which trustc), or \
name a targo with ALAB_CARGO=<targo>: a package another compiler builds is unpackable for the \
channel (atpkg-publish pack-package admits the store's Trust only)"
command -v "${cargo}" >/dev/null || alab_die "no ${cargo} (${cargo_said}; the Trust toolchain: aterm help rust)"
command -v otool >/dev/null || alab_die "no otool (the Xcode command line tools: xcode-select --install)"
python="${ALAB_PYTHON:-}"
if [[ -z "${python}" ]]; then
  for p in /opt/homebrew/bin/python3.14 /opt/homebrew/bin/python3.13 /opt/homebrew/bin/python3.12 \
    /opt/homebrew/bin/python3.11 python3; do
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
cache="${HOME}/.cache/codex-alab"
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
needed, after waiting $((waited / 60)) minutes: free space (each release target/ is 12 to 15 GB), then rerun"
    alab_say "disk: ${free} GB free where ${dir} is, ${need} GB needed: checking again in 5 minutes"
    sleep 300
    waited=$((waited + 300))
  done
}

# ---- 2. the scratch worktree, stamped ------------------------------------------------------
wt="${HOME}/Developer/codex-alab-pkg-${sha9}"
[[ ! -e "${wt}" ]] || alab_die "${wt} exists: a run in progress, or one that was killed. Remove a \
stale one with: git -C ${repo_root} worktree remove --force ${wt}"
# This run's scratch paths (the partial and staged package, the rg being built, the compiler
# probes, the smoke's CODEX_HOME), removed when it ends, however it ends.
scratch_paths=()
cleanup() {
  local p
  for p in ${scratch_paths[@]+"${scratch_paths[@]}"}; do
    rm -rf "${p:?}"
  done
  [[ -n "${wt_made:-}" ]] || return 0
  if [[ "${ALAB_PKG_KEEP_WORKTREE:-0}" == 1 ]]; then
    alab_say "kept the scratch worktree ${wt} (ALAB_PKG_KEEP_WORKTREE=1); remove it with: git -C \
${repo_root} worktree remove --force ${wt}"
  else
    # This run's worktree only: never a prune, which drops every stale registration in the
    # clone (a person's worktree on an unmounted disk, say).
    git -C "${repo_root}" worktree remove --force "${wt}" 2>/dev/null || {
      rm -rf "${wt:?}"
      alab_say "git could not remove the worktree ${wt}; its files are gone, its registration \
stays until: git -C ${repo_root} worktree remove --force ${wt}"
    }
  fi
}
trap cleanup EXIT
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
# build can write its own target/.rustc_info.json (and a process it leaves behind can keep
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
  (cd "${p}" && CARGO_TARGET_DIR="${p}/target" "${cargo}" --unverified build --quiet \
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
print(cid)' "${p}/target/.rustc_info.json" "$1")" || alab_die "refusing the compiler ${cargo} builds with (above)"
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
  (cd "$1" && alab_contained "${p}" -- /usr/bin/env CARGO_TARGET_DIR="${p}/target" \
    "${cargo}" --unverified build --quiet --manifest-path "${p}/Cargo.toml") >&2 \
    || alab_die "${cargo} could not build a probe crate in $1 (above)"
  same_compiler "${p}/target" "targo in $1" || alab_die "refusing the compiler ${cargo} builds with in $1 (above)"
}
# admitted_compiler <json>: whether the trusted compiler is the atpkg store's admitted Trust,
# as pack-package's check_compiler holds a package to it: the recorded sysroot is the store
# build's root (physically: bin/'s parent), and its -vV is that root's own trustc -vV byte for
# byte (the binaries' digest and the id are then the ones pack-package derives, both taken
# over that very root). Adds "admitted" (true or false) and "admitted_why" to <json>, which is
# alab-build.json's compiler block. Prints the reason; exits 1 when it is not admitted.
admitted_compiler() {
  "${python}" -I -c '
import json, os, subprocess, sys
path, store_bin = sys.argv[1], sys.argv[2]
c = json.load(open(path))
sysroot, said = c["sysroot"], c["rustc"]
store = "the atpkg store" + chr(39) + "s"
if not store_bin:
    ok, why = False, "the atpkg store holds no Trust (aterm pkg install trust)"
else:
    root = os.path.dirname(store_bin)
    vv = subprocess.run([os.path.join(store_bin, "trustc"), "-vV"], capture_output=True, text=True).stdout
    first = vv.splitlines()[0] if vv else "nothing"
    if os.path.realpath(sysroot) != os.path.realpath(root):
        ok, why = False, f"built with the compiler at {sysroot}, not {store} admitted Trust at {root}"
    elif c["rustc_vV"] != vv:
        ok, why = False, f"the compiler said {said!r}; {store} trustc -vV says {first!r}"
    else:
        ok, why = True, f"{store} admitted Trust at {root}"
c["admitted"], c["admitted_why"] = ok, why
json.dump(c, open(path, "w"))
print(why)
sys.exit(0 if ok else 1)' "$1" "${store_bin}"
}
scratch_dir cfacts "${cache}/.compiler.XXXXXX"
trusted_compiler "${cfacts}/trusted.json"
alab_say "compiler ${compiler_id}: $(sed -n 's/.*"rustc": "\([^"]*\)".*/\1/p' "${cfacts}/trusted.json"), \
$(sed -n 's/.*"sysroot": "\([^"]*\)".*/\1/p' "${cfacts}/trusted.json") (${cargo_said}: ${cargo})"
unpackable=""
if admitted_said="$(admitted_compiler "${cfacts}/trusted.json")"; then
  alab_say "compiler admitted: ${admitted_said}"
else
  [[ -n "${admitted_said}" ]] || alab_die "could not hold the compiler to the atpkg store's Trust (above)"
  [[ -n "${ALAB_CARGO:-}" ]] || alab_die "refusing the compiler the store's targo builds with: \
${admitted_said}. atpkg-publish pack-package refuses every package another compiler built"
  unpackable="${admitted_said}"
  cat >&2 <<EOF
alab: ************************************************************************************
alab: UNPACKABLE FOR THE CHANNEL: ALAB_CARGO=${ALAB_CARGO}
alab:   ${admitted_said}.
alab:   atpkg-publish pack-package refuses this package; alab-build.json records
alab:   compiler.admitted false. Unset ALAB_CARGO to build on the store's admitted Trust.
alab: ************************************************************************************
EOF
fi

need_disk "$(dirname "${wt}")"
git worktree add --quiet --detach "${wt}" "${commit}"
wt_made=1
[[ -z "$(git -C "${wt}" status --porcelain)" ]] || alab_die "the scratch worktree ${wt} is not clean"

manifest="${wt}/codex-rs/Cargo.toml"
[[ -f "${manifest}" ]] || alab_die "no codex-rs/Cargo.toml at ${sha9}"
awk -v v="${version}" '
  /^\[workspace\.package\][[:space:]]*$/ { in_pkg = 1; print; next }
  in_pkg && /^\[/ { in_pkg = 0 }
  in_pkg && !done && /^version[[:space:]]*=/ { print "version = \"" v "\""; done = 1; next }
  { print }
  END { exit !done }' "${manifest}" >"${manifest}.alab" \
  || alab_die "no version in [workspace.package] of codex-rs/Cargo.toml at ${sha9}"
mv "${manifest}.alab" "${manifest}"
export CARGO_INCREMENTAL=0
export CARGO_TARGET_DIR="${wt}/codex-rs/target"
mkdir -p "${CARGO_TARGET_DIR}"
# Contained: it reads upstream's .cargo/config.toml. It may write only the lock.
(cd "${wt}/codex-rs" && alab_contained "${wt}/codex-rs/Cargo.lock" "${CARGO_TARGET_DIR}" -- \
  "${cargo}" metadata --format-version 1 >/dev/null) \
  || alab_die "${cargo} metadata could not resolve codex-rs at ${sha9}"
# The stamp may move the workspace members' versions in the lock, and nothing else.
moved="$(git -C "${wt}" diff --name-only | grep -v -x -e codex-rs/Cargo.toml -e codex-rs/Cargo.lock || true)"
[[ -z "${moved}" ]] || alab_die "stamping the version changed more than the manifest and lock: ${moved}"
moved="$(git -C "${wt}" diff -U0 -- codex-rs/Cargo.lock | grep '^[-+]' | grep -v -e '^+++ ' -e '^--- ' \
  | grep -v -e '^[-+]version = ' || true)"
[[ -z "${moved}" ]] || alab_die "refreshing the lock moved more than the workspace versions:
${moved}"
alab_say "stamped ${version} in ${wt} (not committed)"

# ---- 3. the compiler, then codex and codex-code-mode-host --------------------------------
probe_compiler "${wt}/codex-rs"
probe_compiler "${cache}"

scripts="${wt}/scripts"
# upstream_py <writable-dir>... -- <python code>: run upstream's packaging Python (scripts/
# codex_package) at this commit, contained like any upstream code.
upstream_py() {
  local dirs=()
  while [[ "$1" != -- ]]; do
    dirs+=("$1")
    shift
  done
  shift
  (cd "${wt}" && alab_contained "${dirs[@]}" -- /usr/bin/env CODEX_REPO_ROOT="${wt}" \
    "${python}" -I -c "import sys; sys.path.insert(0, sys.argv[1]); ${1}" "${scripts}" "${@:2}")
}

# upstream's pinned rusty_v8 pair, fetched into the cache and verified there by v8.py.
v8_env="$(upstream_py "${v8_cache}" -- '
from pathlib import Path
from codex_package.targets import TARGET_SPECS
from codex_package.v8 import resolve_codex_v8_cargo_env, resolved_v8_crate_version
env = resolve_codex_v8_cargo_env(TARGET_SPECS[sys.argv[2]], environ={}, cache_root=Path(sys.argv[3]))
print(resolved_v8_crate_version())
print(env["RUSTY_V8_ARCHIVE"])
print(env["RUSTY_V8_SRC_BINDING_PATH"])' "${target_triple}" "${v8_cache}")" \
  || alab_die "could not fetch and verify upstream's rusty_v8 archives (above)"
v8_version="$(printf '%s\n' "${v8_env}" | sed -n 1p)"
v8_archive="$(printf '%s\n' "${v8_env}" | sed -n 2p)"
v8_binding="$(printf '%s\n' "${v8_env}" | sed -n 3p)"
# Checked again here, in ALab's code, so a change to v8.py cannot skip it: the release's
# checksum manifest matches the digest the tree pins, and each file matches the manifest.
v8_manifest="$(dirname "${v8_archive}")/rusty_v8_ptrcomp_sandbox_release_${target_triple}.sha256"
v8_trusted="${wt}/third_party/v8/rusty_v8_${v8_version//./_}_release_manifests.sha256"
[[ -f "${v8_trusted}" ]] || alab_die "no ${v8_trusted#"${wt}/"} at ${sha9}: rusty_v8 ${v8_version} \
has no pinned checksum manifest"
[[ -f "${v8_manifest}" ]] || alab_die "v8.py left no ${v8_manifest}"
want="$(awk -v n="$(basename "${v8_manifest}")" '$2 == n { print $1 }' "${v8_trusted}")"
[[ -n "${want}" && "$(shasum -a 256 "${v8_manifest}" | awk '{ print $1 }')" == "${want}" ]] \
  || alab_die "${v8_manifest} does not match ${v8_trusted}"
for f in "${v8_archive}" "${v8_binding}"; do
  want="$(awk -v n="$(basename "${f}")" '$2 == n { print $1 }' "${v8_manifest}")"
  [[ -n "${want}" && "$(shasum -a 256 "${f}" | awk '{ print $1 }')" == "${want}" ]] \
    || alab_die "${f} does not match ${v8_manifest}"
done
alab_say "rusty_v8 ${v8_version}: $(basename "${v8_archive}") verified against $(basename "${v8_trusted}")"

need_disk "${wt}"
alab_say "building codex and codex-code-mode-host (release) at ${sha9}"
(cd "${wt}/codex-rs" && alab_contained --read "${v8_cache}" "${CARGO_TARGET_DIR}" -- /usr/bin/env \
  STABLE_GIT_COMMIT="${commit}" RUSTY_V8_ARCHIVE="${v8_archive}" RUSTY_V8_SRC_BINDING_PATH="${v8_binding}" \
  "${cargo}" --unverified build --release --locked -p codex-cli --bin codex \
  -p codex-code-mode-host --bin codex-code-mode-host) || alab_die "the release build failed (above)"
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
  rg_said="$(cd "${wt}" && alab_contained --read "$(dirname "$1")" -- "$1" --version 2>&1)" || rc=$?
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
  (cd "${cache}" && ALAB_CARGO_HOME="${cache}/cargo-home" alab_contained "${rg_partial}" -- \
    /usr/bin/env CARGO_TARGET_DIR="${rg_partial}/target" \
    PCRE2_SYS_STATIC=1 "${cargo}" --unverified install --locked --features "${rg_features}" \
    "ripgrep@${rg_version}" --root "${rg_partial}") || alab_die "building ripgrep ${rg_version} failed (above)"
  same_compiler "${rg_partial}/target" "the rg build" || alab_die "refusing the rg build (above)"
  rm -rf "${rg_partial:?}/target"
  rg_ok "${rg_partial}/bin/rg" || alab_die "the rg just built is not ripgrep ${rg_version} with PCRE2: \
$(printf '%s\n' "${rg_said}" | tr '\n' ' ')"
  rm -rf "${rg_root:?}"
  mv "${rg_partial}" "${rg_root}"
fi

# ---- 5. the package ------------------------------------------------------------------------
# upstream's packager stages it in a new hidden directory, the only one it may write; the
# binaries and rg it reads are read-only to it. A process it leaves behind can keep writing
# there, so nothing more is written there and nothing there is published: the package is
# copied (cp -RP: a symlink is copied, never followed) into another new directory no
# contained step is ever given, refused if it holds anything but regular files and
# directories, or a setuid, setgid, sticky, group- or world-writable one; ALab's files are
# added there, the smoke reads it read-only, and that copy is what is renamed into place.
scratch_dir partial "${pkg_root}/.${version}.partial.XXXXXX"
release="${CARGO_TARGET_DIR}/release"
upstream_py --read "${release}/codex" --read "${release}/codex-code-mode-host" --read "${rg_root}" \
  "${partial}" -- '
from codex_package.cli import main
sys.argv = ["build_codex_package.py"] + sys.argv[2:]
sys.exit(main())' --target "${target_triple}" --variant codex --package-version "${version}" \
  --package-dir "${partial}" --entrypoint-bin "${release}/codex" \
  --code-mode-host-bin "${release}/codex-code-mode-host" --rg-bin "${rg_bin}" --strip all \
  || alab_die "scripts/build_codex_package.py failed (above)"
# copy_out <from> <to>: copy the package <from> (which a contained step could write) into
# the new, empty <to>, which none could, and remove <from>; refuse anything but regular files
# and directories, and a setuid, setgid, sticky, group- or world-writable one.
copy_out() {
  local odd
  cp -RP "$1/." "$2/" || alab_die "could not copy the staged package out of $1"
  rm -rf "${1:?}"
  odd="$(find "$2" \( ! -type f ! -type d \) -print -o -perm +7022 -print | sed -n 1,5p)"
  [[ -z "${odd}" ]] || alab_die "the package upstream's packager staged holds what a package must not \
(a symlink, a special file, or a setuid/setgid/sticky or group/world-writable file): ${odd}"
  chmod 0755 "$2"
}
scratch_dir staged "${pkg_root}/.${version}.staged.XXXXXX"
copy_out "${partial}" "${staged}"
# LICENSE and NOTICE (Apache-2.0) from the commit's own objects, as regular files.
for f in LICENSE NOTICE; do
  [[ "$(git -C "${repo_root}" ls-tree "${commit}" -- "${f}" | awk '{ print $1, $2 }')" == "100644 blob" ]] \
    || alab_die "${f} at ${sha9} is not a regular file"
  git -C "${repo_root}" cat-file blob "${commit}:${f}" >"${staged}/${f}"
done
# The compiler is the trusted one (resolved from the toolchain before any contained step;
# every build's record matched it), never a trustc from PATH.
"${python}" -I -c '
import json, sys
out, compiler = sys.argv[1], json.load(open(sys.argv[2]))
keys = ["source", "version", "target", "ripgrep", "ripgrep_features", "rusty_v8", "packager", "built_at"]
d = dict(zip(keys, sys.argv[3:]))
d = {"source": d["source"], "version": d["version"], "target": d["target"], "compiler": compiler,
     "ripgrep": d["ripgrep"], "ripgrep_features": d["ripgrep_features"], "rusty_v8": d["rusty_v8"],
     "packager": d["packager"], "built_at": d["built_at"]}
with open(out, "w") as f:
    json.dump(d, f, indent=2)
    print(file=f)' "${staged}/alab-build.json" "${cfacts}/trusted.json" "${commit}" "${version}" \
  "${target_triple}" "${rg_version}" "+${rg_features}" "${v8_version}" "${packager}" \
  "$(date -u +%Y-%m-%dT%H:%M:%SZ)"

# ---- 6. smoke ------------------------------------------------------------------------------
# Each step contained, the package read-only, writing only a scratch CODEX_HOME.
scratch_dir codex_home "${TMPDIR:-/tmp}/alab-package-home.XXXXXX"
smoke_fail() {
  alab_die "smoke: $*"
}
smoke_codex() {
  (cd "${wt}" && alab_contained --read "${staged}" "${codex_home}" -- /usr/bin/env CODEX_HOME="${codex_home}" \
    "${staged}/bin/codex" "$@")
}
said="$(smoke_codex --version)" || smoke_fail "bin/codex --version failed"
[[ "${said}" == "codex-cli ${version}" ]] || smoke_fail "bin/codex --version said '${said}', not 'codex-cli ${version}'"
echo "smoke: bin/codex --version: ${said}"
features="$(smoke_codex features list)" || smoke_fail "bin/codex features list failed"
for f in daemon_auto_start plugins; do
  state="$(printf '%s\n' "${features}" | awk -v f="${f}" '$1 == f { print $NF }')"
  [[ "${state}" == false ]] || smoke_fail "features list says ${f} is '${state:-missing}', not false (ALab's default)"
  echo "smoke: features list: ${f} ${state}"
done
said="$(cd "${wt}" && alab_contained --read "${staged}" "${codex_home}" -- "${staged}/codex-path/rg" --version)" \
  || smoke_fail "codex-path/rg --version failed"
printf '%s\n' "${said}" | grep -q '^features:.*+pcre2' && printf '%s\n' "${said}" | grep -q '^PCRE2 .* is available' \
  || smoke_fail "codex-path/rg has no PCRE2: ${said}"
printf '%s\n' "${said}" | sed '/^$/d; s/^/smoke: codex-path\/rg --version: /'
for f in bin/codex bin/codex-code-mode-host codex-path/rg; do
  codesign --verify --strict "${staged}/${f}" 2>&1 || alab_die "smoke: codesign --verify --strict ${f} failed"
  kind="$(file -b "${staged}/${f}")"
  [[ "${kind}" == *arm64* ]] || alab_die "smoke: ${f} is not arm64: ${kind}"
  # Only the system's libraries: a Homebrew dylib (libpcre2, say) is absent on another Mac.
  libs="$(otool -L "${staged}/${f}" | tail -n +2 | awk '{ print $1 }' \
    | grep -v -e '^/usr/lib/' -e '^/System/Library/' || true)"
  [[ -z "${libs}" ]] || alab_die "smoke: ${f} links libraries outside the system: ${libs}"
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
mv "${staged}" "${pkgdir}" || alab_die "could not move the package into place at ${pkgdir}"

# ---- 7. link command, and pruning ----------------------------------------------------------
# alab/prune.py keeps the newest <n> and every package in use: the one atpkg links as codex
# (its link marker <prefix>/links/codex, the bin/ shim and agents/ twin, read without ever
# following a symlink into a binary, and aterm pkg which), any a running process executes
# from (proc_pidpath, ps comm), and any a standing codex-alab session record's resume line
# names (a crash relaunch still owed). It prunes nothing when it cannot tell.
"${python}" -I "${repo_root}/alab/prune.py" --root "${pkg_root}" --keep "${keep}" --prefix "${pkg_prefix}" \
  || alab_say "pruning stopped (above); what it had not reached is kept"

echo "package: ${pkgdir} ($(du -sh "${pkgdir}" | awk '{ print $1 }'))"
echo "link it: aterm pkg link codex '${pkgdir}' bin/codex"
if [[ -n "${unpackable}" ]]; then
  echo "UNPACKABLE FOR THE CHANNEL (ALAB_CARGO=${ALAB_CARGO}): ${unpackable}; atpkg-publish pack-package refuses it"
fi
