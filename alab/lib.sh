# shellcheck shell=bash
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
#
# Shared by alab/*.sh. Kept to bash 3.2: macOS's /bin/bash runs these.

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

# alab_pristine <checkout>: that checkout's pristine upstream tree, a worktree inside its own
# git dir. alab/check.sh and alab/sync.sh build and test upstream alone there, in the tree's
# own codex-rs/target: cargo judges a workspace crate fresh by file times, so a target shared
# with the ALab tree could hand either tree the other's builds. One per checkout, so a check
# in one checkout never moves the tree under another's.
alab_pristine() { git -C "$1" rev-parse --path-format=absolute --git-path alab-pristine; }

# alab_pristine_at <checkout> <rev>: check that tree out at <rev>. It is disposable: one that
# cannot be checked out (deleted or pruned by hand, a stale lock) is made again. A run killed
# inside `git worktree add` leaves the tree registered and locked ("initializing"), which
# prune keeps and a plain add refuses: -f -f replaces that registration, and any other stale
# one at that path, without pruning a person's worktrees (one on an unmounted disk, say).
# A tree already at <rev> and unmodified is left alone, so a call from alab/check.sh run
# contained (alab_contained: git cannot write there) succeeds once alab/sync.sh has put it there.
alab_pristine_at() {
  local dir want
  dir="$(alab_pristine "$1")" || return 1
  want="$(git -C "$1" rev-parse -q --verify "$2^{commit}")" || return 1
  if [[ -e "${dir}/.git" && "$(git -C "${dir}" rev-parse -q --verify HEAD 2>/dev/null)" == "${want}" ]] \
    && git -C "${dir}" diff --quiet HEAD -- 2>/dev/null; then
    return 0
  fi
  { [[ -e "${dir}/.git" ]] && git -C "${dir}" checkout --quiet --force --detach "${want}" 2>/dev/null; } || {
    rm -rf "${dir:?}"
    git -C "$1" worktree add --quiet --force --force --detach "${dir}" "${want}"
  }
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

# alab_upstream_ready: the upstream remote and its refs exist (alab/setup.sh makes them).
alab_upstream_ready() {
  git remote get-url upstream >/dev/null 2>&1 \
    || alab_die "no 'upstream' remote here: run alab/setup.sh first"
}

# alab_real <path>: <path> with every symlink in it resolved, also when its last parts do not
# exist yet (the sandbox matches resolved paths: /var is /private/var).
alab_real() {
  local dir="$1" rest=""
  while [[ ! -d "${dir}" ]]; do
    rest="/${dir##*/}${rest}"
    dir="$(dirname "${dir}")"
  done
  printf '%s%s\n' "$(cd "${dir}" && pwd -P)" "${rest}"
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

# alab_home_or_above <dir>: <dir> is HOME or a directory above it (so reading it would read
# all of HOME).
alab_home_or_above() {
  local home d="$1"
  home="$(alab_real "${HOME:?}")"
  [[ "${d}" == "${home}" || "${d}" == / || "${home}" == "${d}"/* ]]
}

# alab_contain_profile <scratch> <cargo-home> [--read <dir>]... <writable-dir>...: the
# sandbox profile alab_contained runs a command under. The last rule that matches an
# operation decides it.
alab_contain_profile() {
  local scratch="$1" cargo_home="$2" home="${HOME:?}" d entry
  shift 2
  local rustup_home="${RUSTUP_HOME:-${home}/.rustup}" common="" top="" reads=() owner_homes
  owner_homes=("${home}/.cargo")
  [[ -z "${CARGO_HOME:-}" || "${CARGO_HOME}" == "${home}/.cargo" ]] || owner_homes+=("${CARGO_HOME}")
  while [[ $# -gt 0 && "$1" == --read ]]; do
    reads+=("$2")
    shift 2
  done
  # Outside a git checkout (a neutral directory, so no repo's .cargo/config.toml applies)
  # there is no clone to read, and nothing in the working directory is readable but what is
  # named (the directory itself is, for getcwd). A working directory, or a checkout, at HOME
  # or above it is refused: reading it would read all of HOME.
  if alab_home_or_above "$(pwd -P)"; then
    alab_say "alab_contained: refusing to run in $(pwd -P), which is HOME or above it"
    return 1
  fi
  if common="$(git rev-parse --path-format=absolute --git-common-dir 2>/dev/null)"; then
    top="$(git rev-parse --show-toplevel)" || return 1
    if alab_home_or_above "$(alab_real "${top}")" || alab_home_or_above "$(alab_real "${common}")"; then
      alab_say "alab_contained: refusing the checkout ${top} (git dir ${common}): it is HOME or above it"
      return 1
    fi
  else
    common=""
  fi
  echo '(version 1)'
  echo '(allow default)'
  # Nothing under HOME is readable but what a build reads: the clone (its git dir, this
  # worktree and its pristine tree), the --read dirs, the toolchains, the tools on PATH,
  # the private cargo home and git's own config.
  echo "(deny file-read-data (subpath $(alab_sb "${home}")))"
  echo '(allow file-read-data'
  if [[ -n "${common}" ]]; then
    for d in "${common}" "${top}" "$(alab_pristine "${top}")"; do
      echo "  (subpath $(alab_sb "${d}"))"
    done
  else
    # The working directory itself, so getcwd() works: its listing, not its files.
    echo "  (literal $(alab_sb "$(pwd -P)"))"
  fi
  for d in ${reads[@]+"${reads[@]}"} "$@" "${cargo_home}" "${rustup_home}" "${home}/.trust" \
    "${home}/.config/git"; do
    echo "  (subpath $(alab_sb "${d}"))"
  done
  echo "  (literal $(alab_sb "${home}/.gitconfig"))"
  # Object stores the clone borrows from (git clone --reference).
  if [[ -n "${common}" && -f "${common}/objects/info/alternates" ]]; then
    while IFS= read -r entry; do
      case "${entry}" in /*) echo "  (subpath $(alab_sb "${entry}"))" ;; esac
    done <"${common}/objects/info/alternates"
  fi
  for d in "${rustup_home}"/toolchains/*; do
    [[ -d "${d}" ]] && echo "  (subpath $(alab_sb "${d}"))"
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
  local starts=()
  if [[ -n "${top}" ]]; then
    starts=("${top}" "$(alab_pristine "${top}")")
  else
    starts=("$(pwd -P)/.")
  fi
  starts+=("$(alab_real "${cargo_home}")/.")
  for d in "${starts[@]}"; do
    while [[ "${d}" == "${home}"/* ]]; do
      d="$(dirname "${d}")"
      echo "  (literal $(alab_sb "${d}/.cargo/config.toml")) (literal $(alab_sb "${d}/.cargo/config"))"
    done
  done
  # PATH's entries under HOME, and the atpkg store its bin/ links into.
  local IFS=:
  for entry in ${PATH}; do
    case "${entry}" in "${home}"/*) [[ -d "${entry}" ]] && echo "  (subpath $(alab_sb "${entry}"))" ;; esac
  done
  unset IFS
  [[ -z "${ATPKG_BIN:-}" || ! -d "${ATPKG_BIN}" ]] || echo "  (subpath $(alab_sb "$(dirname "${ATPKG_BIN}")/store"))"
  echo ')'
  echo '(deny file-read-data'
  for d in "${cargo_home}" "${owner_homes[@]}"; do
    echo "  (literal $(alab_sb "${d}/credentials")) (literal $(alab_sb "${d}/credentials.toml"))"
  done
  echo "  (literal $(alab_sb "${home}/.config/git/credentials")))"
  # Writes: the build's own directories, its scratch dir and the private cargo home (its
  # download caches), but never that home's config.
  echo '(deny file-write*)'
  echo '(allow file-write* (subpath "/dev")'
  for d in "${scratch}" "$@" "${cargo_home}"; do
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
  echo '(deny appleevent-send)'
  echo '(deny process-exec (literal "/bin/launchctl") (literal "/usr/bin/open") (literal "/usr/bin/osascript") (literal "/usr/bin/security"))'
}

# alab_contained [--read <dir>]... <writable-dir>... -- <command...>: run <command>, which builds or runs
# upstream's code, under a macOS sandbox, so a build script or test of a crate upstream just
# added cannot use what this account holds. <command> gets a scrubbed environment (no SSH
# agent, no tokens; a scratch TMPDIR) and cannot read files under HOME other than the clone,
# the Rust toolchains, PATH's tools, the private cargo home (not its credentials), the
# .cargo/config.toml of the directories above it and git's config; read the keychain or the
# clipboard; open apps, send Apple events or run launchctl; connect to a Unix socket but its
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
  local reads=() dirs=() scratch profile rc=0 v cargo_home own
  while [[ $# -gt 0 && "$1" != -- ]]; do
    if [[ "$1" == --read ]]; then
      [[ $# -ge 2 && "$2" != -- ]] || alab_die "alab_contained: --read needs a directory"
      reads+=(--read "$2")
      shift
    else
      dirs+=("$1")
    fi
    shift
  done
  [[ "${1:-}" == -- && $# -ge 2 ]] \
    || alab_die "alab_contained: [--read <dir>]... <writable-dir>... -- <command...>"
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
  alab_cargo_home_ready "${cargo_home}" || {
    alab_say "alab_contained: cannot make the private cargo home ${cargo_home}"
    return 1
  }
  cargo_home="$(alab_real "${cargo_home}")"
  own="$(alab_real "${HOME}/.cargo")"
  if [[ "${cargo_home}" == "${own}" || "${cargo_home}" == "${own}"/* ]] || alab_home_or_above "${cargo_home}"; then
    alab_say "alab_contained: the private cargo home ${cargo_home} must not be ~/.cargo, inside it, or HOME"
    return 1
  fi
  # The scratch dir is the command's TMPDIR and the one place it may bind a Unix socket, so it
  # is short: under a launchd or a nested TMPDIR, a test's socket path ran past macOS's
  # 104-byte sun_path ("path must be shorter than SUN_LEN"). mktemp makes it private (0700).
  scratch="$(alab_real "$(mktemp -d /tmp/alab.XXXXXX)")"
  profile="$(alab_contain_profile "${scratch}" "${cargo_home}" ${reads[@]+"${reads[@]}"} ${dirs[@]+"${dirs[@]}"})" || {
    rm -rf "${scratch:?}"
    return 1
  }
  # cargo fetches git dependencies with git itself: libgit2's TLS cannot check a certificate
  # without the keychain service the sandbox denies, and git's can.
  local env_args=("HOME=${HOME}" "PATH=${PATH}" "TMPDIR=${scratch}" "CARGO_HOME=${cargo_home}"
    "TARGO_HOME=${cargo_home}" "CARGO_NET_GIT_FETCH_WITH_CLI=true")
  for v in USER LOGNAME SHELL TERM LANG LC_ALL LC_CTYPE NO_COLOR RUSTUP_HOME \
    CARGO_TARGET_DIR CARGO_INCREMENTAL CARGO_PROFILE_DEV_DEBUG CODEX_HOME RUST_MIN_STACK RUST_BACKTRACE ATPKG_BIN \
    $(compgen -e | grep '^ALAB_' || true); do
    [[ -z "${!v+set}" ]] || env_args+=("${v}=${!v}")
  done
  env -i "${env_args[@]}" /usr/bin/sandbox-exec -p "${profile}" "$@" || rc=$?
  rm -rf "${scratch:?}"
  return "${rc}"
}
