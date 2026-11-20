#!/usr/bin/env bash
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
#
# alab/sync.sh — merge the newest openai/codex into codex-alab's main. Run it at least
# daily; alab/daily.sh is the unattended wrapper.
#
#   alab/sync.sh [--ref <upstream-rev>] [--stepwise] [--no-check] [--no-push]
#   alab/sync.sh --continue [--no-check] [--no-push]
#   alab/sync.sh --status | --abort
#
# It works in a dedicated worktree (ALAB_SYNC_DIR, default <git-common-dir>/alab-sync), so
# nobody's checkout moves and its codex-rs/target/ persists from one sync to the next:
#   1. fetch upstream and origin; stop if origin/main already contains the upstream rev
#   2. merge the rev into origin/main (--no-ff): rerere replays known resolutions, and when
#      both sides changed codex-rs/Cargo.lock upstream's side wins (alab/merge-attributes)
#   3. re-apply alab/removed-paths; keep ALab's side of alab/owned-paths (upstream's copy
#      lands in alab/upstream/); record upstream changes to alab/watched-paths
#   4. conflicts left: exit 3 and name them. A resolver fixes them in the worktree, stages
#      them (git add) and runs --continue
#   5. re-resolve codex-rs/Cargo.lock for ALab's crates, refusing if that moves any package
#      upstream locked; commit "alab sync: merge upstream openai/codex@<sha>"; run
#      alab/check.sh; push to origin main as a fast-forward. main is never rewritten
# Every step that runs upstream's code (the lock re-resolve and alab/check.sh, including its
# certified pristine attribution) runs contained (lib.sh, alab_contained), with
# the clone's private cargo home <git-common-dir>/alab-cargo-home (ALAB_CARGO_HOME names
# another), never ~/.cargo: the owner's own builds compile from that one's caches.
# --stepwise first lands, as one merge, a prefix of the upstream range that merges without a
# resolver (found by binary search), then merges up to the first upstream commit that needs
# one: main stays current while it waits, and the resolver meets one upstream change at a time.
# An unattended sync that stopped on an error (the network, a killed run) is resumed by the
# next run; one that stopped for a person (3, 4, 6), a person's --continue that failed, and a
# --no-push merge wait for --continue or --abort.
#
# Exit status: 0 landed, already current, or (--no-push) merged and checked (with --no-check,
# only merged); 3 conflicts
# await a resolver; 4 alab/check.sh failed, or a fix is left uncommitted (fix it in the
# worktree, then --continue); 5 an earlier sync still waits (for its resolver, or a --no-push
# merge to land); 6 pristine upstream certified but its exact binary also failed at runtime
# (a lane fault, fixed in Trust, not here); 75 a resource stop with no source verdict;
# 1, or a failing git command's own status (128),
# anything else.
set -euo pipefail

invoked_from="$(pwd)"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=alab/lib.sh
. "${repo_root}/alab/lib.sh"
cd "${repo_root}"

mode=start
ref=upstream/main
stepwise=0
check=1
push=1
while [[ $# -gt 0 ]]; do
  case "$1" in
    --ref)
      [[ $# -ge 2 ]] || alab_die "--ref needs a revision"
      ref="$2"
      shift
      ;;
    --stepwise) stepwise=1 ;;
    --continue) mode=continue ;;
    --abort) mode=abort ;;
    --status) mode=status ;;
    --no-check) check=0 ;;
    --no-push) push=0 ;;
    -h | --help)
      sed -n '5,/^set -euo pipefail$/p' "${repo_root}/alab/sync.sh" | sed -e '$d' -e 's/^# \{0,1\}//'
      exit 0
      ;;
    *) alab_die "unknown argument '$1' (see --help)" ;;
  esac
  shift
done
[[ "${check}" -eq 1 || "${push}" -eq 0 ]] \
  || alab_die "--no-check requires --no-push: every published sync must pass alab/check.sh"
[[ "${stepwise}" -eq 0 || "${push}" -eq 1 ]] \
  || alab_die "--stepwise lands its clean prefix before merging further, so it cannot run with --no-push"
alab_upstream_ready
[[ "${mode}" == status || "$(git config --bool rerere.enabled || true)" == true ]] \
  || alab_die "rerere is off in this clone, so no resolution would be remembered: run alab/setup.sh"

