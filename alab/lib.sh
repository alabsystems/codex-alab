# shellcheck shell=bash
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
#
# Shared by alab/*.sh. Kept to bash 3.2: macOS's /bin/bash runs these.

alab_library_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"

ALAB_UPSTREAM_URL="https://github.com/openai/codex.git"

alab_say() { printf 'alab: %s\n' "$*" >&2; }
alab_die() {
  printf 'alab: %s\n' "$*" >&2
  exit 1
}

# alab_spec_lines: the pathspecs in an alab/*-paths file on stdin, one per line (comments,
# blank lines and surrounding blanks dropped).
alab_spec_lines() {
  sed -e '/^[[:space:]]*#/d' -e '/^[[:space:]]*$/d' -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//'
}

# alab_specs <file>: the same, from a file (nothing when it does not exist).
alab_specs() {
  [[ -f "$1" ]] || return 0
  alab_spec_lines <"$1"
}

# The marker every ALab hook in an upstream file carries: ALAB: or ALAB(<topic>):. Any topic
# text counts, so a hook tagged ALAB(tui_hooks): or ALAB(TUI): is not silently unguarded.
ALAB_MARKER_RE='ALAB(\([^)]+\))?:'

# alab_pristine <checkout> <rev>: that checkout's pristine upstream tree, a worktree inside its own
# git dir. alab/check.sh and alab/sync.sh build and test upstream alone there, in the tree's
# own codex-rs/target: cargo judges a workspace crate fresh by file times, so a target shared
# with the ALab tree could hand either tree the other's builds. One per checkout, so a check
# in one checkout never moves the tree under another's. Each resolved commit has its own
# path: advancing upstream never checks out over a retained build world.
alab_pristine() {
  local base want
  want="$(git -C "$1" rev-parse -q --verify "$2^{commit}")" || return 1
  base="$(git -C "$1" rev-parse --path-format=absolute --git-path alab-pristine)" || return 1
  printf '%s-%s\n' "${base}" "${want}"
}

