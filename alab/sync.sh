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
# Every step that runs upstream's code (the lock re-resolve, alab/check.sh, the pristine build
# that tells an ALab fault from a lane fault) runs contained (lib.sh, alab_contained), with
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
# merge to land); 6 upstream itself does not build and run with this toolchain (a lane fault,
# fixed in Trust, not here); 1, or a failing git command's own status (128), anything else.
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
[[ "${stepwise}" -eq 0 || "${push}" -eq 1 ]] \
  || alab_die "--stepwise lands its clean prefix before merging further, so it cannot run with --no-push"
alab_upstream_ready
[[ "${mode}" == status || "$(git config --bool rerere.enabled || true)" == true ]] \
  || alab_die "rerere is off in this clone, so no resolution would be remembered: run alab/setup.sh"

common_dir="$(git rev-parse --path-format=absolute --git-common-dir)"
wt="${ALAB_SYNC_DIR:-${common_dir}/alab-sync}"
# A relative ALAB_SYNC_DIR means relative to where the caller stood, not to the repo root.
case "${wt}" in /*) ;; *) wt="${invoked_from}/${wt}" ;; esac
cargo="${ALAB_CARGO:-targo}"
# Every sync merge, and every --stepwise probe, merges with these: codex-rs/Cargo.lock is
# derived data, so when both sides changed it upstream's side wins and is re-resolved at
# commit time. Scoped to these commands; no other merge in the clone is affected. (A merge
# of main's concurrent commits keeps the sync's side instead: take_concurrent_main.)
merge_cfg=(-c "core.attributesFile=${repo_root}/alab/merge-attributes"
  -c "merge.alab-upstream.name=upstream's side, re-resolved by alab/sync.sh"
  -c "merge.alab-upstream.driver=cp -f %B %A")

in_wt() { git -C "${wt}" "$@"; }
merging() { in_wt rev-parse -q --verify MERGE_HEAD >/dev/null 2>&1; }
unmerged() { in_wt -c core.quotepath=off diff --name-only --diff-filter=U "$@"; }
# The sync worktree holds commits origin/main does not (a merge, fix-forward commits).
unlanded() { [[ -n "$(in_wt rev-list -1 origin/main..HEAD)" ]]; }
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
state_path() { in_wt rev-parse --path-format=absolute --git-path "alab-sync-$1"; }
# The upstream commit this sync merges (alab-sync-target); a sync without that record stops for a
# person rather than guessing.
sync_target() {
  cat "$(state_path target)" 2>/dev/null && return 0
  # No merge and no message either: the commits are not a sync's, and --abort would drop them.
  if ! merging && [[ ! -f "$(state_path msg)" ]]; then
    exit_with 3 "${wt} holds commits main lacks (git -C ${wt} log origin/main..HEAD), and no sync \
is in progress: land or move them yourself, then alab/sync.sh --abort"
  fi
  exit_with 3 "the sync in ${wt} lost its state file $(state_path target): alab/sync.sh --abort, then sync again"
}
clear_state() {
  rm -f "$(state_path msg)" "$(state_path msg).final" "$(state_path auto)" "$(state_path hand)" "$(state_path target)" \
    "$(state_path stop)"
}
# stop <code> [<why>]: exit, recording a stop for a person (3, 4, 6) and why, for --status. A
# sync that stopped on an error instead (exit 1: the network, a missing tool) has no record,
# and the next run resumes it.
stop() {
  case "$1" in
    3 | 4 | 6) printf '%s\n%s\n' "$1" "${2:-}" >"$(state_path stop)" 2>/dev/null || true ;;
  esac
  exit "$1"
}
# stop_code: the first line of the stop record (3, 4, 6, no-push, no-push-unchecked, continue).
stop_code() { head -n 1 "$(state_path stop)" 2>/dev/null || true; }
exit_with() {
  local code="$1"
  shift
  printf 'alab: %s\n' "$*" >&2
  stop "${code}" "$*"
}
# specs <name>: the pathspecs in alab/<name> (removed-paths, owned-paths, watched-paths) as
# main has them; the sync worktree's copy can lag behind main between syncs.
specs() { git show "origin/main:alab/$1" 2>/dev/null | alab_spec_lines; }
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

ensure_worktree() {
  local holder
  holder="$(git worktree list --porcelain \
    | awk '/^worktree /{ p = substr($0, 10) } $0 == "branch refs/heads/alab-sync" { print p }')"
  # A sync worktree deleted by hand is still registered: drop that registration alone (git
  # worktree prune would drop a person's worktree on a disk that is not mounted now, too),
  # unless alab-sync holds commits main lacks: a new sync would reset the branch and leave them
  # in its reflog alone.
  if [[ -n "${holder}" && ! -e "${holder}/.git" ]]; then
    [[ -z "$(git rev-list -1 origin/main..alab-sync)" ]] || alab_die "the sync worktree ${holder} \
was deleted, and alab-sync holds commits main lacks (git log origin/main..alab-sync): \
git worktree add --force ${holder} alab-sync brings it back with them; land them, or drop them \
with alab/sync.sh --abort"
    git worktree remove --force --force "${holder}"
    holder=""
  fi
  if [[ -n "${holder}" && ! "${holder}" -ef "${wt}" ]]; then
    alab_die "the sync worktree of this clone is ${holder}, not ${wt}: set ALAB_SYNC_DIR=${holder}, \
or retire it (alab/sync.sh --abort with that ALAB_SYNC_DIR, then git worktree remove ${holder})"
  fi
  if [[ ! -e "${wt}/.git" ]]; then
    # -f -f: ${wt} may still be registered (deleted by hand, or locked by a killed add).
    git worktree add --quiet --force --force --detach "${wt}" origin/main
    alab_say "sync worktree created at ${wt}"
  fi
}

# Status lines (porcelain v1) that are not fully staged: unstaged edits and untracked files.
loose_changes() { in_wt status --porcelain | grep -v '^[MADRCT] ' || true; }

apply_removed_paths() {
  local list=() line count
  while IFS= read -r line; do list+=("${line}"); done < <(specs removed-paths)
  [[ ${#list[@]} -gt 0 ]] || return 0
  count="$(in_wt ls-files -- "${list[@]}" | sort -u | wc -l | tr -d ' ')" \
    || alab_die "alab/removed-paths has an entry git rejects (above): fix it in an alab(removed) \
commit on main, then alab/sync.sh --abort and sync again"
  if [[ "${count}" -gt 0 ]]; then
    in_wt rm -r -q -f --ignore-unmatch -- "${list[@]}"
    echo "  removed-paths: dropped ${count} upstream file(s)" >>"$(state_path auto)"
  fi
}

# A conflict in a path ALab owns keeps ALab's side. Upstream's version is kept beside it in
# alab/upstream/<path>, so whoever maintains the ALab file can carry over what matters.
keep_owned_paths() {
  local list=() line f ours theirs conflicted
  while IFS= read -r line; do list+=("${line}"); done < <(specs owned-paths)
  [[ ${#list[@]} -gt 0 ]] || return 0
  # Listed in full first: git diff refreshes the index under index.lock after it prints, so a
  # git add in the loop below, fed by it directly, can race it (exit 128, "index.lock exists").
  conflicted="$(unmerged -- "${list[@]}")"
  while IFS= read -r f; do
    [[ -n "${f}" ]] || continue
    if in_wt cat-file -e "MERGE_HEAD:${f}" 2>/dev/null; then
      mkdir -p "$(dirname "${wt}/alab/upstream/${f}")"
      in_wt show "MERGE_HEAD:${f}" >"${wt}/alab/upstream/${f}"
      in_wt add -- "alab/upstream/${f}"
      theirs="upstream's is alab/upstream/${f}"
    else
      in_wt rm -q -f --ignore-unmatch -- "alab/upstream/${f}"
      theirs="upstream deleted it"
    fi
    if in_wt cat-file -e "HEAD:${f}" 2>/dev/null; then
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
  local lock="${wt}/codex-rs/Cargo.lock" build_target rc=0
  [[ -f "${wt}/codex-rs/Cargo.toml" ]] || return 0
  command -v "${cargo}" >/dev/null 2>&1 \
    || alab_die "${cargo} is not on PATH: it re-resolves codex-rs/Cargo.lock (ALAB_CARGO names another)"
  # A target dir is made out here: inside, cargo would make it beside itself, and cannot.
  build_target="$(alab_build_target_ready "${wt}/codex-rs" "${CARGO_TARGET_DIR:-${wt}/codex-rs/target}")" \
    || alab_die "the resolver target could not be prepared; no metadata command started"
  (cd "${wt}/codex-rs" && \
    { [[ -z "${CARGO_TARGET_DIR:-}" ]] || export CARGO_TARGET_DIR="${build_target}"; } && \
    alab_contained --read "${alab_library_root}/alab" "${lock}" "${build_target}" -- \
    /bin/bash -c '. "$1/alab/lib.sh"; shift; alab_build_command "$@"' \
    alab-resolver-disk "${alab_library_root}" "${cargo}" metadata --format-version 1 >/dev/null) || rc=$?
  [[ "${rc}" -ne 75 ]] || exit_with 75 "disk resource stop: no lockfile verdict; free room, then alab/sync.sh --continue"
  [[ "${rc}" -eq 0 ]] || alab_die "${cargo} metadata could not resolve codex-rs in ${wt}: fix the manifests, then --continue"
  [[ -f "${lock}" && ! -L "${lock}" ]] || alab_die "${lock} is not a regular file after ${cargo} metadata"
  if ! in_wt diff --quiet -- codex-rs/Cargo.lock; then
    in_wt add -- codex-rs/Cargo.lock
    echo "  codex-rs/Cargo.lock re-resolved" >>"$(state_path auto)"
  fi
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
  local target own drift
  [[ -f "${wt}/codex-rs/Cargo.toml" ]] || return 0
  target="$(sync_target)"
  in_wt cat-file -e "${target}:codex-rs/Cargo.lock" 2>/dev/null || return 0
  own="$(vendored_packages)"
  drift="$(comm -23 <(in_wt show "${target}:codex-rs/Cargo.lock" | lock_packages) \
    <(lock_packages <"${wt}/codex-rs/Cargo.lock") \
    | awk -v own="${own}" 'BEGIN { n = split(own, l, "\n"); for (i = 1; i <= n; i++) set[l[i]] }
      !(($1 " " $2 " " $4) in set)')"
  [[ -z "${drift}" ]] || exit_with 3 "codex-rs/Cargo.lock in ${wt} no longer holds these packages \
as upstream locked them:
$(printf '%s\n' "${drift}" | awk '{ gsub(/"/, ""); print "  " $1 " " $2 }')
Put each back (cd codex-rs && ${cargo} update -p <name>@<new> --precise <old>), git add \
codex-rs/Cargo.lock, then alab/sync.sh --continue"
}

# Upstream changes to paths ALab watches go into the merge message and the run's output.
note_watched_paths() {
  local base="$1" target="$2" list=() line changed
  while IFS= read -r line; do list+=("${line}"); done < <(specs watched-paths)
  [[ ${#list[@]} -gt 0 ]] || return 0
  changed="$(git -c core.quotepath=off diff --name-only "${base}" "${target}" -- "${list[@]}")"
  [[ -n "${changed}" ]] || return 0
  {
    echo "Upstream changed watched paths (alab/watched-paths):"
    printf '%s\n' "${changed}" | sed 's/^/  /'
  } | tee -a "$(state_path msg)" >&2
}

report_conflicts() {
  local target="$1" base f
  base="$(git merge-base origin/main "${target}")"
  {
    echo "conflicts for a resolver, in ${wt}:"
    while IFS= read -r f; do
      [[ -n "${f}" ]] || continue
      echo "  ${f}"
      git log --format='      upstream %h %s' -3 "${base}..${target}" -- "${f}"
      git log --no-merges --format='      alab     %h %s' -3 "${base}..origin/main" -- "${f}"
    done < <(unmerged)
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

# The check failed. If it was the build (check.sh exit 2), build the pristine upstream
# commit with the same toolchain: when that fails too, the fault is the toolchain's.
check_failed() {
  local rc="$1" target pristine scratch_home pristine_rc=0
  target="$(cat "$(state_path target)" 2>/dev/null || true)"
  if [[ "${rc}" -eq 2 && -n "${target}" ]]; then
    pristine="$(alab_pristine "${wt}" "${target}")" || exit_with 4 "the pristine upstream revision could not be resolved"
    alab_say "the build failed; building pristine upstream ${target:0:9} to tell an ALab fault from a lane fault"
    alab_pristine_at "${wt}" "${target}" || exit_with 4 "alab/check.sh failed in ${wt} (above), and \
pristine upstream ${target:0:9} could not be checked out at ${pristine} to tell an ALab fault from a \
lane fault. Fix that, then alab/sync.sh --continue"
    # It builds and runs what check.sh step 2 does, so a binary that builds but cannot run
    # counts, in the pristine tree's own target dir (lib.sh, alab_pristine).
    # Contained, as check.sh is (run_check).
    scratch_home="$(mktemp -d "${TMPDIR:-/tmp}/alab-pristine-home.XXXXXX")"
    alab_build_target_ready "${pristine}/codex-rs" "${pristine}/codex-rs/target" >/dev/null \
      || exit_with 4 "the pristine target could not be prepared; no build started"
    (cd "${pristine}/codex-rs" && export CARGO_TARGET_DIR="${pristine}/codex-rs/target" \
      CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CODEX_HOME="${scratch_home}" \
      && alab_contained --pristine "${pristine}" --read "${alab_library_root}/alab" "${CARGO_TARGET_DIR}" "${scratch_home}" -- \
        /bin/bash -c '. "$1/alab/lib.sh"; shift; alab_build_command "$@"' \
        alab-pristine-disk "${alab_library_root}" "${cargo}" --unverified run --quiet --locked -p codex-cli --bin codex -- --version) || pristine_rc=$?
    if [[ "${pristine_rc}" -ne 0 ]]; then
      rm -rf "${scratch_home:?}"
      [[ "${pristine_rc}" -ne 75 ]] || exit_with 75 "disk resource stop during pristine build: no compiler verdict; free room, then alab/sync.sh --continue"
      exit_with 6 "upstream ${target:0:9} itself does not build and run with ${cargo} (above): a \
Trust lane fault, fixed in Trust, not in codex-alab. Once the toolchain is fixed: alab/sync.sh --continue"
    fi
    rm -rf "${scratch_home:?}"
    alab_say "upstream ${target:0:9} alone builds and runs: ALab's layer broke it"
  fi
  exit_with 4 "alab/check.sh failed in ${wt} (above). A wrong resolution is fixed in the merge \
itself (git rerere forget <file>, fix, git commit --amend); an adaptation to upstream's \
change is its own commit. Then alab/sync.sh --continue"
}

run_check() {
  local rc=0
  [[ "${check}" -eq 1 ]] || return 0
  alab_say "checking $(in_wt log -1 --format='%h %s') with alab/check.sh"
  # The executable gate owns native project selection, the shared target
  # inventory and containment for both source and pristine.
  (cd "${wt}" && ./alab/check.sh) || rc=$?
  # Killed by a signal (memory pressure, a person's Ctrl-C), not failed: no fault is known yet,
  # so no stop is recorded and the next run checks again. check.sh passes a killed cargo's
  # status up too.
  [[ "${rc}" -ne 75 ]] || exit_with 75 "disk resource stop during check: no source verdict; free room, then alab/sync.sh --continue"
  if [[ "${rc}" -gt 128 ]]; then
    [[ "${mode}" == continue ]] && alab_die "alab/check.sh was killed (exit ${rc}): alab/sync.sh --continue checks again"
    alab_die "alab/check.sh was killed (exit ${rc}): the next alab/sync.sh checks again"
  fi
  [[ "${rc}" -eq 0 ]] || check_failed "${rc}"
}

# Someone landed ALab work on main while this sync ran: merge it into the sync (the sync
# merge stays on main's first-parent line), so the result is again a fast-forward of main.
take_concurrent_main() {
  local main_upstream target own
  # Another clone's sync landed a newer upstream meanwhile: this merge is superseded, and its
  # lockfile (kept below) would roll back upstream's or fail the drift check.
  target="$(sync_target)"
  main_upstream="$(git log -1 --first-parent --merges --grep='^alab sync:' --format=%P origin/main | awk '{ print $2 }')"
  if [[ -n "${main_upstream}" ]] && ! git merge-base --is-ancestor "${main_upstream}" "${target}"; then
    # A person's commits on the sync (a fix-forward after exit 4) would go with --abort.
    own="$(in_wt log --no-merges --format='  %h %s' origin/main..HEAD)"
    exit_with 3 "main merged upstream ${main_upstream:0:9} meanwhile, past this sync's: \
alab/sync.sh --abort, then sync again (rerere replays the resolutions)${own:+. --abort drops \
these commits in ${wt}; land on main first any it still needs:
${own}}"
  fi
  alab_say "origin/main moved while this sync ran; merging its new commits into the sync"
  # codex-rs/Cargo.lock is derived here too: keep the sync's (upstream's, re-resolved), then
  # re-resolve it for the concurrent commits' crates under the same drift check.
  in_wt -c "core.attributesFile=${repo_root}/alab/merge-attributes" \
    -c "merge.alab-upstream.name=the sync's side, re-resolved by alab/sync.sh" \
    -c "merge.alab-upstream.driver=true" \
    merge --quiet --no-ff --no-commit origin/main \
    || exit_with 3 "main's concurrent commits conflict with the sync in ${wt}: resolve, git add, \
then alab/sync.sh --continue"
  reresolve_cargo_lock
  in_wt commit --quiet -m "alab(sync): merge main's concurrent commits"
}

push_main() {
  local before report rc delay=1
  while :; do
    before="$(git rev-parse origin/main)"
    rc=0
    report="$(LC_ALL=C in_wt push --porcelain origin HEAD:refs/heads/main)" || rc=$?
    [[ -z "${report}" ]] || printf '%s\n' "${report}"
    [[ "${rc}" -ne 0 ]] || return 0
    # A remote hook, authentication failure or transport error is not evidence of a race,
    # even if someone happened to advance main too. Git's per-ref porcelain distinguishes it.
    if ! awk -F '\t' '$1 == "!" && $2 == "HEAD:refs/heads/main" &&
      ($3 == "[rejected] (fetch first)" || $3 == "[rejected] (non-fast-forward)") { race = 1 }
      END { exit !race }' <<<"${report}"; then
      alab_die "push refused (above), not a proven concurrent update of main"
    fi
    git fetch --quiet origin || alab_die "fetching origin after the push race failed (above)"
    [[ "$(git rev-parse origin/main)" != "${before}" ]] \
      || alab_die "push refused (above), but origin/main did not advance"
    git merge-base --is-ancestor "${before}" origin/main \
      || alab_die "origin/main changed history during the push (above); a person must inspect it"
    rc=0
    in_wt merge-base --is-ancestor origin/main HEAD || rc=$?
    case "${rc}" in
      0) alab_die "push refused (above), though origin/main has nothing this sync lacks" ;;
      1) ;;
      *) alab_die "could not prove that origin/main has commits this sync lacks" ;;
    esac
    alab_say "main advanced during the push; retrying its merge and check after ${delay}s"
    sleep "${delay}"
    take_concurrent_main
    run_check
    delay=$((delay < 30 ? delay * 2 : 60))
  done
}

# Commit the merge (if still open), gate it, land it.
finish() {
  local before sync_merge=0 target
  merging || unlanded || alab_die "nothing to land in ${wt}: start a sync with alab/sync.sh"
  # A person's --continue that then dies on an error leaves the sync with that person; only an
  # unattended run that died is resumed by the next run.
  if [[ "${mode}" == continue ]]; then
    echo continue >"$(state_path stop)"
  else
    rm -f "$(state_path stop)"
  fi
  target="$(sync_target)"
  if merging; then
    if [[ "$(in_wt rev-parse MERGE_HEAD)" == "${target}" ]]; then
      # The upstream merge itself: redo what the sync resolves by itself, in case the run
      # that merged died before it did.
      [[ -f "$(state_path msg)" ]] || exit_with 3 "the sync in ${wt} lost its state file \
$(state_path msg): alab/sync.sh --abort, then sync again"
      sync_merge=1
      apply_removed_paths
      keep_owned_paths
    elif ! in_wt merge-base --is-ancestor MERGE_HEAD origin/main; then
      exit_with 3 "${wt} is merging $(in_wt rev-parse --short MERGE_HEAD), neither the sync's \
upstream commit nor main's: git merge --abort there, then alab/sync.sh --continue"
    elif [[ -f "$(state_path msg)" ]]; then
      # The sync merge under it was committed by hand and is not finished (commit_sync_merge).
      exit_with 3 "${wt} is merging main on top of a sync merge committed by hand: git merge \
--abort there, then alab/sync.sh --continue (it finishes the sync merge, then merges main)"
    fi
    [[ -z "$(unmerged)" ]] || {
      [[ "${sync_merge}" -eq 0 || -s "$(state_path hand)" ]] || unmerged >"$(state_path hand)"
      report_conflicts "$(in_wt rev-parse MERGE_HEAD)"
      stop 3 "conflicts for a resolver: $(unmerged | tr '\n' ' ')"
    }
    if [[ -n "$(comm -12 <(new_markers --cached HEAD) <(new_markers --cached MERGE_HEAD))" ]]; then
      exit_with 3 "conflict markers are staged in ${wt}: finish the resolution, then --continue"
    fi
    [[ -z "$(loose_changes)" ]] || exit_with 3 "unstaged or untracked changes in ${wt}: \
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
    if [[ "$(in_wt rev-parse -q --verify HEAD^2 || true)" != "${target}" ]]; then
      in_wt merge-base --is-ancestor "${target}" HEAD || exit_with 3 "the sync's merge of \
${target:0:9} is gone from ${wt} (aborted by hand?): alab/sync.sh --abort, then sync again"
      exit_with 3 "the sync merge in ${wt} was committed by hand and is not HEAD: fold the commits \
on top of it into it (git reset --soft <the merge>, git commit --amend), then alab/sync.sh --continue"
    fi
    [[ -z "$(loose_changes)" ]] || exit_with 3 "unstaged or untracked changes in ${wt}: \
stage them (git add) or discard them, then --continue"
    if [[ -n "$(comm -12 <(new_markers HEAD^1 HEAD) <(new_markers HEAD^2 HEAD))" ]]; then
      exit_with 3 "conflict markers are committed in the sync merge in ${wt}: finish the \
resolution, git commit --amend, then --continue"
    fi
    commit_sync_merge --amend
  fi
  [[ -z "$(in_wt status --porcelain)" ]] || exit_with 4 "uncommitted changes in ${wt}: commit \
the fix or discard it, then alab/sync.sh --continue"
  in_wt merge-base --is-ancestor origin/main HEAD || take_concurrent_main
  drift_check
  run_check
  if [[ "${push}" -eq 0 ]]; then
    if [[ "${check}" -eq 1 ]]; then
      echo no-push >"$(state_path stop)"
      alab_say "merged and checked in ${wt}, not pushed (--no-push): alab/sync.sh --continue lands it"
    else
      echo no-push-unchecked >"$(state_path stop)"
      alab_say "merged in ${wt}, neither checked nor pushed: alab/sync.sh --continue checks and lands it"
    fi
    return 0
  fi
  before="$(git rev-parse --short origin/main)"
  push_main
  git fetch --quiet origin
  rm -f "$(state_path target)" "$(state_path stop)"
  # A main nobody has checked out follows along; a checked-out main is its owner's to pull.
  if git worktree list --porcelain | has_line '^branch refs/heads/main$'; then
    alab_say "landed ${before}..$(git rev-parse --short origin/main); checkouts on main: git pull --ff-only"
  else
    if git merge-base --is-ancestor main origin/main 2>/dev/null; then
      git branch --quiet -f main origin/main
    fi
    alab_say "landed ${before}..$(git rev-parse --short origin/main) on main"
  fi
}

# merges_unattended <rev>: merging <rev> into origin/main leaves no conflict the sync cannot
# resolve by itself: every conflicted path is one removed-paths or owned-paths covers, by
# git's own pathspec matching (ls-files over the probe's tree in a throwaway index). A probe:
# git merge-tree writes no worktree, and the clone's index is not touched.
merges_unattended() {
  local out rc=0 list=() line idx uncovered
  out="$(git "${merge_cfg[@]}" -c core.quotepath=off merge-tree --write-tree --name-only \
    --no-messages origin/main "$1")" || rc=$?
  [[ "${rc}" -eq 0 ]] && return 0
  [[ "${rc}" -eq 1 ]] || return 1
  while IFS= read -r line; do list+=("${line}"); done < <(specs removed-paths; specs owned-paths)
  [[ ${#list[@]} -gt 0 ]] || return 1
  idx="${common_dir}/alab-probe.$$.index"
  GIT_INDEX_FILE="${idx}" git read-tree "$(printf '%s\n' "${out}" | head -n 1)" || {
    rm -f "${idx}"
    return 1
  }
  uncovered="$(comm -23 <(printf '%s\n' "${out}" | sed -e 1d -e '/^$/d' | sort -u) \
    <(GIT_INDEX_FILE="${idx}" git -c core.quotepath=off ls-files -- "${list[@]}" | sort -u))"
  rm -f "${idx}"
  [[ -z "${uncovered}" ]]
}

# Prints "<clean-prefix-or-empty>|<first-commit-needing-a-resolver>", or "<target>|" when
# the whole range merges unattended. Upstream's history is linear, so its first-parent
# commits are the steps; a binary search keeps the probes to a handful.
stepwise_plan() {
  local target="$1" commits=() c lo hi mid
  if merges_unattended "${target}"; then
    echo "${target}|"
    return
  fi
  while IFS= read -r c; do commits+=("${c}"); done \
    < <(git rev-list --reverse --first-parent "$(git merge-base origin/main "${target}")..${target}")
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

# Merge <target> into origin/main and land it (finish), or stop for a resolver (exit 3).
run_sync() {
  local target="$1" base pending merge_rc=0 merge_out
  if git merge-base --is-ancestor "${target}" origin/main; then
    alab_say "main already contains $(git log -1 --format='%h %cs' "${target}"): nothing to do"
    return 0
  fi
  in_wt clean -fdq
  in_wt checkout --quiet -B alab-sync origin/main
  base="$(git merge-base origin/main "${target}")"
  pending="$(git rev-list --count "origin/main..${target}")"
  {
    echo "alab sync: merge upstream openai/codex@$(git rev-parse --short=12 "${target}")"
    echo
    echo "Upstream: $(git log -1 --format='%H (%cs) %s' "${target}")"
    echo "New upstream commits: ${pending}"
  } >"$(state_path msg)"
  : >"$(state_path auto)"
  : >"$(state_path hand)"
  rm -f "$(state_path stop)"
  echo "${target}" >"$(state_path target)"
  note_watched_paths "${base}" "${target}"

  alab_say "merging ${pending} upstream commit(s) up to $(git log -1 --format='%h %cs %s' "${target}")"
  merge_out="$(in_wt "${merge_cfg[@]}" merge --no-ff --no-commit "${target}" 2>&1)" || merge_rc=$?
  if ! merging; then
    printf '%s\n' "${merge_out}" >&2
    clear_state
    alab_die "git merge stopped before merging (exit ${merge_rc}); ${wt} is unchanged"
  fi
  printf '%s\n' "${merge_out}" \
    | sed -E -n "s/^(Resolved|Staged) '(.*)' using previous resolution\.$/  rerere: \2/p" \
      >>"$(state_path auto)"

  apply_removed_paths
  keep_owned_paths
  if [[ -n "$(unmerged)" ]]; then
    unmerged >"$(state_path hand)"
    report_conflicts "${target}"
    stop 3 "conflicts for a resolver: $(unmerged | tr '\n' ' ')"
  fi
  finish
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
    if [[ ! -e "${wt}/.git" ]]; then
      echo "sync worktree  none yet (${wt})"
    elif merging || unlanded; then
      if merging; then
        echo "sync worktree  merging $(in_wt rev-parse --short MERGE_HEAD), $(waiting_hours)h in ${wt}"
        unmerged | sed 's/^/  unresolved: /'
      elif in_wt merge-base --is-ancestor origin/main HEAD; then
        echo "sync worktree  committed, not landed, $(waiting_hours)h: alab/sync.sh --continue (${wt})"
      else
        echo "sync worktree  committed, not landed, $(waiting_hours)h, origin/main moved since: \
--continue merges it in (${wt})"
      fi
      case "$(stop_code)" in
        3 | 4 | 6)
          echo "  stopped for a person (exit $(stop_code)):"
          sed -e 1d -e '/^$/d' -e 's/^/    /' "$(state_path stop)"
          ;;
        no-push) echo "  merged and checked with --no-push" ;;
        no-push-unchecked) echo "  merged with --no-push --no-check: not checked yet" ;;
        continue) echo "  a person's --continue stopped on an error (alab/sync.sh --continue again, or --abort)" ;;
        *) echo "  stopped on an error, not for a resolver: the next alab/sync.sh resumes it" ;;
      esac
    elif [[ -n "$(in_wt status --porcelain --untracked-files=no)" ]]; then
      echo "sync worktree  idle, with uncommitted changes: alab/sync.sh --abort discards them (${wt})"
    else
      echo "sync worktree  idle (${wt})"
    fi
    exit 0
    ;;
  abort)
    take_lock
    [[ -e "${wt}/.git" ]] || exit 0
    in_wt merge --abort >/dev/null 2>&1 || true
    in_wt reset --quiet --hard origin/main
    in_wt clean -fdq
    clear_state
    alab_say "sync abandoned; ${wt} is back at origin/main (rerere keeps what it learned)"
    exit 0
    ;;
  continue)
    take_lock
    [[ -e "${wt}/.git" ]] || alab_die "no sync worktree at ${wt}: start one with alab/sync.sh"
    finish
    exit 0
    ;;
esac

# mode=start
take_lock
git fetch --quiet upstream || alab_die "fetching upstream failed (above). If git rejected a \
non-fast-forward update of upstream/main, upstream rewrote its history: a person decides how \
ALab's layer moves onto it (alab/README.md, 'If upstream rewrites history')"
git fetch --quiet origin
git rev-parse -q --verify origin/main >/dev/null || alab_die "origin has no main branch to sync"
target="$(git rev-parse --verify "${ref}^{commit}")" || alab_die "no such upstream revision '${ref}'"
# A release tag is cut off upstream main (version bump, cherry-picks): merged here, its
# "version = X" and cherry-picks would stay in main through every later sync.
git merge-base --is-ancestor "${target}" upstream/main || alab_die "${ref} is not on upstream/main \
(a release tag? those are cut off main): main tracks upstream/main only (alab/README.md, 'Releases')"
# Whatever upstream commit main merged last must still be upstream history.
last_merged="$(git log -1 --first-parent --merges --grep='^alab sync:' --format=%P origin/main \
  | awk '{ print $2 }')"
[[ -z "${last_merged}" ]] || git merge-base --is-ancestor "${last_merged}" upstream/main \
  || alab_die "upstream/main no longer contains ${last_merged:0:9}, the upstream commit main \
merged last: upstream rewrote its history (alab/README.md, 'If upstream rewrites history')"
if git merge-base --is-ancestor "${target}" origin/main; then
  alab_say "main already contains ${ref} ($(git log -1 --format='%h %cs' "${target}")): nothing to do"
  exit 0
fi

ensure_worktree
if merging || unlanded; then
  case "$(stop_code)" in
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
  # Under --no-push it is still unlanded: syncing further would rebuild alab-sync from origin/main.
  [[ "${push}" -eq 1 ]] || exit 0
fi
[[ -z "$(in_wt status --porcelain --untracked-files=no)" ]] || alab_die "${wt} has uncommitted \
changes and no sync in progress: alab/sync.sh --abort discards them"

if [[ "${stepwise}" -eq 0 ]]; then
  run_sync "${target}"
  exit 0
fi
# Each round lands a clean prefix and then the first change that needs a resolver; a round
# whose change rerere resolved lands as well, and the next round goes on from there.
while ! git merge-base --is-ancestor "${target}" origin/main; do
  before="$(git rev-parse origin/main)"
  IFS='|' read -r prefix first <<<"$(stepwise_plan "${target}")"
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