common_dir="$(git rev-parse --path-format=absolute --git-common-dir)"
wt="${ALAB_SYNC_DIR:-${common_dir}/alab-sync}"
# A relative ALAB_SYNC_DIR means relative to where the caller stood, not to the repo root.
case "${wt}" in /*) ;; *) wt="${invoked_from}/${wt}" ;; esac
# Every sync merge, and every --stepwise probe, merges with these: codex-rs/Cargo.lock is
# derived data, so when both sides changed it upstream's side wins and is re-resolved at
# commit time. Scoped to these commands; no other merge in the clone is affected. (A merge
# of main's concurrent commits keeps the sync's side instead: take_concurrent_main.)
export ALAB_SYNC_RECIPE_DRIVER="${repo_root}/alab/feature_recipes/manifest.py"
merge_cfg=(-c "core.attributesFile=${repo_root}/alab/merge-attributes"
  -c 'merge.alab-feature-recipes.driver="${ALAB_PYTHON:-/usr/bin/python3}" -IB "$ALAB_SYNC_RECIPE_DRIVER" %O %A %B %L'
  -c "merge.alab-upstream.name=upstream's side, re-resolved by alab/sync.sh"
  -c "merge.alab-upstream.driver=cp -f %B %A")

in_wt() { git -C "${wt}" "$@"; }
merging() {
  local rc=0
  in_wt rev-parse -q --verify MERGE_HEAD >/dev/null || rc=$?
  case "${rc}" in
    0) return 0 ;;
    1) return 1 ;;
    *) alab_die "cannot read sync merge state (git exit ${rc})" ;;
  esac
}
unmerged() { in_wt -c core.quotepath=off diff --name-only --diff-filter=U "$@"; }
# The sync worktree holds commits origin/main does not (a merge, fix-forward commits).
unlanded() {
  local commits
  commits="$(in_wt rev-list -1 origin/main..HEAD)" || alab_die "cannot read unlanded sync commits"
  [[ -n "${commits}" ]]
}
# grep -c, not grep -q: under pipefail an early-exiting grep -q SIGPIPEs a long writer and
# the pipeline then reports failure (141) exactly when the line was found.
has_line() { [[ "$(grep -cE "$1" || true)" != 0 ]]; }
# new_markers <diff args...>: the files to which that diff adds a conflict-marker line. A sync
# merge's check takes those that both sides lack: a marker-shaped line upstream ships (in a
# document or a test fixture) is content, not a resolution left half done.
new_markers() {
  in_wt -c core.quotepath=off diff --no-renames -U0 "$@" \
    | awk '/^\+\+\+ /{ f = substr($0, 5) } /^\+(<<<<<<<|>>>>>>>)( |$)/{ print f }' | sort -u
}
# Only compare complete marker sets; a failed diff or parser is no empty set.
shared_new_markers() {
  local left right
  if [[ "$1" == --cached ]]; then
    left="$(new_markers --cached "$2")" || return
    right="$(new_markers --cached "$3")" || return
  else
    left="$(new_markers "$1" "$3")" || return
    right="$(new_markers "$2" "$3")" || return
  fi
  comm -12 <([[ -z "${left}" ]] || printf '%s\n' "${left}") \
    <([[ -z "${right}" ]] || printf '%s\n' "${right}")
}

state_path() { in_wt rev-parse --path-format=absolute --git-path "alab-sync-$1"; }
# The upstream commit this sync merges (alab-sync-target); a sync without that record stops for a
# person rather than guessing.
sync_target() {
  local path target rc=0
  path="$(state_path target)" || alab_die "cannot read sync target: path lookup failed"
  target="$("${ALAB_PYTHON:-/usr/bin/python3}" -I -B -c '
import os
import sys
try:
    with open(sys.argv[1], "rb") as record:
        target = record.read().rstrip(b"\n")
except FileNotFoundError:
    if os.path.lexists(sys.argv[1]):
        raise SystemExit("dangling sync target record")
    sys.exit(3)  # Known absence retains the existing recovery diagnosis.
except OSError as error:
    raise SystemExit(str(error))
if not target or b"\n" in target or b"\0" in target:
    raise SystemExit("invalid sync target record")
sys.stdout.buffer.write(target)
' "${path}")" || rc=$?
  case "${rc}" in
    0) printf '%s\n' "${target}"; return 0 ;;
    3) ;; # Only known absence may produce lost-state recovery advice below.
    *) alab_die "cannot read sync target" ;;
  esac
  # No merge and no message either: the commits are not a sync's, and --abort would drop them.
  if ! merging && [[ ! -f "$(state_path msg)" ]]; then
    exit_with 3 "${wt} holds commits main lacks (git -C ${wt} log origin/main..HEAD), and no sync \
is in progress: land or move them yourself, then alab/sync.sh --abort"
  fi
  exit_with 3 "the sync in ${wt} lost its state file $(state_path target): alab/sync.sh --abort, then sync again"
}
clear_state() {
  local name path paths=()
  # Resolve the complete set first: a failed msg lookup must never become .final.
  for name in msg auto hand target stop; do
    path="$(state_path "${name}")" || alab_die "cannot locate sync recovery state"
    [[ "${path}" == /* ]] || alab_die "cannot locate sync recovery state: non-absolute path"
    paths+=("${path}")
    [[ "${name}" != msg ]] || paths+=("${path}.final")
  done
  rm -f -- "${paths[@]}"
}
# Replace a stop decision only after its complete record has been written.
write_stop() {
  local path temporary
  path="$(state_path stop)" || alab_die "cannot write sync stop record: path read failed"
  [[ "${path}" == /* ]] || alab_die "cannot write sync stop record: non-absolute path"
  [[ ! -d "${path}" ]] || alab_die "cannot write sync stop record: destination is a directory"
  temporary="$(mktemp "${path}.XXXXXX")" || alab_die "cannot write sync stop record: staging creation failed"
  if ! printf '%s\n' "$@" >"${temporary}"; then
    rm -f -- "${temporary}"
    alab_die "cannot write sync stop record: incomplete write"
  fi
  if ! mv -f -- "${temporary}" "${path}"; then
    rm -f -- "${temporary}"
    alab_die "cannot write sync stop record: replacement failed"
  fi
}

# stop <code> [<why>]: exit, recording a stop for a person (3, 4, 6) and why, for --status. A
# sync that stopped on an error instead (exit 1: the network, a missing tool) has no record,
# and the next run resumes it.
stop() {
  case "$1" in
    3 | 4 | 6) write_stop "$1" "${2:-}" ;;
  esac
  exit "$1"
}
# stop_code: the first line of the stop record (3, 4, 6, no-push, no-push-unchecked, continue).
stop_code() {
  local path
  path="$(state_path stop)" || return
  "${ALAB_PYTHON:-/usr/bin/python3}" -I -B -c '
import os
import sys
try:
    with open(sys.argv[1], "rb") as record:
        code = record.readline().rstrip(b"\n")
except FileNotFoundError:
    if os.path.lexists(sys.argv[1]):
        raise SystemExit("dangling sync stop record")
    sys.exit(0)
except OSError as error:
    raise SystemExit(str(error))
if code not in (b"3", b"4", b"6", b"no-push", b"no-push-unchecked", b"continue"):
    raise SystemExit("invalid sync stop record")
sys.stdout.buffer.write(code)
' "${path}"
}
exit_with() {
  local code="$1"
  shift
  printf 'alab: %s\n' "$*" >&2
  stop "${code}" "$*"
}
# specs <name>: the pathspecs in alab/<name> (removed-paths, owned-paths, watched-paths) as
# main has them; the sync worktree's copy can lag behind main between syncs.
specs() {
  local policy
  policy="$(git show "origin/main:alab/$1")" || return
  printf '%s\n' "${policy}" | alab_spec_lines
}
# How long the sync in the worktree has waited: since its merge began, or was committed.
waiting_hours() {
  local since
  since="$(date -r "$(state_path msg)" +%s 2>/dev/null)" || since="$(in_wt log -1 --format=%ct HEAD)"
  echo $((($(date +%s) - since) / 3600))
}

# Only one sync at a time per clone: a scheduled run and a person must not interleave. A
# background holder takes a kernel lock (lib.sh, alab_locked) and keeps it while this run
# lives: the lock is the holder's own, so no git or cargo process this run starts inherits it
# and keeps it past the run, and the holder lets go within seconds of this run dying.
take_lock() {
  local lock="${common_dir}/alab-sync.lockf" err="${common_dir}/alab-sync.lockf.err" got
  # The holder prints its pid once it has the lock, or "failed <status>" (75: another process
  # holds it); killing it releases the lock at once. A pipe, not a file, so a run killed while
  # it waits leaves nothing behind, and the holder's own messages go to ${err}, not to this
  # run's output, which it would otherwise keep open.
  exec 9< <(exec 2>/dev/null; alab_locked "${lock}" /bin/sh -c \
    'echo $$; exec >/dev/null; while kill -0 "$1" 2>/dev/null; do sleep 1; done' \
    alab-sync-lock "$$" 2>"${err}" || echo "failed $?")
  read -r got <&9 || true
  exec 9<&-
  case "${got}" in
    [0-9]*) lock_holder="${got}" ;;
    "failed 75") alab_die "another alab/sync.sh is running in this clone (it holds ${lock})" ;;
    *) alab_die "could not take ${lock} (${got:-no answer}): $(cat "${err}" 2>/dev/null)" ;;
  esac
  trap 'kill "${lock_holder}" 2>/dev/null || true' EXIT
}

# A configured path is usable only when Git and the checkout identify each other.
sync_worktree_state() {
  "${ALAB_PYTHON:-/usr/bin/python3}" -I -B "${repo_root}/alab/sync_worktree.py" "${common_dir}" "${wt}" "${1:-start}"
}

ensure_worktree() {
  local state
  state="$(sync_worktree_state)" || return $?
  if [[ "${state}" == absent ]]; then
    local add=(git worktree add --quiet --detach -- "${wt}" origin/main)
    # A checked sync writes both the configured checkout and its Git registration.
    if [[ "${check}" -eq 1 ]]; then
      add=("${ALAB_PYTHON:-/usr/bin/python3}" -I -B "${repo_root}/alab/command_guard.py"
           --inherit-fds --resource-stop --min-free-gib 20 --disk-path "${wt}" --disk-path "${common_dir}"
           -- "${add[@]}")
    fi
    "${add[@]}" || return $?
    state="$(sync_worktree_state)" || return $?
    [[ "${state}" == present ]] || alab_die "cannot verify new sync worktree at ${wt}"
    alab_say "sync worktree created at ${wt}"
  fi
}

# Status predicates return only known clean/dirty answers; read failures are fatal
# even inside Boolean contexts where shell errexit is disabled.
worktree_clean() {
  local porcelain
  porcelain="$(in_wt status --porcelain "$@")" || alab_die "cannot read sync worktree status"
  [[ -z "${porcelain}" ]]
}

# A killed Git writer can leave its index lock even when status calls the checkout clean.
# Never start a new sync over a recorded attempt with incomplete checkout state.
interrupted_checkout() {
  local target index
  target="$(state_path target)" || alab_die "cannot locate the recorded sync target"
  [[ -f "${target}" ]] || return 1
  index="$(in_wt rev-parse --path-format=absolute --git-path index)" || alab_die "cannot locate the sync index"
  [[ -e "${index}.lock" || -L "${index}.lock" ]] || ! GIT_OPTIONAL_LOCKS=0 worktree_clean --untracked-files=normal
}

worktree_fully_staged() {
  local porcelain
  porcelain="$(in_wt status --porcelain)" || alab_die "cannot read sync worktree status"
  porcelain="$(printf '%s\n' "${porcelain}" | awk 'length && $0 !~ /^[MADRCT] /')" \
    || alab_die "cannot classify sync worktree status"
  [[ -z "${porcelain}" ]]
}

apply_removed_paths() {
  local list=() line count policy
  policy="$(specs removed-paths)" || alab_die "cannot read alab/removed-paths from origin/main"
  while IFS= read -r line; do [[ -z "${line}" ]] || list+=("${line}"); done <<<"${policy}"
  [[ ${#list[@]} -gt 0 ]] || return 0
  count="$(in_wt ls-files -- "${list[@]}" | sort -u | wc -l | tr -d ' ')" \
    || alab_die "alab/removed-paths has an entry git rejects (above): fix it in an alab(removed) \
commit on main, then alab/sync.sh --abort and sync again"
  if [[ "${count}" -gt 0 ]]; then
    in_wt rm -r -q -f --ignore-unmatch -- "${list[@]}"
    echo "  removed-paths: dropped ${count} upstream file(s)" >>"$(state_path auto)"
  fi
}

# A successful tree listing distinguishes deletion from an unreadable object.
tree_has_path() {
  local entry
  entry="$(in_wt ls-tree --full-tree --name-only "$1" -- ":(literal)$2")" \
    || alab_die "cannot read owned path from $1"
  [[ -n "${entry}" ]]
}

# Publish a complete upstream copy without truncating the previous archive on read errors.
archive_owned_path() {
  local f="$1" archive directory temporary
  archive="${wt}/alab/upstream/${f}"
  directory="${archive%/*}"
  [[ ! -d "${archive}" ]] || alab_die "cannot archive owned path: destination is a directory"
  mkdir -p "${directory}" || alab_die "cannot archive owned path: directory creation failed"
  temporary="$(mktemp -d "${directory}/.alab-archive.XXXXXX")" || alab_die "cannot archive owned path: staging creation failed"
  if [[ -f "${archive}" && ! -L "${archive}" ]] && ! cp -p "${archive}" "${temporary}/content"; then
    rm -rf -- "${temporary}"
    alab_die "cannot archive owned path: mode preservation failed"
  fi
  if ! in_wt show "MERGE_HEAD:${f}" >"${temporary}/content"; then
    rm -rf -- "${temporary}"
    alab_die "cannot archive owned path: upstream read failed"
  fi
  if ! mv -f -- "${temporary}/content" "${archive}"; then
    rm -rf -- "${temporary}"
    alab_die "cannot archive owned path: replacement failed"
  fi
  rmdir "${temporary}" || alab_die "cannot archive owned path: staging cleanup failed"
}

# A conflict in a path ALab owns keeps ALab's side. Upstream's version is kept beside it in
# alab/upstream/<path>, so whoever maintains the ALab file can carry over what matters.
keep_owned_paths() {
  local list=() line f ours theirs conflicted policy ours_present theirs_present
  policy="$(specs owned-paths)" || alab_die "cannot read alab/owned-paths from origin/main"
  while IFS= read -r line; do [[ -z "${line}" ]] || list+=("${line}"); done <<<"${policy}"
  [[ ${#list[@]} -gt 0 ]] || return 0
  # Listed in full first: git diff refreshes the index under index.lock after it prints, so a
  # git add in the loop below, fed by it directly, can race it (exit 128, "index.lock exists").
  conflicted="$(unmerged -- "${list[@]}")" || alab_die "cannot read unresolved owned paths"
  while IFS= read -r f; do
    [[ -n "${f}" ]] || continue
    # Read both sides before mutating either the archive or the conflict.
    ours_present=0 theirs_present=0
    if tree_has_path HEAD "${f}"; then ours_present=1; fi
    if tree_has_path MERGE_HEAD "${f}"; then theirs_present=1; fi
    if [[ "${theirs_present}" -eq 1 ]]; then
      archive_owned_path "${f}"
      in_wt add -- "alab/upstream/${f}"
      theirs="upstream's is alab/upstream/${f}"
    else
      in_wt rm -q -f --ignore-unmatch -- "alab/upstream/${f}"
      theirs="upstream deleted it"
    fi
    if [[ "${ours_present}" -eq 1 ]]; then
      in_wt checkout --quiet --ours -- "${f}"
      in_wt add -- "${f}"
      ours="kept ALab's ${f}"
    else
      in_wt rm -q -f -- "${f}"
      ours="kept ALab's deletion of ${f}"
    fi
    echo "  owned-paths: ${ours}; ${theirs}" >>"$(state_path auto)"
  done <<<"${conflicted}"
}

# lock_packages: Cargo.lock on stdin -> one line per registry/git package (name, version,
# source, checksum). Workspace packages carry no source and are skipped, and so are
# [[patch.unused]] entries. Dependency lists are left out: an ALab crate that enables a
# feature, or adds a second version of a crate (Cargo then writes "foo 1.2.3" where it wrote
# "foo"), changes them without moving any package, and a moved dependency is its own entry.
lock_packages() {
  awk '
    function flush() {
      if (pkg && name != "" && source != "") print name, version, source, checksum
      name = version = source = checksum = ""
    }
    /^\[/ { flush(); pkg = ($0 == "[[package]]"); next }
    /^name = / { name = $3 }
    /^version = / { version = $3 }
    /^source = / { source = $3 }
    /^checksum = / { checksum = $3 }
    END { flush() }' | sort
}

# Re-resolve codex-rs/Cargo.lock for ALab's crates, then hold it to "upstream's packages
# exactly as upstream locked them": a resolver that moved one fails closed here. Contained
# (lib.sh, alab_contained), able to write only the lock and the target dir: it reads the
# .cargo/config.toml upstream just merged, which can name the rustc cargo runs.
reresolve_cargo_lock() {
  local lock="${wt}/codex-rs/Cargo.lock" rc=0 diff_rc=0
  [[ -f "${wt}/codex-rs/Cargo.toml" ]] || return 0
  [[ -f "${lock}" && ! -L "${lock}" ]] || alab_die "${lock} must be a regular file before lock resolution"
  # Lease the merged worktree's compiler before containment denies access to the store.
  alab_project_run "${wt}" "" /bin/bash -c \
    '. "$1/alab/lib.sh"; shift; alab_project_resolve_cargo_lock "$@"' \
    alab-native-resolver "${alab_library_root}" "${wt}/codex-rs" >/dev/null || rc=$?
  [[ "${rc}" -ne 75 ]] || exit_with 75 "lock resolution stopped: no lockfile verdict; see the resolver diagnostic"
  [[ "${rc}" -le 128 ]] || exit "${rc}"
  [[ "${rc}" -eq 0 ]] || alab_die "lock resolution failed in ${wt}; see the resolver diagnostic, then use --continue"
  [[ -f "${lock}" && ! -L "${lock}" ]] || alab_die "${lock} must be a regular file after lock resolution"
  in_wt diff --quiet -- codex-rs/Cargo.lock || diff_rc=$?
  case "${diff_rc}" in
    0) ;;
    1)
      in_wt add -- codex-rs/Cargo.lock
      echo "  codex-rs/Cargo.lock re-resolved" >>"$(state_path auto)"
      ;;
    *) alab_die "cannot read resolved lock changes (git exit ${diff_rc})" ;;
  esac
  drift_check
}

# The crates alab/vendor carries in place of upstream's (each one's ALAB-SOURCE.json), one
# line each in lock_packages' spelling minus the source: name, version, upstream's checksum.
vendored_packages() {
  local src
  for src in "${wt}"/alab/vendor/*/ALAB-SOURCE.json; do
    [[ -f "${src}" ]] || continue
    jq -r '"\"\(.crate)\" \"\(.version)\" \"\(.crate_sha256)\""' "${src}" \
      || alab_die "${src} names no crate, version and crate_sha256"
  done
}