# alab_pristine_config <alab-tree>: the cargo --config value under which a pristine upstream tree
# builds with the crates <alab-tree>'s alab/vendor carries in place of upstream's (each one's
# ALAB-SOURCE.json: the very version and archive upstream locks, adapted to the Trust toolchain;
# alab/vendor/README.md), or nothing when it carries none. Without them upstream alone does not
# build under Trust at all (allocative 0.3.6: E0119, `impl Allocative for !` beside Infallible's,
# which Trust's std makes one type), so no pristine comparison could run. ALab's tree takes them
# through codex-rs/Cargo.toml's [patch.crates-io]; upstream's tree is never edited, so they come
# in as path overrides, which change no dependency and leave upstream's lockfile as it is
# (--locked holds): upstream alone, as this toolchain builds it.
alab_pristine_config() {
  local record dir paths=""
  for record in "$1"/alab/vendor/*/ALAB-SOURCE.json; do
    [[ -f "${record}" ]] || continue
    dir="$(alab_real "$(dirname "${record}")")" || return 1
    case "${dir}" in *\"* | *\\*)
      alab_say "alab_pristine_config: ${dir} cannot be named in a cargo --config string"
      return 1
      ;;
    esac
    paths="${paths:+${paths}, }\"${dir}\""
  done
  [[ -z "${paths}" ]] || printf 'paths = [%s]\n' "${paths}"
}

# A pristine tree may contain ignored build outputs, but no changed or untracked source.
alab_pristine_clean() {
  local untracked common actual_common top
  [[ -d "$1" && ! -L "$1" && -f "$1/.git" && ! -L "$1/.git" ]] || return 1
  common="$(git -C "$3" rev-parse --path-format=absolute --git-common-dir)" || return 1
  actual_common="$(GIT_OPTIONAL_LOCKS=0 git -C "$1" rev-parse --path-format=absolute --git-common-dir 2>/dev/null)" || return 1
  top="$(GIT_OPTIONAL_LOCKS=0 git -C "$1" rev-parse --show-toplevel 2>/dev/null)" || return 1
  [[ "${actual_common}" == "${common}" && "${top}" == "$(alab_real "$1")" \
    && "$(GIT_OPTIONAL_LOCKS=0 git -C "$1" rev-parse -q --verify HEAD 2>/dev/null)" == "$2" ]] \
    && GIT_OPTIONAL_LOCKS=0 git -C "$1" diff --quiet HEAD -- 2>/dev/null \
    && untracked="$(GIT_OPTIONAL_LOCKS=0 git -C "$1" ls-files --others --exclude-standard 2>/dev/null)" \
    && [[ -z "${untracked}" ]]
}

# alab_pristine_trees <checkout>: legacy path plus registered clean revision trees for
# daily's existing target-only cleanup, NUL-delimited. Unknown, replaced and dirty keyed paths stay kept.
alab_pristine_trees() {
  local base field tree key
  base="$(git -C "$1" rev-parse --path-format=absolute --git-path alab-pristine)" || return 1
  printf '%s\0' "${base}"
  while IFS= read -r -d '' field; do
    [[ "${field}" == 'worktree '* ]] || continue
    tree="${field#worktree }"
    [[ "${tree}" == "${base}"-* ]] || continue
    key="${tree#"${base}"-}"
    [[ "${key}" =~ ^[0-9a-f]{40}([0-9a-f]{24})?$ ]] || continue
    alab_pristine_clean "${tree}" "${key}" "$1" || continue
    printf '%s\0' "${tree}"
  done < <(git -C "$1" worktree list --porcelain -z)
}

# alab_pristine_at <checkout> <rev>: create the revision's tree with one ordinary add.
# Occupied dirty, mismatched, broken or symlink paths are kept and refused, regardless of
# cleanup policy. No repair or forced checkout can overwrite an existing build world.
# A tree already at <rev> and unmodified is left alone, so a call from alab/check.sh run
# contained (alab_contained: git cannot write there) succeeds once alab/sync.sh has put it there.
alab_pristine_at() {
  local dir want
  want="$(git -C "$1" rev-parse -q --verify "$2^{commit}")" || return 1
  dir="$(alab_pristine "$1" "${want}")" || return 1
  if alab_pristine_clean "${dir}" "${want}" "$1"; then
    return 0
  fi
  if [[ ! -e "${dir}" && ! -L "${dir}" ]]; then
    git -C "$1" worktree add --quiet --detach "${dir}" "${want}"
    return $?
  fi
  alab_say "kept pristine tree ${dir}: occupied revision path is not clean at ${want}"
  return 1
}

# alab_locked <lockfile> <command...>: run <command> holding an exclusive kernel lock on
# <lockfile> (BSD flock semantics: released when its holder exits, however it exits, so no
# lock outlives a killed run). Exits 75 at once if another process holds it, and 69 (it says
# why) when neither tool is installed: it returns, so a caller can tell a person.
alab_locked() {
  if [[ -x /usr/bin/lockf ]]; then
    /usr/bin/lockf -k -s -t 0 "$@"
  elif command -v flock >/dev/null 2>&1; then
    flock -n -o -E 75 "$@"
  else
    alab_say "neither lockf (macOS) nor flock (Linux) is installed: alab/ needs one for its lock"
    return 69
  fi
}

# alab_build_target_ready <workspace> <target>: publish the real workspace to a capable Rust
# cache registrar BEFORE making its target, then print the existing Cargo output location.
# Old hosts keep unmarked targets unmanaged. A marked target always requires a working owner
# gate; neither an unavailable helper nor a refused publication may start a build in it.
alab_build_target_ready() {
  local workspace target home marked_root capabilities="" marked=0 helper=""
  local registrar=() required=()
  workspace="$(alab_real "$1")" || return 1
  target="$2"
  case "${target}" in /*) ;; *) target="${workspace}/${target}" ;; esac
  target="$(alab_real "${target}")" || return 1
  home="$(alab_real "${HOME:?}")" || return 1
  for helper in "${target}/.aterm-build-cache.json" "${target}/build.noindex/.aterm-build-cache.json"; do
    if [[ -e "${helper}" || -L "${helper}" ]]; then
      marked=1
      marked_root="$(alab_real "$(dirname "${helper}")")" || return 75
      case "${marked_root}" in "${home}"/*) ;; *)
        alab_say "the managed cache ${marked_root} is outside the current HOME; no build started"
        return 75 ;;
      esac
    fi
  done
  if [[ -n "${ALAB_ATERM:-}" ]]; then
    registrar=("${ALAB_ATERM}" pkg)
  elif [[ -n "${ATPKG_BIN:-}" && -x "${ATPKG_BIN}/atpkg" ]]; then
    registrar=("${ATPKG_BIN}/atpkg")
  elif helper="$(command -v atpkg 2>/dev/null)"; then
    registrar=("${helper}")
  elif helper="$(command -v aterm 2>/dev/null)"; then
    registrar=("${helper}" pkg)
  fi
  if [[ ${#registrar[@]} -gt 0 ]]; then
    capabilities="$(HOME="${home}" "${registrar[@]}" __build_cache_register --capabilities 2>/dev/null)" \
      || capabilities=""
  fi
  if [[ "${capabilities}" == aterm-build-cache-register-v1 ]]; then
    mkdir -p "$(dirname "${target}")" || return 1
    # A compatible helper distinguishes a published/protected owner from an unmanaged
    # observation. Never accept the latter for an existing marked cache.
    for helper in "${target}/.aterm-build-cache.json" "${target}/build.noindex/.aterm-build-cache.json"; do
      if [[ -e "${helper}" || -L "${helper}" ]]; then
        marked=1
        marked_root="$(alab_real "$(dirname "${helper}")")" || return 75
        case "${marked_root}" in "${home}"/*) ;; *) return 75 ;; esac
      fi
    done
    [[ "${marked}" -eq 0 ]] || required=(--require-owned)
    if ! (cd "${workspace}" && HOME="${home}" "${registrar[@]}" __build_cache_register \
      ${required[@]+"${required[@]}"} targo --unverified build \
      "--manifest-path=${workspace}/Cargo.toml" "--target-dir=${target}") >/dev/null; then
      alab_say "cache owner publication failed for ${target}; no build started"
      return 75
    fi
  else
    for helper in "${target}/.aterm-build-cache.json" "${target}/build.noindex/.aterm-build-cache.json"; do
      [[ ! -e "${helper}" && ! -L "${helper}" ]] || marked=1
    done
    if [[ "${marked}" -ne 0 ]]; then
      alab_say "the managed cache ${target} needs a capable aterm cache registrar; no build started"
      return 75
    fi
  fi
  mkdir -p "${target}" || return 1
  printf '%s\n' "${target}"
}

# The dedicated lint location used by both the gate and its outside preparation.
alab_check_lint_target() { printf '%s/tippy\n' "$1"; }

# Resolve both locked dependency graphs before any compiler. Manifest-only
# metadata misses the pristine registry/Git closure used by later attribution.
# Offline resolution cannot fetch; keep every error and check both trees.
alab_check_dependencies_ready() {
  local tree cargo_home status=0 code cargo="${ALAB_CARGO:-targo}" source_tree="${1:-}" config
  local config_args=()
  [[ -n "${source_tree}" ]] || return 0
  config="$(alab_pristine_config "${source_tree}")" || return $?
  cargo_home="$(alab_cargo_home)" || return $?
  for tree in "$@"; do
    config_args=()
    if [[ "${tree}" != "${source_tree}" && -n "${config}" ]]; then
      config_args=(--config "${config}")
    fi
    if (cd "${tree}/codex-rs" && CARGO_HOME="${cargo_home}" CARGO_NET_OFFLINE=true \
      alab_build_command "${cargo}" metadata --offline --locked ${config_args[@]+"${config_args[@]}"} --format-version 1 >/dev/null); then
      :
    else
      code=$?
      alab_say "locked offline dependency graph unavailable for ${tree}; no compiler started"
      [[ "${code}" -ne 75 && "${code}" -le 128 ]] || return "${code}"
      [[ "${status}" -ne 0 ]] || status="${code}"
    fi
  done
  return "${status}"
}

# alab_check_targets_ready <tree> <pristine-tree> <target>: declare every
# gate producer/target pair before entering containment or starting a compiler.
# The pristine build target also hosts ALab's attribution runner. Keep this
# inventory shared by the canonical sync and callers running its frozen gate.
alab_check_targets_ready() {
  local tree="$1" pristine="$2" target="$3" build_target pristine_target
  build_target="$(alab_build_target_ready "${tree}/codex-rs" "${target}")" || return $?
  pristine_target="$(alab_build_target_ready "${pristine}/codex-rs" "${pristine}/codex-rs/target")" || return $?
  alab_build_target_ready "${tree}/codex-rs" "${pristine_target}" >/dev/null || return $?
  alab_build_target_ready "${tree}/codex-rs" "$(alab_check_lint_target "${build_target}")" >/dev/null || return $?
  alab_build_target_ready "${pristine}/codex-rs" "$(alab_check_lint_target "${pristine_target}")" >/dev/null || return $?
  printf '%s\n' "${build_target}"
}

# Run one sync/check compiler command under the existing 20 GiB disk guardian.
# Each Cargo boundary owns its group, including unit runtime calls that used to
# detach through Perl. Preserve inherited descriptors, argv and environment; no
# source/cache ownership or deletion policy changes here.
alab_build_command() {
  local target common cargo_home trust_home private_home build alternate shared workspace manifest="" word next=0
  local extra=()
  workspace="$(pwd -P)" || return 75
  # The runner helper names ALab's workspace while building from pristine's
  # cwd. These producers name workspace manifests, never a member manifest.
  for word in "$@"; do
    if [[ "${next}" -eq 1 ]]; then manifest="${word}"; break; fi
    case "${word}" in
      --) break ;;
      --manifest-path) next=1 ;;
      --manifest-path=*) manifest="${word#--manifest-path=}"; break ;;
    esac
  done
  if [[ -n "${manifest}" ]]; then
    manifest="$(alab_real "${manifest}")" || return 75
    if ! grep -Eq '^[[:space:]]*\[workspace\][[:space:]]*(#.*)?$' "${manifest}"; then
      alab_say "disk resource stop: cannot establish the workspace root of ${manifest}; no command started"
      return 75
    fi
    workspace="$(dirname "${manifest}")"
  fi
  target="${CARGO_TARGET_DIR:-${CARGO_BUILD_TARGET_DIR:-${workspace}/target}}"
  target="$(alab_real "${target}")" || return 1
  cargo_home="$(alab_real "${CARGO_HOME:-${HOME:?}/.cargo}")" || return 1
  trust_home="$(alab_real "${TARGO_HOME:-${CARGO_HOME:-${HOME:?}/.cargo}}")" || return 1
  if [[ -n "${CARGO_BUILD_BUILD_DIR:-}" ]]; then
    build="${CARGO_BUILD_BUILD_DIR//\{workspace-root\}/${workspace}}"
    alternate="${build//\{cargo-cache-home\}/${trust_home}}"
    build="${build//\{cargo-cache-home\}/${cargo_home}}"
    if [[ "${build}" == *'{'* || "${build}" == *'}'* ]]; then
      alab_say "disk resource stop: unresolved Cargo build directory ${build}; no command started"
      return 75
    fi
    build="$(alab_real "${build}")" || return 75
    extra+=(--disk-path "${build}")
    if [[ "${alternate}" != "${build}" ]]; then
      alternate="$(alab_real "${alternate}")" || return 75
      extra+=(--disk-path "${alternate}")
    fi
  elif [[ -z "${CARGO_TARGET_DIR:-}" && -z "${CARGO_BUILD_TARGET_DIR:-}" && "$(uname -s)" == Darwin ]]; then
    # The native macOS shim keeps intermediates here without moving Cargo's
    # target artifacts. Checking the root also follows a volume-mounted alias.
    shared="${HOME:?}/Library/Caches/aterm/.ubd/cargo-build.noindex"
    extra+=(--disk-path "${shared}")
  fi
  common="$(git rev-parse --path-format=absolute --git-common-dir)" || return 1
  private_home="$(alab_cargo_home)" || return 1
  "${ALAB_PYTHON:-/usr/bin/python3}" -I -B "${alab_library_root}/alab/command_guard.py" \
    --inherit-fds --resource-stop --min-free-gib 20 --disk-path "${target}" \
    --disk-path "${target}/build.noindex" --disk-path "${workspace}" --disk-path "$(pwd -P)" --disk-path "${common}" \
    --disk-path "${cargo_home}" --disk-path "${trust_home}" --disk-path "${private_home}" \
    --disk-path /tmp --disk-path "${TMPDIR:-/tmp}" ${extra[@]+"${extra[@]}"} -- "$@"
}

# Keep one package cache immutable through the command guardian's group stop.
# The guardian owns the BSD lock through cleanup; children inherit no lock descriptor.
# Canonicalize before locking so two symlink spellings still serialize together.
alab_package_cache_locked() {
  _alab_package_cache_guard run "$@"
}

# An executable native controller must itself become the cooperative guardian:
# a signal forwarded to its root PID must reach the owner of writer retirement.
alab_package_cache_exec() {
  _alab_package_cache_guard exec "$@"
}

_alab_package_cache_guard() {
  local mode="$1"
  shift
  local cache="$1" account_home home_root output repo cargo_home common
  local disk_args=() source_args=()
  shift
  # Only packaging opts in; other cache-guard clients retain their existing behavior.
  if [[ "${1:-}" == --build-disk ]]; then
    [[ $# -ge 4 ]] || alab_die "--build-disk needs output, repo and command"
    output="$2" repo="$3"
    shift 3
    [[ "${output}" == /* && "${repo}" == /* ]] || alab_die "build disk paths must be absolute"
    # Fetches and controller worktrees write Git metadata even with a separate Cargo home.
    common="$(git -C "${repo}" rev-parse --path-format=absolute --git-common-dir)" || return 1
    source_args=(--package-source-repo "${repo}")
    if [[ -n "${ALAB_CARGO_HOME:-}" ]]; then
      cargo_home="${ALAB_CARGO_HOME}"
    else
      cargo_home="${common}/alab-cargo-home"
    fi
    # The artifact child can resolve onto another volume beneath the output directory.
    disk_args=(--min-free-gib 20 --disk-path "${cache}" --disk-path "${output}" --disk-path "${output}/codex"
      --disk-path "${HOME:?}/Developer" --disk-path "${common}" --disk-path "${cargo_home}"
      --disk-path /tmp --disk-path "${TMPDIR:-/tmp}")
  fi
  [[ "${cache}" == /* ]] || alab_die "ALAB_PACKAGE_CACHE must be an absolute path, not '${cache}'"
  cache="$(alab_real "${cache}")" || return 1
  account_home="$(alab_account_home)" || return 1
  for home_root in "${HOME:?}" "${account_home}"; do
    if alab_home_or_above "${cache}" "${home_root}"; then
      alab_say "the package cache ${cache} must not be an account home or above it"
      return 1
    fi
  done
  mkdir -p "${cache}" || return 1
  if [[ ${#source_args[@]} -gt 0 ]]; then
    # Native maintenance owns deletion and takes the coordinator itself. Run it
    # before the guarded disk floor, never between the producer's Cargo phases.
    "${ALAB_PYTHON:-/usr/bin/python3}" -I -B "${alab_library_root}/alab/package_cache.py" \
      compact --cache "${cache}" >/dev/null \
      || alab_say "package cache maintenance unavailable; existing disk guard remains active"
  fi
  local guardian=(/usr/bin/env "ALAB_PACKAGE_CACHE=${cache}" "${ALAB_PYTHON:-/usr/bin/python3}" -I -B
    "${alab_library_root}/alab/command_guard.py" --cache-lock "${cache}/.alab-package.lockf")
  guardian+=(${disk_args[@]+"${disk_args[@]}"} ${source_args[@]+"${source_args[@]}"} -- "$@")
  if [[ "${mode}" == exec ]]; then
    exec "${guardian[@]}"
  fi
  "${guardian[@]}"
}

# alab_upstream_ready: the upstream remote and its refs exist (alab/setup.sh makes them).
alab_upstream_ready() {
  git remote get-url upstream >/dev/null 2>&1 \
    || alab_die "no 'upstream' remote here: run alab/setup.sh first"
}

# alab_real <path>: <path> with every symlink in it resolved, also when its last parts do not
# exist yet (the sandbox matches resolved paths: /var is /private/var).
alab_real() {
  local dir="${1:-}" rest="" base
  [[ -n "${dir}" ]] || return 1
  while [[ ! -d "${dir}" ]]; do
    rest="/${dir##*/}${rest}"
    dir="$(dirname "${dir}")" || return 1
    [[ -n "${dir}" ]] || return 1
  done
  base="$(cd "${dir}" && pwd -P)" || return 1
  [[ -n "${base}" ]] || return 1
  printf '%s%s\n' "${base}" "${rest}"
}

# alab_sb <path>: <path>, resolved, as a sandbox profile string.
alab_sb() {
  local s
  s="$(alab_real "$1")"
  s="${s//\\/\\\\}"
  printf '"%s"' "${s//\"/\\\"}"
}

# alab_cargo_home: the private cargo home contained commands build with (alab_contained):
# ALAB_CARGO_HOME, else <git-common-dir>/alab-cargo-home of the working directory's clone.
# Outside a git checkout, ALAB_CARGO_HOME must be set. Never the owner's ~/.cargo: every
# build the owner runs outside the sandbox compiles from that home's registry and git
# caches, so a build script that could write them could plant code that runs uncontained.
alab_cargo_home() {
  local d common
  if [[ -n "${ALAB_CARGO_HOME:-}" ]]; then
    d="${ALAB_CARGO_HOME}"
  elif common="$(git rev-parse --path-format=absolute --git-common-dir 2>/dev/null)"; then
    d="${common}/alab-cargo-home"
  else
    alab_say "alab_contained: $(pwd -P) is not in a git checkout, so there is no private cargo home: set ALAB_CARGO_HOME"
    return 1
  fi
  case "${d}" in /*) ;; *)
    alab_say "alab_contained: ALAB_CARGO_HOME must be an absolute path, not '${d}'"
    return 1
    ;;
  esac
  printf '%s\n' "${d}"
}

# alab_cargo_home_ready <dir>: make the private cargo home, outside the sandbox. The first
# time, it gets a copy of the owner's cargo config (~/.cargo/config.toml, or the older
# ~/.cargo/config) when there is one and it names no token or credential provider; it is
# never seeded again. The sandbox cannot write its config, so one contained step cannot
# change what the next one is configured with.
alab_cargo_home_ready() {
  local d="$1" own="${HOME:?}/.cargo" f
  mkdir -p "${d}" || return 1
  [[ ! -e "${d}/.alab-seeded" ]] || return 0
  for f in "${own}/config.toml" "${own}/config"; do
    [[ -f "${f}" && ! -L "${f}" ]] || continue
    if grep -q -i -E 'token|credential' "${f}"; then
      alab_say "not copying ${f} into ${d}: it names a token or a credential provider"
    else
      cp "${f}" "${d}/config.toml" || return 1
      alab_say "seeded ${d}/config.toml from ${f}"
    fi
    break
  done
  : >"${d}/.alab-seeded"
}

# alab_home_or_above <dir> [<home>]: <dir> is that home or a directory above it (so reading
# it would read all of the home).
alab_home_or_above() {
  local home d="$1"
  home="$(alab_real "${2:-${HOME:?}}")"
  [[ "${d}" == "${home}" || "${d}" == / || "${home}" == "${d}"/* ]]
}

# The account home is independent of HOME: a smoke or test may set HOME to a scratch
# directory, but that must not make this account's files readable. Query passwd with the
# selected interpreter, isolated from caller Python paths; refuse if it cannot name a home.
alab_account_home() {
  local home
  home="$("${ALAB_PYTHON:-/usr/bin/python3}" -I -c 'import os, pwd; print(pwd.getpwuid(os.getuid()).pw_dir)')" || {
    alab_say "alab_contained: cannot look up the current account home"
    return 1
  }
  case "${home}" in /*) ;; *)
    alab_say "alab_contained: the current account has no absolute home"
    return 1
    ;;
  esac
  [[ "${home}" != / ]] || {
    alab_say "alab_contained: refusing the current account's root home"
    return 1
  }
  alab_real "${home}"
}

# alab_contain_profile <scratch> <cargo-home> [--pristine <dir>] [--read <dir>]... <writable-dir>...: the
# sandbox profile alab_contained runs a command under. The last rule that matches an
# operation decides it.
alab_contain_profile() {
  local scratch="$1" cargo_home="$2" home="${HOME:?}" d entry account_home capacity
  shift 2
  account_home="$(alab_account_home)" || return 1
  # Native Rust admission shares one cooperative account ledger, independently
  # of a scratch HOME. Keep exactly that directory available for claims/locks;
  # an alias must never turn this exception into another readable/writable root.
  capacity="$(alab_real "${account_home}/Library/Caches/aterm/capacity")" || return 1
  if [[ "${capacity}" != "${account_home}/Library/Caches/aterm/capacity" ]] \
    || alab_home_or_above "${capacity}" "${account_home}"; then
    alab_say "alab_contained: refusing an aliased or unsafe native capacity directory"
    return 1
  fi
  local rustup_home="${RUSTUP_HOME:-${home}/.rustup}" common="" top="" pristine="" reads=() owner_homes
  local admitted=()
  owner_homes=("${home}/.cargo" "${account_home}/.cargo")
  [[ -z "${CARGO_HOME:-}" || "${CARGO_HOME}" == "${home}/.cargo" ]] || owner_homes+=("${CARGO_HOME}")
  while [[ $# -gt 0 && ( "$1" == --read || "$1" == --pristine ) ]]; do
    [[ $# -ge 2 && -n "$2" ]] || return 1
    if [[ "$1" == --pristine ]]; then
      pristine="$2"
    else
      reads+=("$2")
    fi
    shift 2
  done
  # Outside pre-registration published the owners. Use the existing read-only fence so even
  # a broadly admitted output ancestor cannot make the cleanup index writable.
  reads+=("${home}/Library/Caches/aterm/.ubd/registered-builds.noindex")
  # Outside a git checkout (a neutral directory, so no repo's .cargo/config.toml applies)
  # there is no clone to read, and nothing in the working directory is readable but what is
  # named (the directory itself is, for getcwd). A working directory, or a checkout, at
  # either home or above it is refused: reading it would read all of the home.
  if alab_home_or_above "$(pwd -P)" || alab_home_or_above "$(pwd -P)" "${account_home}"; then
    alab_say "alab_contained: refusing to run in $(pwd -P), which is a home or above it"
    return 1
  fi
  if common="$(git rev-parse --path-format=absolute --git-common-dir 2>/dev/null)"; then
    top="$(git rev-parse --show-toplevel)" || return 1
    if alab_home_or_above "$(alab_real "${top}")" || alab_home_or_above "$(alab_real "${common}")" \
      || alab_home_or_above "$(alab_real "${top}")" "${account_home}" \
      || alab_home_or_above "$(alab_real "${common}")" "${account_home}"; then
      alab_say "alab_contained: refusing the checkout ${top} (git dir ${common}): it is a home or above it"
      return 1
    fi
  else
    common=""
  fi
  echo '(version 1)'
  echo '(allow default)'
  # Nothing under either home is readable but what a build reads: the clone (its git dir, this
  # worktree and its pristine tree), the --read dirs, the toolchains, the tools on PATH,
  # the private cargo home and git's own config.
  echo "(deny file-read-data (subpath $(alab_sb "${home}")) (subpath $(alab_sb "${account_home}")))"
  echo '(allow file-read-data'
  if [[ -n "${common}" ]]; then
    for d in "${common}" "${top}"; do
      echo "  (subpath $(alab_sb "${d}"))"
      admitted+=("${d}")
    done
  else
    # The working directory itself, so getcwd() works: its listing, not its files.
    echo "  (literal $(alab_sb "$(pwd -P)"))"
  fi
  if [[ -n "${pristine}" ]]; then
    echo "  (subpath $(alab_sb "${pristine}"))"
    admitted+=("${pristine}")
  fi
  for d in ${reads[@]+"${reads[@]}"} "$@" "${capacity}" "${cargo_home}" "${rustup_home}" "${home}/.trust" \
    "${home}/.config/git"; do
    echo "  (subpath $(alab_sb "${d}"))"
    admitted+=("${d}")
  done
  for d in "${home}/.local/share/uv/python" "${account_home}/.local/share/uv/python"; do
    [[ -d "${d}" ]] || continue
    echo "  (subpath $(alab_sb "${d}"))"
    admitted+=("${d}")
  done
  echo "  (literal $(alab_sb "${home}/.gitconfig"))"
  # Object stores the clone borrows from (git clone --reference).
  if [[ -n "${common}" && -f "${common}/objects/info/alternates" ]]; then
    while IFS= read -r entry; do
      case "${entry}" in /*) echo "  (subpath $(alab_sb "${entry}"))" ;; esac
    done <"${common}/objects/info/alternates"
  fi
  for d in "${rustup_home}"/toolchains/*; do
    [[ -d "${d}" ]] || continue
    echo "  (subpath $(alab_sb "${d}"))"
    admitted+=("${d}")
  done
  # cargo reads the .cargo/config.toml of every directory above the build (a sync worktree in
  # a clone's git dir has the clone's above it; ~/.cargo's own is one of them); outside a
  # checkout, of the working directory and every one above it. Those files only, not the
  # rest of ~/.cargo.
  # The pristine tree's directories count too: it lives in the clone's git dir, which need
  # not be above this checkout (a worktree elsewhere). So do the cargo home's: `cargo install`
  # of a registry crate reads the config of the directories above its cargo home instead of
  # the working directory's (targo install.rs, reload_rooted_at(home)), and the clone's
  # private one sits in the clone's git dir.
  local starts=() root
  if [[ -n "${top}" ]]; then
    starts=("${top}")
  else
    starts=("$(pwd -P)/.")
  fi
  [[ -z "${pristine}" ]] || starts+=("${pristine}")
  starts+=("$(alab_real "${cargo_home}")/.")
  for d in "${starts[@]}"; do
    for root in "${home}" "${account_home}"; do
      entry="${d}"
      while [[ "${entry}" == "${root}"/* ]]; do
        entry="$(dirname "${entry}")"
        echo "  (literal $(alab_sb "${entry}/.cargo/config.toml")) (literal $(alab_sb "${entry}/.cargo/config"))"
      done
    done
  done
  # PATH's entries under HOME, and the atpkg store its bin/ links into.
  local IFS=:
  for entry in ${PATH}; do
    case "${entry}" in "${home}"/*|"${account_home}"/*)
      [[ -d "${entry}" ]] || continue
      d="$(alab_real "${entry}")"
      if alab_home_or_above "${d}" || alab_home_or_above "${d}" "${account_home}"; then
        alab_say "alab_contained: refusing the PATH entry ${entry}, which reads a home or above it"
        return 1
      fi
      echo "  (subpath $(alab_sb "${d}"))"
      admitted+=("${d}")
      ;;
    esac
  done
  unset IFS
  if [[ -n "${ATPKG_BIN:-}" && -d "${ATPKG_BIN}" ]]; then
    echo "  (subpath $(alab_sb "$(dirname "${ATPKG_BIN}")/store"))"
    admitted+=("$(dirname "${ATPKG_BIN}")/store")
    if [[ -d "$(dirname "${ATPKG_BIN}")/abrew" ]]; then
      echo "  (subpath $(alab_sb "$(dirname "${ATPKG_BIN}")/abrew"))"
      admitted+=("$(dirname "${ATPKG_BIN}")/abrew")
    fi
  fi
  # The Trust sysroot PATH's trustc runs from. atpkg's targo and trustc are shims that exec
  # wherever atpkg linked the toolchain: the store above, or a Trust checkout's dev sysroot
  # (pkg link --if-checkout), which is in none of the roots above, and there trustc could not
  # load its driver library (librustc_driver: "file system sandbox blocked open()"). Asked
  # from /, so no rust-toolchain file in a checkout can name another; readable, never
  # writable, like every toolchain here.
  local sysroot
  if command -v trustc >/dev/null 2>&1 && sysroot="$(cd / && trustc --print sysroot 2>/dev/null)" \
    && [[ "${sysroot}" == /* && -d "${sysroot}" ]]; then
    d="$(alab_real "${sysroot}")"
    if alab_home_or_above "${d}" || alab_home_or_above "${d}" "${account_home}"; then
      alab_say "alab_contained: refusing the Trust sysroot ${sysroot}, which reads a home or above it"
      return 1
    fi
    echo "  (subpath $(alab_sb "${d}"))"
    admitted+=("${d}")
  fi
  # targo authenticates a protected subcommand (tippy) by opening every directory above the
  # toolchain it selected, on the path it launched from and the canonical one. Each directory
  # above an admitted one, up to and including the home, is readable as a directory (its
  # listing), never as the files it holds.
  for d in ${admitted[@]+"${admitted[@]}"}; do
    d="$(alab_real "${d}")"
    for root in "${home}" "${account_home}"; do
      root="$(alab_real "${root}")"
      entry="${d}"
      while [[ "${entry}" == "${root}"/* ]]; do
        entry="$(dirname "${entry}")"
        echo "  (literal $(alab_sb "${entry}"))"
      done
    done
  done
  echo ')'
  echo '(deny file-read-data'
  for d in "${cargo_home}" "${owner_homes[@]}"; do
    echo "  (literal $(alab_sb "${d}/credentials")) (literal $(alab_sb "${d}/credentials.toml"))"
  done
  echo "  (literal $(alab_sb "${home}/.config/git/credentials"))"
  echo "  (literal $(alab_sb "${account_home}/.config/git/credentials")))"
  # Writes: build outputs, scratch, private Cargo downloads and the native
  # cooperative capacity protocol. Never source or the cargo home's config.
  echo '(deny file-write*)'
  echo '(allow file-write* (subpath "/dev")'
  for d in "${scratch}" "$@" "${cargo_home}" "${capacity}"; do
    echo "  (subpath $(alab_sb "${d}"))"
  done
  echo ')'
  # Nor the config `cargo install` reads above the home (<home>/.cargo/config.toml): denied as a
  # directory, so a contained step can neither write one nor make the directory to hold it.
  echo "(deny file-write* (literal $(alab_sb "${cargo_home}/config.toml")) (literal $(alab_sb "${cargo_home}/config"))"
  echo "  (subpath $(alab_sb "${cargo_home}/.cargo"))"
  echo "  (literal $(alab_sb "${cargo_home}/.alab-seeded")))"
  # A --read dir stays read-only, also inside a writable one.
  if [[ ${#reads[@]} -gt 0 ]]; then
    echo '(deny file-write*'
    for d in "${reads[@]}"; do
      echo "  (subpath $(alab_sb "${d}"))"
    done
    echo ')'
  fi
  # No Unix socket but its own and DNS's: not the SSH agent's, nor aterm's.
  echo '(deny network-outbound (remote unix-socket))'
  echo "(allow network-outbound (remote unix-socket (subpath $(alab_sb "${scratch}"))) (remote unix-socket (path-literal \"/private/var/run/mDNSResponder\")))"
  # The keychain, the clipboard, Launch Services and Apple events; launchctl, open, osascript.
  echo '(deny mach-lookup (global-name "com.apple.SecurityServer") (global-name "com.apple.securityd.xpc")'
  echo '  (global-name "com.apple.pasteboard.1") (global-name "com.apple.coreservices.launchservicesd"))'
  # Preferences IPC and its shared cache can read outside the filesystem roots. The
  # upstream Seatbelt preferences policy names these channels; contained builds need none.
  echo '(deny mach-lookup (global-name "com.apple.cfprefsd.daemon") (global-name "com.apple.cfprefsd.agent")'
  echo '  (local-name "com.apple.cfprefsd.agent"))'
  echo '(deny ipc-posix-shm-read* (ipc-posix-name-prefix "apple.cfprefs.") (ipc-posix-name-prefix "/apple.cfprefs."))'
  echo '(deny user-preference-read)'
  echo '(deny appleevent-send)'
  echo '(deny process-exec (literal "/bin/launchctl") (literal "/usr/bin/open") (literal "/usr/bin/osascript") (literal "/usr/bin/security"))'
}

# Admit only an actual directory in the namespace trusted mktemp allocates for
# contained commands. Its successful creation establishes exclusivity; this is
# not an identity lock against another process changing the path afterward.
alab_contained_scratch_path() {
  local path="${1:-}" parent root
  [[ "${path}" == /* && -d "${path}" && ! -L "${path}" ]] || return 1
  [[ "${path##*/}" =~ ^alab\.[A-Za-z0-9]{6}$ ]] || return 1
  parent="$(alab_real "${path%/*}")" || return 1
  root="$(alab_real /tmp)" || return 1
  [[ "${parent}" == "${root}" ]]
}

# The driving clone owns inner TMPDIR too, even for a neutral cargo-install cwd.
alab_contained_scratch_cleanup() {
  if ! alab_contained_scratch_path "${1:-}"; then
    alab_say "kept contained scratch ${1:-}: not an admitted scratch directory"
    return 0
  fi
  if "${ALAB_PYTHON:-/usr/bin/python3}" -I "${alab_library_root}/alab/retention.py" \
    --repo "${alab_library_root}"; then
    rm -rf "${1:?}"
  else
    alab_say "kept contained scratch ${1}: cleanup policy preserves builds"
  fi
}

# macOS refuses to exec its set-id ps from a sandbox. An ordinary signed copy
# still observes the caller's own children, without lifting containment or keeping
# privilege bits. Prove its first exec before this scratch path can be removed.
alab_contained_ps() {
  local tools="$1" signature observed
  mkdir -p "${tools}" || return 1
  cp /bin/ps "${tools}/ps" || return 1
  chmod 0755 "${tools}/ps" || return 1
  if ! signature="$(/usr/bin/codesign --force --sign - "${tools}/ps" 2>&1)"; then
    alab_say "cannot sign contained ps: ${signature}"
    return 1
  fi
  observed="$(/usr/bin/env -i PATH=/usr/bin:/bin "${tools}/ps" -o pid= -p "$$")" || return 1
  [[ "${observed//[[:space:]]/}" == "$$" ]]
}

# alab_contained [--pristine <dir>] [--read <dir>]... <writable-dir>... -- <command...>: run <command>, which builds or runs
# upstream's code, under a macOS sandbox, so a build script or test of a crate upstream just
# added cannot use what this account holds. <command> gets a scrubbed environment (no SSH
# agent, no tokens; a scratch TMPDIR) and cannot read files under HOME or the account home
# other than the clone, the Rust toolchains (the sysroot PATH's trustc runs from among them),
# PATH's tools, the private cargo home (not its credentials), the .cargo/config.toml of the
# directories above it and git's config; read
# preferences, the keychain or the clipboard; open apps, send Apple events or run launchctl;
# connect to a Unix socket but its
# own (not the SSH agent's, nor aterm's); or write anywhere but the <writable-dir>s (the
# target dirs: so no source, .git, .cargo or rust-toolchain file that git or cargo reads
# outside the sandbox), its scratch dir and the private cargo home (alab_cargo_home: its
# CARGO_HOME and TARGO_HOME), never ~/.cargo or ~/.trust/cache, whose caches the owner's own
# builds read. A --read <dir> (a file too) is readable and
# never writable, even inside a <writable-dir>: an input the command needs that it must not
# change, such as a binary a later step ships. Outside a git checkout nothing of the working
# directory is readable but what is named; a working directory at HOME or above it is
# refused. It keeps the network: builds fetch crates and V8. What it does not stop: a
# process it starts that outlives it, still sandboxed, and keeps writing what it could
# write (so nothing outside the sandbox writes into, or runs from, a directory a contained
# step was given); and a program that asks launchd itself to start a job, which then runs
# outside it. ALab-written commands (alab/*.sh) may run in it: they need no write to git.
# ALAB_CONTAIN=0 runs <command> uncontained, with the caller's own cargo home, and so does a
# system without sandbox-exec (Linux: it says so).
alab_contained() {
  local reads=() dirs=() scratch profile rc=0 v cargo_home own account_home home_root
  local command_path="${PATH}" tools
  while [[ $# -gt 0 && "$1" != -- ]]; do
    if [[ "$1" == --read || "$1" == --pristine ]]; then
      [[ $# -ge 2 && "$2" != -- ]] || alab_die "alab_contained: $1 needs a directory"
      reads+=("$1" "$2")
      shift
    else
      dirs+=("$1")
    fi
    shift
  done
  [[ "${1:-}" == -- && $# -ge 2 ]] \
    || alab_die "alab_contained: [--pristine <dir>] [--read <dir>]... <writable-dir>... -- <command...>"
  shift
  if [[ "${ALAB_CONTAIN:-1}" == 0 ]]; then
    "$@"
    return
  fi
  if [[ ! -x /usr/bin/sandbox-exec ]]; then
    alab_say "no sandbox-exec on this system: $1 runs uncontained"
    "$@"
    return
  fi
  cargo_home="$(alab_cargo_home)" || return 1
  account_home="$(alab_account_home)" || return 1
  cargo_home="$(alab_real "${cargo_home}")"
  for home_root in "${HOME}" "${account_home}"; do
    own="$(alab_real "${home_root}/.cargo")"
    if [[ "${cargo_home}" == "${own}" || "${cargo_home}" == "${own}"/* ]] \
      || alab_home_or_above "${cargo_home}" "${home_root}"; then
      alab_say "alab_contained: the private cargo home ${cargo_home} must not be ${own}, inside it, or a home"
      return 1
    fi
  done
  alab_cargo_home_ready "${cargo_home}" || {
    alab_say "alab_contained: cannot make the private cargo home ${cargo_home}"
    return 1
  }
  # The scratch dir is the command's TMPDIR and the one place it may bind a Unix socket, so it
  # is short: under a launchd or a nested TMPDIR, a test's socket path ran past macOS's
  # 104-byte sun_path ("path must be shorter than SUN_LEN"). mktemp makes it private (0700).
  scratch="$(mktemp -d /tmp/alab.XXXXXX)" || {
    alab_say "alab_contained: cannot allocate contained scratch"
    return 1
  }
  alab_contained_scratch_path "${scratch}" || {
    alab_say "alab_contained: mktemp did not name an allocated scratch directory"
    return 1
  }
  scratch="$(alab_real "${scratch}")" || {
    alab_say "alab_contained: cannot resolve contained scratch; keeping it"
    return 1
  }
  alab_contained_scratch_path "${scratch}" || return 1
  if [[ -u /bin/ps || -g /bin/ps ]]; then
    tools="${scratch}/tools"
    if ! alab_contained_ps "${tools}"; then
      alab_say "cannot prepare contained process inspection; kept scratch ${scratch}"
      return 1
    fi
    reads+=(--read "${tools}")
    command_path="${tools}:${command_path}"
  fi
  profile="$(alab_contain_profile "${scratch}" "${cargo_home}" ${reads[@]+"${reads[@]}"} ${dirs[@]+"${dirs[@]}"})" || {
    alab_contained_scratch_cleanup "${scratch}"
    return 1
  }
  # cargo fetches git dependencies with git itself: libgit2's TLS cannot check a certificate
  # without the keychain service the sandbox denies, and git's can.
  local env_args=("HOME=${HOME}" "PATH=${command_path}" "TMPDIR=${scratch}" "CARGO_HOME=${cargo_home}"
    "TARGO_HOME=${cargo_home}" "CARGO_NET_GIT_FETCH_WITH_CLI=true")
  # Preserve the caller's allocated compile/test concurrency across the scrub.
  for v in USER LOGNAME SHELL TERM LANG LC_ALL LC_CTYPE NO_COLOR RUSTUP_HOME CARGO_BUILD_JOBS RUST_TEST_THREADS \
    CARGO_TARGET_DIR CARGO_INCREMENTAL CARGO_PROFILE_DEV_DEBUG CARGO_NET_OFFLINE CODEX_HOME RUST_MIN_STACK RUST_BACKTRACE ATPKG_BIN \
    ATPKG_PROJECT_FRONT ATPKG_PROJECT_TARGO ATPKG_PROJECT_TRUSTC ATPKG_PROJECT_TRUSTDOC ATPKG_PROJECT_PAYLOAD ATPKG_PROJECT_EXECUTION \
    $(compgen -e | grep '^ALAB_' || true); do
    [[ -z "${!v+set}" ]] || env_args+=("${v}=${!v}")
  done
  env -i "${env_args[@]}" /usr/bin/sandbox-exec -p "${profile}" "$@" || rc=$?
  alab_contained_scratch_cleanup "${scratch}"
  return "${rc}"
}

# Native project compiler selection and explicit admission, shared by gate and package.
alab_project_run() (
  local repo="$1" source="$2" native cargo_home key
  shift 2
  # Selection and its controller use this repository, not inherited Git redirects.
  while IFS= read -r key; do
    case "${key}" in GIT_*) unset "${key}" ;; esac
  done < <(compgen -e)
  export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 GIT_OPTIONAL_LOCKS=0
  export GIT_NO_REPLACE_OBJECTS=1 GIT_NO_LAZY_FETCH=1 GIT_TERMINAL_PROMPT=0
  cargo_home="$(cd "${repo}" && alab_cargo_home)" || return $?
  native="${ALAB_ATPKG_PUBLISH:-atpkg-publish}"
  native="$(command -v "${native}")" || {
    alab_say "a qualified atpkg-publish is required (ALAB_ATPKG_PUBLISH)"
    return 1
  }
  case "${native}" in /*) ;; *)
    alab_say "atpkg-publish must resolve to an absolute executable"
    return 1 ;;
  esac
  if [[ -n "${source}" ]]; then
    CARGO_HOME="${cargo_home}" TARGO_HOME="${cargo_home}" \
      "${native}" toolchain run --repo "${repo}" --source "${source}" -- "$@"
  else
    CARGO_HOME="${cargo_home}" TARGO_HOME="${cargo_home}" \
      "${native}" toolchain run --repo "${repo}" -- "$@"
  fi
)

# Called only by a controller the native runner started. These fields carry the
# already selected tools; they are not a replacement for native authentication.
alab_project_selected() {
  local key value
  [[ "${ALAB_CONTAIN:-1}" != 0 ]] || {
    alab_say "native project qualification requires the protected profile"
    return 1
  }
  for key in ATPKG_PROJECT_FRONT ATPKG_PROJECT_TARGO ATPKG_PROJECT_TRUSTC ATPKG_PROJECT_TRUSTDOC; do
    value="${!key:-}"
    [[ "${value}" == /* && -x "${value}" ]] || {
      alab_say "missing native project tool ${key}"
      return 1
    }
  done
  for key in ATPKG_PROJECT_PAYLOAD ATPKG_PROJECT_EXECUTION; do
    value="${!key:-}"
    [[ "${value}" == /* && -d "${value}" ]] || {
      alab_say "missing native project root ${key}"
      return 1
    }
  done
  export ALAB_CARGO="${alab_library_root}/alab/project-targo.sh"
}

# Resolve only local metadata through the existing isolated Git reader. No
# compiler is queried and no source/cache tree is written.
alab_project_source() {
  "${ALAB_PYTHON:-/usr/bin/python3}" -I -B -c '
import re, sys
sys.path.insert(0, sys.argv[1] + "/alab")
from github import git
value = git(sys.argv[1], "rev-parse", "--verify", sys.argv[2] + "^{commit}").decode("ascii").removesuffix("\n")
if not re.fullmatch("[0-9a-f]{40}", value):
    raise SystemExit("local source resolution returned no full commit")
print(value)
' "$1" "$2"
}