# The lock being landed holds every package upstream locked at the sync's target, as upstream
# locked it, however the merge was committed. A crate alab/vendor carries (patched in by
# codex-rs/Cargo.toml) loses its registry row only when the copy is of the very version and
# archive upstream locked; an upstream bump past it is drift like any other.
drift_check() {
  local target own drift entry upstream_set merged_set
  [[ -f "${wt}/codex-rs/Cargo.toml" ]] || return 0
  target="$(sync_target)" || alab_die "cannot read sync target for lock comparison"
  entry="$(in_wt ls-tree --full-tree --name-only "${target}" -- codex-rs/Cargo.lock)" \
    || alab_die "cannot read upstream lock tree"
  [[ -n "${entry}" ]] || return 0
  own="$(vendored_packages)" || alab_die "cannot read vendored package identities"
  upstream_set="$(in_wt show "${target}:codex-rs/Cargo.lock" | lock_packages)" \
    || alab_die "cannot read upstream lock packages"
  merged_set="$(lock_packages <"${wt}/codex-rs/Cargo.lock")" \
    || alab_die "cannot read merged lock packages"
  drift="$(comm -23 <([[ -z "${upstream_set}" ]] || printf '%s\n' "${upstream_set}") \
    <([[ -z "${merged_set}" ]] || printf '%s\n' "${merged_set}") \
    | awk -v own="${own}" 'BEGIN { n = split(own, l, "\n"); for (i = 1; i <= n; i++) set[l[i]] }
      !(($1 " " $2 " " $4) in set)')" || alab_die "cannot read lock drift comparison"
  [[ -z "${drift}" ]] || exit_with 3 "codex-rs/Cargo.lock in ${wt} no longer holds these packages \
as upstream locked them:
$(printf '%s\n' "${drift}" | awk '{ gsub(/"/, ""); print "  " $1 " " $2 }')
Put each back (cd codex-rs && targo update -p <name>@<new> --precise <old>), git add \
codex-rs/Cargo.lock, then alab/sync.sh --continue"
}

# Upstream changes to paths ALab watches go into the merge message and the run's output.
note_watched_paths() {
  local base="$1" target="$2" list=() line changed policy
  policy="$(specs watched-paths)" || alab_die "cannot read alab/watched-paths from origin/main"
  while IFS= read -r line; do [[ -z "${line}" ]] || list+=("${line}"); done <<<"${policy}"
  [[ ${#list[@]} -gt 0 ]] || return 0
  changed="$(git -c core.quotepath=off diff --name-only "${base}" "${target}" -- "${list[@]}")"
  [[ -n "${changed}" ]] || return 0
  {
    echo "Upstream changed watched paths (alab/watched-paths):"
    printf '%s\n' "${changed}" | sed 's/^/  /'
  } | tee -a "$(state_path msg)" >&2
}

report_conflicts() {
  local target="$1" base f conflicts
  conflicts="$(unmerged)" || alab_die "cannot read unresolved paths"
  base="$(git merge-base origin/main "${target}")"
  {
    echo "conflicts for a resolver, in ${wt}:"
    while IFS= read -r f; do
      [[ -n "${f}" ]] || continue
      echo "  ${f}"
      case "${f}" in
        codex-rs/app-server-protocol/schema/precomputed/app-server-exports-*.json.zst)
          echo "      Regenerate from merged source, then compare before staging (alab/design/upstream-alignment.md):"
          echo "      python3 -IB alab/protocol_export_merge.py \"\$schema_scratch/precomputed/${f##*/}\""
          ;;
      esac
      git log --format='      upstream %h %s' -3 "${base}..${target}" -- "${f}"
      git log --no-merges --format='      alab     %h %s' -3 "${base}..origin/main" -- "${f}"
    done <<<"${conflicts}"
    echo "Keep both intents (alab/README.md, 'Resolving a conflict'), git add each file in"
    echo "the worktree, then: alab/sync.sh --continue"
  } >&2
}

# Commit the sync merge: re-apply removed-paths and re-resolve codex-rs/Cargo.lock first, and
# record what was resolved and how. "--amend" rewrites a merge the resolver committed by hand.
commit_sync_merge() {
  local msg auto hand
  apply_removed_paths
  reresolve_cargo_lock
  msg="$(state_path msg)"
  auto="$(state_path auto)"
  hand="$(state_path hand)"
  # A copy: a commit that fails (or a run killed after it) leaves msg as it was for the retry.
  cp "${msg}" "${msg}.final"
  if [[ -s "${auto}" ]]; then
    echo "Auto-resolved:" >>"${msg}.final"
    awk '!seen[$0]++' "${auto}" >>"${msg}.final"
  fi
  if [[ -s "${hand}" ]]; then
    echo "Resolved by hand:" >>"${msg}.final"
    sed 's/^/  /' "${hand}" >>"${msg}.final"
  fi
  in_wt commit --quiet --cleanup=strip "$@" -F "${msg}.final"
  rm -f "${msg}" "${msg}.final" "${auto}" "${hand}"
}

# The selected, contained gate has already certified and run pristine upstream
# before returning a source verdict. This boundary only maps that proven outcome;
# setup, resource and precompiler failures carry no lane verdict.
check_failed() {
  local rc="$1" target
  target="$(cat "$(state_path target)" 2>/dev/null || true)"
  if [[ "${rc}" -eq 6 ]]; then
    exit_with 6 "certified pristine upstream${target:+ ${target:0:9}} failed at runtime in alab/check.sh (above): a \
Trust lane fault, fixed in Trust, not in codex-alab. Once the toolchain is fixed: alab/sync.sh --continue"
  fi
  if [[ "${rc}" -eq 2 ]]; then
    alab_say "certified pristine upstream${target:+ ${target:0:9}} passed in alab/check.sh: ALab's layer broke the candidate"
  fi
  exit_with 4 "alab/check.sh failed in ${wt} (above). A wrong resolution is fixed in the merge \
itself (git rerere forget <file>, fix, git commit --amend); an adaptation to upstream's \
change is its own commit. Then alab/sync.sh --continue"
}

run_check() {
  local rc=0 check_python
  [[ "${check}" -eq 1 ]] || return 0
  alab_say "checking $(in_wt log -1 --format='%h %s') with alab/check.sh"
  # The recorder invokes the unchanged executable gate, which owns native
  # selection, targets and containment; its observation grants no execution lane.
  check_python="$(alab_python_311)" || alab_die "qualification recording needs Python 3.11+"
  "${check_python}" -I -B "${repo_root}/alab/feature_campaign.py" sync --repo "${wt}" || rc=$?
  # Killed by a signal (memory pressure, a person's Ctrl-C), not failed: no fault is known yet,
  # so no stop is recorded and the next run checks again. check.sh passes a killed cargo's
  # status up too.
  [[ "${rc}" -ne 75 ]] || exit_with 75 "resource stop during check: no source verdict; retry with alab/sync.sh --continue"
  if [[ "${rc}" -gt 128 ]]; then
    [[ "${mode}" == continue ]] && alab_die "alab/check.sh was killed (exit ${rc}): alab/sync.sh --continue checks again"
    alab_die "alab/check.sh was killed (exit ${rc}): the next alab/sync.sh checks again"
  fi
  [[ "${rc}" -ne 1 ]] || return 1
  [[ "${rc}" -eq 0 ]] || check_failed "${rc}"
}

# Someone landed ALab work on main while this sync ran: merge it into the sync (the sync
# merge stays on main's first-parent line), so the result is again a fast-forward of main.
take_concurrent_main() {
  local main="$1" main_upstream target own rc=0 head_file
  # Another clone's sync landed a newer upstream meanwhile: this merge is superseded, and its
  # lockfile (kept below) would roll back upstream's or fail the drift check.
  target="$(sync_target)"
  main_upstream="$(alab_last_sync_upstream "${main}")" || alab_die "cannot read main's last upstream sync"
  if [[ -n "${main_upstream}" ]] && ! alab_ancestor_check git merge-base --is-ancestor "${main_upstream}" "${target}"; then
    # A person's commits on the sync (a fix-forward after exit 4) would go with --abort.
    own="$(in_wt log --no-merges --format='  %h %s' "${main}..HEAD")"
    exit_with 3 "main merged upstream ${main_upstream:0:9} meanwhile, past this sync's: \
alab/sync.sh --abort, then sync again (rerere replays the resolutions)${own:+. --abort drops \
these commits in ${wt}; land on main first any it still needs:
${own}}"
  fi
  alab_say "origin/main moved while this sync ran; merging its new commits into the sync"
  # codex-rs/Cargo.lock is derived here too: keep the sync's (upstream's, re-resolved), then
  # re-resolve it for the concurrent commits' crates under the same drift check.
  write_in_wt "${merge_cfg[@]}" \
    -c "merge.alab-upstream.name=the sync's side, re-resolved by alab/sync.sh" \
    -c "merge.alab-upstream.driver=true" \
    merge --quiet --no-ff --no-commit "${main}" || rc=$?
  if [[ "${rc}" -ne 0 ]]; then
    # Git can fail before opening a merge. Only its conflict status and the exact
    # incoming commit together establish a resolver stop for an unattended run.
    if [[ "${rc}" -eq 1 ]]; then
      head_file="$(in_wt rev-parse --path-format=absolute --git-path MERGE_HEAD)" || alab_die "cannot locate incoming concurrent merge head"
      if cmp -s "${head_file}" <(printf '%s\n' "${main}"); then
        exit_with 3 "main's concurrent commits conflict with the sync in ${wt}: resolve, git add, \
then alab/sync.sh --continue"
      fi
    fi
    alab_say "merging main's concurrent commits stopped (git exit ${rc})"
    exit "${rc}"
  fi
  reresolve_cargo_lock
  in_wt commit --quiet -m "alab(sync): merge main's concurrent commits"
}

# A concurrent fetch can advance the shared tracking ref while the native gate
# compares hooks or records its inputs. Reconcile that forward change and run
# the whole gate on the new candidate. Keep its exact base for publication.
check_current_main() {
  local base after source rc
  base="$(in_wt rev-parse origin/main)" || alab_die "cannot read main before qualification"
  while :; do
    alab_ancestor_check in_wt merge-base --is-ancestor "${base}" HEAD || take_concurrent_main "${base}"
    drift_check
    source="$(in_wt rev-parse HEAD)" || alab_die "cannot read candidate before qualification"
    rc=0
    run_check || rc=$?
    after="$(in_wt rev-parse origin/main)" || alab_die "cannot read main after qualification"
    if [[ "${after}" == "${base}" ]]; then
      [[ "${rc}" -eq 0 ]] || check_failed "${rc}"
      checked_main="${base}"
      return 0
    fi
    alab_ancestor_check in_wt merge-base --is-ancestor "${base}" "${after}" \
      || alab_die "origin/main changed history during qualification"
    [[ "$(in_wt rev-parse HEAD)" == "${source}" ]] && worktree_clean \
      || exit_with 4 "candidate changed during qualification in ${wt}; inspect it before continuing"
    alab_say "main advanced during qualification; rechecking the candidate against ${after:0:9}"
    base="${after}"
  done
}

sync_fetch() {
  local tags=()
  [[ "$1" != upstream ]] || tags+=('+refs/tags/rust-v*:refs/upstream-tags/rust-v*')
  "${ALAB_PYTHON:-/usr/bin/python3}" -I -B "${repo_root}/alab/github.py" --repo "${repo_root}" \
    fetch "$1" "refs/heads/main:refs/remotes/$1/main" ${tags[@]+"${tags[@]}"}
}

sync_push() {
  "${ALAB_PYTHON:-/usr/bin/python3}" -I -B "${repo_root}/alab/github_push.py" \
    --repo "${wt}" --expected "$1" origin
}

push_main() {
  local before report rc delay=1
  while :; do
    before="${checked_main:?missing main revision from qualification}"
    rc=0
    report="$(sync_push "${before}")" || rc=$?
    [[ -z "${report}" ]] || printf '%s\n' "${report}"
    [[ "${rc}" -ne 0 ]] || return 0
    if [[ "${rc}" -eq 75 || "${rc}" -gt 128 ]]; then return "${rc}"; fi
    # A remote hook, authentication failure or transport error is not evidence of a race,
    # even if someone happened to advance main too. Git's per-ref porcelain distinguishes it.
    if ! awk -F '\t' '$1 == "!" && $2 == "HEAD:refs/heads/main" &&
      ($3 == "[rejected] (fetch first)" || $3 == "[rejected] (non-fast-forward)") { race = 1 }
      END { exit !race }' <<<"${report}"; then
      alab_die "push refused (above), not a proven concurrent update of main"
    fi
    sync_fetch origin || return $?
    [[ "$(git rev-parse origin/main)" != "${before}" ]] \
      || alab_die "push refused (above), but origin/main did not advance"
    alab_ancestor_check git merge-base --is-ancestor "${before}" origin/main \
      || alab_die "origin/main changed history during the push (above); a person must inspect it"
    if alab_ancestor_check in_wt merge-base --is-ancestor origin/main HEAD; then
      alab_die "push refused (above), though origin/main has nothing this sync lacks"
    fi
    alab_say "main advanced during the push; retrying its merge and check after ${delay}s"
    sleep "${delay}"
    check_current_main
    delay=$((delay < 30 ? delay * 2 : 60))
  done
}

# Commit the merge (if still open), gate it, land it.
finish() {
  local before sync_merge=0 target markers conflicts merge_head head_file head_rc=0 parent parent_rc=0
  merging || unlanded || alab_die "nothing to land in ${wt}: start a sync with alab/sync.sh"
  # A person's --continue that then dies on an error leaves the sync with that person; only an
  # unattended run that died is resumed by the next run.
  if [[ "${mode}" == continue ]]; then
    write_stop continue
  else
    rm -f "$(state_path stop)"
  fi
  target="$(sync_target)"
  if merging; then
    merge_head="$(in_wt rev-parse --verify MERGE_HEAD)" || alab_die "cannot read sync merge head"
    head_file="$(in_wt rev-parse --path-format=absolute --git-path MERGE_HEAD)" || alab_die "cannot locate sync merge heads"
    cmp -s "${head_file}" <(printf '%s\n' "${merge_head}") || head_rc=$?
    case "${head_rc}" in
      0) ;;
      1) exit_with 3 "the pending merge in ${wt} does not have exactly one incoming commit; inspect its MERGE_HEAD before continuing" ;;
      *) alab_die "cannot validate the complete sync merge head" ;;
    esac
    if [[ "${merge_head}" == "${target}" ]]; then
      # The upstream merge itself: redo what the sync resolves by itself, in case the run
      # that merged died before it did.
      [[ -f "$(state_path msg)" ]] || exit_with 3 "the sync in ${wt} lost its state file \
$(state_path msg): alab/sync.sh --abort, then sync again"
      sync_merge=1
    elif ! alab_ancestor_check in_wt merge-base --is-ancestor "${merge_head}" origin/main; then
      exit_with 3 "${wt} is merging ${merge_head:0:9}, neither the sync's \
upstream commit nor main's: git merge --abort there, then alab/sync.sh --continue"
    elif [[ -f "$(state_path msg)" ]]; then
      # The sync merge under it was committed by hand and is not finished (commit_sync_merge).
      exit_with 3 "${wt} is merging main on top of a sync merge committed by hand: git merge \
--abort there, then alab/sync.sh --continue (it finishes the sync merge, then merges main)"
    fi
    # Git can leave a pending merge with no conflicting index entries after an interruption.
    if [[ "${mode}" == start && "${1:-}" != --observed-merge ]]; then
      if [[ "${sync_merge}" -eq 1 ]]; then
        stop_if_pathless_conflict "${merge_head}" ""
      else
        stop_if_pathless_conflict "${merge_head}" "" -c "merge.alab-upstream.driver=true"
      fi
    fi
    if [[ "${sync_merge}" -eq 1 ]]; then
      apply_removed_paths
      keep_owned_paths
    fi
    conflicts="$(unmerged)" || alab_die "cannot read unresolved paths"
    [[ -z "${conflicts}" ]] || {
      [[ "${sync_merge}" -eq 0 || -s "$(state_path hand)" ]] || printf '%s\n' "${conflicts}" >"$(state_path hand)"
      report_conflicts "${merge_head}"
      stop 3 "conflicts for a resolver: $(printf '%s\n' "${conflicts}" | tr '\n' ' ')"
    }
    markers="$(shared_new_markers --cached HEAD MERGE_HEAD)" || alab_die "cannot read staged conflict markers"
    if [[ -n "${markers}" ]]; then
      exit_with 3 "conflict markers are staged in ${wt}: finish the resolution, then --continue"
    fi
    worktree_fully_staged || exit_with 3 "unstaged or untracked changes in ${wt}: \
stage the resolution (git add) or discard it, then --continue"
    if [[ "${sync_merge}" -eq 1 ]]; then
      commit_sync_merge
    else
      # A resolved merge of main's concurrent commits (take_concurrent_main).
      reresolve_cargo_lock
      in_wt commit --quiet -m "alab(sync): merge main's concurrent commits"
    fi
  elif [[ -f "$(state_path msg)" ]]; then
    # The sync merge is committed, but not by this script: the resolver ran `git commit` on
    # the open merge. Give it what this script adds; only an amend of that merge can, so
    # nothing may be committed on top of it yet.
    parent="$(in_wt rev-parse -q --verify HEAD^2)" || parent_rc=$?
    case "${parent_rc}" in
      0) ;;
      1) parent="" ;; # HEAD is known to have no second parent.
      *) alab_die "cannot read sync merge parent (git exit ${parent_rc})" ;;
    esac
    if [[ "${parent}" != "${target}" ]]; then
      alab_ancestor_check in_wt merge-base --is-ancestor "${target}" HEAD || exit_with 3 "the sync's merge of \
${target:0:9} is gone from ${wt} (aborted by hand?): alab/sync.sh --abort, then sync again"
      exit_with 3 "the sync merge in ${wt} was committed by hand and is not HEAD: fold the commits \
on top of it into it (git reset --soft <the merge>, git commit --amend), then alab/sync.sh --continue"
    fi
    worktree_fully_staged || exit_with 3 "unstaged or untracked changes in ${wt}: \
stage them (git add) or discard them, then --continue"
    markers="$(shared_new_markers HEAD^1 HEAD^2 HEAD)" || alab_die "cannot read committed conflict markers"
    if [[ -n "${markers}" ]]; then
      exit_with 3 "conflict markers are committed in the sync merge in ${wt}: finish the \
resolution, git commit --amend, then --continue"
    fi
    commit_sync_merge --amend
  fi
  worktree_clean || exit_with 4 "uncommitted changes in ${wt}: commit \
the fix or discard it, then alab/sync.sh --continue"
  check_current_main
  if [[ "${push}" -eq 0 ]]; then
    if [[ "${check}" -eq 1 ]]; then
      write_stop no-push
      alab_say "merged and checked in ${wt}, not pushed (--no-push): alab/sync.sh --continue lands it"
    else
      write_stop no-push-unchecked
      alab_say "merged in ${wt}, neither checked nor pushed: alab/sync.sh --continue checks and lands it"
    fi
    return 0
  fi
  before="$(git rev-parse --short origin/main)"
  push_main
  sync_fetch origin
  rm -f "$(state_path target)" "$(state_path stop)"
  # A main nobody has checked out follows along; a checked-out main is its owner's to pull.
  if git worktree list --porcelain | has_line '^branch refs/heads/main$'; then
    alab_say "landed ${before}..$(git rev-parse --short origin/main); checkouts on main: git pull --ff-only"
  else
    # The remote already landed; an unreadable optional local main stays put.
    if git merge-base --is-ancestor main origin/main 2>/dev/null; then
      git branch --quiet -f main origin/main
    fi
    alab_say "landed ${before}..$(git rev-parse --short origin/main) on main"
  fi
}

# Measure Git's actual output paths and keep its writer group under the running disk floor.
sync_disk_write() {
  local objects index cwd scratch="${TMPDIR:-/tmp}"
  objects="$(git rev-parse --path-format=absolute --git-path objects)" || return 2
  index="$(git rev-parse --path-format=absolute --git-path index)" || return 2
  cwd="$(pwd -P)" || return 2
  [[ "${scratch}" == /* ]] || scratch="${cwd}/${scratch}"
  "${ALAB_PYTHON:-/usr/bin/python3}" -I -B "${repo_root}/alab/command_guard.py" \
    --inherit-fds --resource-stop --min-free-gib 20 --disk-path "${objects}" \
    --disk-path "${index}" --disk-path "${common_dir}" --disk-path "${cwd}" --disk-path "${scratch}" -- "$@"
}

# A source-only merge can run without build space. A checked merge admits its checkout,
# index and object store at launch and watches them until Git and its writers have stopped.
write_in_wt() {
  if [[ "${check}" -eq 0 ]]; then
    in_wt "$@"
  else
    (cd "${wt}" && sync_disk_write git "$@")
  fi
}

# Print clean, pathless, or Git's tree and conflicting file names.
sync_merge_probe() {
  local checked="$1" left="$2" right="$3" out rc=0 report
  shift 3
  # Git's 1 means conflict. The guardian's 1 can mean it never launched Git.
  # Carry the clean/conflicted decision in stdout only after Git completed.
  local command=(/bin/bash -c '
    status=0
    git "$@" || status=$?
    case "$status" in 0 | 1) printf "\n%s\n" "$status" ;; *) exit "$status" ;; esac
  ' alab-merge-tree "${merge_cfg[@]}" "$@" -c core.quotepath=off merge-tree --write-tree --name-only \
    --no-messages "${left}" "${right}")
  if [[ "${checked}" -eq 1 ]]; then command=(sync_disk_write "${command[@]}"); fi
  # Use the actual merge worktree's attributes, drivers, and per-worktree configuration.
  out="$(cd "${wt}" && "${command[@]}")" || rc=$?
  if [[ "${rc}" -eq 75 || "${rc}" -gt 128 ]]; then exit "${rc}"; fi
  [[ "${rc}" -eq 0 ]] || alab_die "cannot probe unattended merge (exit ${rc})"
  case "${out##*$'\n'}" in
    0) printf '%s\n' clean ;;
    1)
      report="${out%$'\n'*}"
      report="${report%$'\n'}"
      [[ -n "${report}" ]] || alab_die "cannot read unattended-merge result tree"
      if [[ "${report}" == *$'\n'* ]]; then
        printf '%s\n' "${report}"
      else
        printf '%s\n' pathless
      fi
      ;;
    *) alab_die "cannot read unattended-merge result" ;;
  esac
}

# A file conflict may already be staged by rerere. Only an independently
# reported conflict with no file paths needs an explicit resolver decision.
stop_if_pathless_conflict() {
  local target="$1" detail="$2" paths head result
  shift 2
  paths="$(unmerged)" || alab_die "cannot read unresolved paths"
  [[ -z "${paths}" ]] || return 0
  head="$(in_wt rev-parse --verify HEAD^{commit})" || alab_die "cannot read sync merge first parent"
  result="$(sync_merge_probe "${check}" "${head}" "${target}" "$@")" || exit $?
  [[ "${result}" == pathless ]] || return 0
  [[ -z "${detail}" ]] || printf '%s\n' "${detail}" >&2
  exit_with 3 "Git reported a merge conflict without conflicting file paths in ${wt}. \
Inspect the merged tree, stage the intended layout, then run alab/sync.sh --continue."
}

# Every conflicting file must match removed-paths or owned-paths, by Git's pathspec matching.
merges_unattended() {
  local out rc=0 list=() line idx directory uncovered covered conflict_paths policy policy_name
  out="$(sync_merge_probe 1 origin/main "$1")" || exit $?
  case "${out}" in clean) return 0 ;; pathless) return 1 ;; esac
  for policy_name in removed-paths owned-paths; do
    policy="$(specs "${policy_name}")" || alab_die "cannot read alab/${policy_name} from origin/main"
    while IFS= read -r line; do [[ -z "${line}" ]] || list+=("${line}"); done <<<"${policy}"
  done
  [[ ${#list[@]} -gt 0 ]] || return 1
  covered="$(
    cd "${wt}" || alab_die "cannot enter the sync worktree for conflict coverage"
    directory="$(mktemp -d "${common_dir}/alab-probe.XXXXXXXX")" || alab_die "cannot reserve unattended-merge probe index"
    idx="${directory}/index"
    trap '
      saved=$?
      if ! rm -f -- "${idx}" "${idx}.lock" || ! rmdir -- "${directory}"; then
        [[ "${saved}" -ne 0 ]] || saved=1
      fi
      exit "${saved}"
    ' EXIT
    GIT_INDEX_FILE="${idx}" sync_disk_write git read-tree "${out%%$'\n'*}" || {
      rc=$?
      if [[ "${rc}" -eq 75 || "${rc}" -gt 128 ]]; then exit "${rc}"; fi
      alab_die "cannot read unattended-merge probe tree"
    }
    GIT_INDEX_FILE="${idx}" git -c core.quotepath=off ls-files -- "${list[@]}" \
      || alab_die "cannot read unattended-merge probe paths"
  )" || exit $?
  conflict_paths="$(printf '%s\n' "${out}" | sed -e 1d -e '/^$/d' | sort -u)" \
    || alab_die "cannot sort unattended-merge conflict paths"
  covered="$(printf '%s\n' "${covered}" | sort -u)" || alab_die "cannot sort unattended-merge covered paths"
  uncovered="$(comm -23 <(printf '%s\n' "${conflict_paths}") <(printf '%s\n' "${covered}"))" \
    || alab_die "cannot compare unattended-merge probe paths"
  [[ -z "${uncovered}" ]]
}

# Prints "<clean-prefix-or-empty>|<first-commit-needing-a-resolver>", or "<target>|" when
# the whole range merges unattended. Upstream's history is linear, so its first-parent
# commits are the steps; a binary search keeps the probes to a handful.
stepwise_plan() {
  local target="$1" commits=() c lo hi mid frontier commits_text
  if merges_unattended "${target}"; then
    echo "${target}|"
    return
  fi
  frontier="$(git merge-base origin/main "${target}")" || alab_die "cannot read stepwise frontier"
  commits_text="$(git rev-list --reverse --first-parent "${frontier}..${target}")" \
    || alab_die "cannot read stepwise upstream commits"
  [[ -n "${commits_text}" ]] || alab_die "stepwise frontier contains no upstream commits"
  while IFS= read -r c; do commits+=("${c}"); done <<<"${commits_text}"
  lo=0
  hi=${#commits[@]}
  while [[ $((hi - lo)) -gt 1 ]]; do
    mid=$(((lo + hi) / 2))
    if merges_unattended "${commits[$((mid - 1))]}"; then lo=${mid}; else hi=${mid}; fi
  done
  if [[ "${lo}" -gt 0 ]]; then
    echo "${commits[$((lo - 1))]}|${commits[$((hi - 1))]}"
  else
    echo "|${commits[$((hi - 1))]}"
  fi
}

# Claim the reserved branch using Git's worktree checks and only fast-forward it.
# A stepwise run calls this again after each landed prefix.
prepare_sync_branch() {
  local main="$1" head tip rc=0 selected branch state
  state="$(sync_worktree_state)" || return $?
  [[ "${state}" == present ]] || alab_die "cannot verify the sync worktree at ${wt}"
  if merging; then alab_die "a merge is already in progress in ${wt}"; fi
  GIT_OPTIONAL_LOCKS=0 worktree_clean --untracked-files=normal || alab_die "${wt} has uncommitted or untracked files"
  # Admit even an already prepared branch before run_sync writes any recovery records.
  head="$(write_in_wt rev-parse --verify 'HEAD^{commit}')" || return $?
  alab_ancestor_check in_wt merge-base --is-ancestor "${head}" "${main}" || alab_die "${wt} holds commits main lacks"
  tip="$(in_wt rev-parse -q --verify 'refs/heads/alab-sync^{commit}')" || rc=$?
  case "${rc}" in
    0)
      alab_ancestor_check in_wt merge-base --is-ancestor "${tip}" "${main}" \
        || alab_die "alab-sync holds commits main lacks"
      branch="$(in_wt rev-parse --symbolic-full-name HEAD)" || alab_die "cannot read the current sync branch"
      if [[ "${branch}" != refs/heads/alab-sync ]]; then
        write_in_wt switch --quiet --no-guess --no-overwrite-ignore alab-sync || return $?
      fi
      ;;
    1) write_in_wt switch --quiet --no-track --no-overwrite-ignore -c alab-sync "${main}" || return $? ;;
    *) alab_die "cannot read the reserved alab-sync branch (git exit ${rc})" ;;
  esac
  branch="$(in_wt symbolic-ref --quiet HEAD)" || alab_die "cannot identify the selected sync branch"
  [[ "${branch}" == refs/heads/alab-sync ]] || alab_die "Git selected a different sync branch"
  selected="$(in_wt rev-parse --verify HEAD)" || alab_die "cannot read the selected sync tip"
  if [[ "${selected}" != "${main}" ]]; then
    alab_ancestor_check in_wt merge-base --is-ancestor "${selected}" "${main}" \
      || alab_die "alab-sync moved to commits main lacks"
    write_in_wt merge --quiet --ff-only --no-autostash --no-overwrite-ignore "${main}" || return $?
  fi
  # A merge also succeeds if HEAD is already ahead. Only the pinned main is a sync base.
  branch="$(in_wt symbolic-ref --quiet HEAD)" || alab_die "cannot read the prepared sync branch"
  selected="$(in_wt rev-parse --verify HEAD)" || alab_die "cannot read the prepared sync tip"
  [[ "${branch}" == refs/heads/alab-sync && "${selected}" == "${main}" ]] || alab_die "the sync branch moved during preparation"
}

# Merge <target> into origin/main and land it (finish), or stop for a resolver (exit 3).
run_sync() {
  local target="$1" base pending merge_rc=0 merge_out conflicts
  local main short_target upstream_summary progress_summary
  main="$(git rev-parse --verify 'refs/remotes/origin/main^{commit}')" || alab_die "cannot read sync merge metadata: origin/main"
  if alab_ancestor_check git merge-base --is-ancestor "${target}" "${main}"; then
    alab_say "main already contains $(git log -1 --format='%h %cs' "${target}"): nothing to do"
    return 0
  fi
  base="$(git merge-base "${main}" "${target}")" || alab_die "cannot read sync merge metadata: merge base"
  pending="$(git rev-list --count "${main}..${target}")" || alab_die "cannot read sync merge metadata: commit count"
  short_target="$(git rev-parse --short=12 "${target}")" || alab_die "cannot read sync merge metadata: target identity"
  upstream_summary="$(git log -1 --format='%H (%cs) %s' "${target}")" || alab_die "cannot read sync merge metadata: upstream summary"
  progress_summary="$(git log -1 --format='%h %cs %s' "${target}")" || alab_die "cannot read sync merge metadata: progress summary"
  prepare_sync_branch "${main}" || return $?
  {
    echo "alab sync: merge upstream openai/codex@${short_target}"
    echo
    echo "Upstream: ${upstream_summary}"
    echo "New upstream commits: ${pending}"
  } >"$(state_path msg)"
  : >"$(state_path auto)"
  : >"$(state_path hand)"
  rm -f "$(state_path stop)"
  echo "${target}" >"$(state_path target)"
  note_watched_paths "${base}" "${target}"

  alab_say "merging ${pending} upstream commit(s) up to ${progress_summary}"
  merge_out="$(write_in_wt "${merge_cfg[@]}" merge --no-ff --no-commit "${target}" 2>&1)" || merge_rc=$?
  if [[ "${merge_rc}" -eq 75 || "${merge_rc}" -gt 128 ]]; then
    printf '%s\n' "${merge_out}" >&2
    # The status probe can itself fail on an interrupted index. Preserve the original stop.
    (if ! merging && interrupted_checkout; then
      alab_say "Git stopped before recording a merge in ${wt}; inspect its index lock and checkout before retrying"
    fi) || true
    return "${merge_rc}"
  fi
  if ! merging; then
    printf '%s\n' "${merge_out}" >&2
    clear_state
    alab_die "git merge stopped before merging (exit ${merge_rc}); ${wt} is unchanged"
  fi
  [[ "${merge_rc}" -le 1 ]] || alab_die "git merge failed in ${wt} (exit ${merge_rc}): ${merge_out}"
  if [[ "${merge_rc}" -eq 1 ]]; then stop_if_pathless_conflict "${target}" "${merge_out}"; fi
  printf '%s\n' "${merge_out}" \
    | sed -E -n "s/^(Resolved|Staged) '(.*)' using previous resolution\.$/  rerere: \2/p" \
      >>"$(state_path auto)"

  apply_removed_paths
  keep_owned_paths
  conflicts="$(unmerged)" || alab_die "cannot read unresolved paths"
  if [[ -n "${conflicts}" ]]; then
    printf '%s\n' "${conflicts}" >"$(state_path hand)"
    report_conflicts "${target}"
    stop 3 "conflicts for a resolver: $(printf '%s\n' "${conflicts}" | tr '\n' ' ')"
  fi
  finish --observed-merge
}

case "${mode}" in
  status)
    echo "origin/main    $(git log -1 --format='%h %cs' origin/main 2>/dev/null || echo missing)"
    echo "upstream/main  $(git log -1 --format='%h %cs' upstream/main) ($(git rev-list --count \
origin/main..upstream/main 2>/dev/null || echo '?') upstream commits not yet in main; fetch first for news)"
    # Observe the same kernel lock as the driver before offering stop/recovery
    # instructions. A committed prefix is still active while its gate runs.
    activity_rc=0
    alab_locked "${common_dir}/alab-sync.lockf" /usr/bin/true || activity_rc=$?
    case "${activity_rc}" in
      0) echo "sync activity=inactive" ;;
      75)
        echo "sync activity=active"
        echo "sync worktree  active (${wt}; sync lock held)"
        exit 0
        ;;
      *) echo "sync activity=unknown"; exit "${activity_rc}" ;;
    esac
    if ! state="$(sync_worktree_state status)"; then
      echo "sync worktree  unknown (could not verify configured checkout)"
      exit 1
    fi
    # The helper appends a slash so command substitution preserves trailing newlines.
    if [[ "${state}" == unavailable\ * || "${state}" == elsewhere\ * ]]; then
      holder="${state#* }"
      if [[ "${state}" == unavailable\ * ]]; then
        printf 'sync worktree  registered branch checkout unavailable (%q)\n' "${holder%/}"
      else
        printf 'sync worktree  branch held elsewhere (%q)\n' "${holder%/}"
      fi
      printf '  configured worktree: %q\n' "${wt}"
    elif [[ "${state}" == absent ]]; then
      echo "sync worktree  none yet (${wt})"
    elif merging || unlanded; then
      stop_reason="$(stop_code)" || alab_die "cannot read sync stop record"
      if merging; then
        echo "sync worktree  merging $(in_wt rev-parse --short MERGE_HEAD), $(waiting_hours)h in ${wt}"
        unmerged | sed 's/^/  unresolved: /'
      elif alab_ancestor_check in_wt merge-base --is-ancestor origin/main HEAD; then
        echo "sync worktree  committed, not landed, $(waiting_hours)h: alab/sync.sh --continue (${wt})"
      else
        echo "sync worktree  committed, not landed, $(waiting_hours)h, origin/main moved since: \
--continue merges it in (${wt})"
      fi
      case "${stop_reason}" in
        3 | 4 | 6)
          echo "  stopped for a person (exit ${stop_reason}):"
          sed -e 1d -e '/^$/d' -e 's/^/    /' "$(state_path stop)"
          ;;
        no-push) echo "  merged and checked with --no-push" ;;
        no-push-unchecked) echo "  merged with --no-push --no-check: not checked yet" ;;
        continue) echo "  a person's --continue stopped on an error (alab/sync.sh --continue again, or --abort)" ;;
        *) echo "  stopped on an error, not for a resolver: the next alab/sync.sh resumes it" ;;
      esac
    elif interrupted_checkout; then
      echo "sync worktree  interrupted before Git recorded a merge; inspect its index lock and checkout (${wt})"
    elif ! GIT_OPTIONAL_LOCKS=0 worktree_clean --untracked-files=normal; then
      echo "sync worktree  idle, with uncommitted or untracked files (${wt})"
    else
      echo "sync worktree  idle (${wt})"
    fi
    exit 0
    ;;
  abort)
    take_lock
    state="$(sync_worktree_state)" || exit $?
    [[ "${state}" == present ]] || exit 0
    in_wt merge --abort >/dev/null 2>&1 || true
    in_wt reset --quiet --hard origin/main
    in_wt clean -fdq
    clear_state
    alab_say "sync abandoned; ${wt} is back at origin/main (rerere keeps what it learned)"
    exit 0
    ;;
  continue)
    take_lock
    state="$(sync_worktree_state)" || exit $?
    [[ "${state}" == present ]] || alab_die "no sync worktree at ${wt}: start one with alab/sync.sh"
    finish
    exit 0
    ;;
esac

# mode=start
take_lock
sync_fetch upstream
sync_fetch origin
git rev-parse -q --verify origin/main >/dev/null || alab_die "origin has no main branch to sync"
target="$(git rev-parse --verify "${ref}^{commit}")" || alab_die "no such upstream revision '${ref}'"
# A release tag is cut off upstream main (version bump, cherry-picks): merged here, its
# "version = X" and cherry-picks would stay in main through every later sync.
alab_ancestor_check git merge-base --is-ancestor "${target}" upstream/main || alab_die "${ref} is not on upstream/main \
(a release tag? those are cut off main): main tracks upstream/main only (alab/README.md, 'Releases')"
# Whatever upstream commit main merged last must still be upstream history.
last_merged="$(alab_last_sync_upstream origin/main)" || alab_die "cannot read main's last upstream sync"
[[ -z "${last_merged}" ]] || alab_ancestor_check git merge-base --is-ancestor "${last_merged}" upstream/main \
  || alab_die "upstream/main no longer contains ${last_merged:0:9}, the upstream commit main \
merged last: upstream rewrote its history (alab/README.md, 'If upstream rewrites history')"
if alab_ancestor_check git merge-base --is-ancestor "${target}" origin/main; then
  alab_say "main already contains ${ref} ($(git log -1 --format='%h %cs' "${target}")): nothing to do"
  exit 0
fi

ensure_worktree
if merging || unlanded; then
  stop_reason="$(stop_code)" || alab_die "cannot read sync stop record"
  case "${stop_reason}" in
    "") ;;
    no-push) exit_with 5 "a sync in ${wt} was merged and checked with --no-push $(waiting_hours)h \
ago and is not landed: alab/sync.sh --continue lands it, or --abort" ;;
    no-push-unchecked) exit_with 5 "a sync in ${wt} was merged with --no-push --no-check \
$(waiting_hours)h ago: alab/sync.sh --continue checks and lands it, or --abort" ;;
    *) exit_with 5 "a sync in ${wt} has waited $(waiting_hours)h for its resolver \
(alab/sync.sh --status): resolve it and --continue, or --abort" ;;
  esac
  sync_target >/dev/null # commits that are no sync's stop here, before anything is resumed
  alab_say "the sync in ${wt} stopped on an error last time, not for a resolver: resuming it"
  finish
  # Under --no-push the pending prefix stays on alab-sync until it lands.
  [[ "${push}" -eq 1 ]] || exit 0
fi
if interrupted_checkout; then
  alab_die "a previous sync stopped before Git recorded a merge in ${wt}; inspect its index lock and checkout before starting another sync"
fi
GIT_OPTIONAL_LOCKS=0 worktree_clean --untracked-files=normal || alab_die "${wt} has uncommitted or untracked files and no sync in progress"

if [[ "${stepwise}" -eq 0 ]]; then
  run_sync "${target}"
  exit 0
fi
# Each round lands a clean prefix and then the first change that needs a resolver; a round
# whose change rerere resolved lands as well, and the next round goes on from there.
while ! alab_ancestor_check git merge-base --is-ancestor "${target}" origin/main; do
  before="$(git rev-parse origin/main)"
  stepwise_result="$(stepwise_plan "${target}")" || exit $?
  IFS='|' read -r prefix first <<<"${stepwise_result}"
  if [[ -n "${prefix}" ]]; then
    [[ -z "${first}" ]] || alab_say "stepwise: landing the clean prefix up to $(git log -1 --format='%h %s' "${prefix}")"
    run_sync "${prefix}"
  fi
  if [[ -n "${first}" ]]; then
    alab_say "stepwise: next is $(git log -1 --format='%h %s' "${first}"), which needs a resolver"
    run_sync "${first}"
  fi
  [[ "$(git rev-parse origin/main)" != "${before}" ]] || alab_die "stepwise made no progress toward ${target:0:9}"
done
