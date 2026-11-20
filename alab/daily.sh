#!/usr/bin/env bash
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
#
# alab/daily.sh — the unattended sync, for a LaunchAgent to run every few hours.
#
#   alab/daily.sh                  one stepwise sync (alab/sync.sh --stepwise), recorded
#   alab/daily.sh --status         the last recorded outcome
#   alab/daily.sh --launchd-plist  print a LaunchAgent for this clone. Installing it is yours:
#     alab/daily.sh --launchd-plist >~/Library/LaunchAgents/systems.alab.codex-alab-sync.plist
#     launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/systems.alab.codex-alab-sync.plist
#
# Each run keeps its output in <git-common-dir>/alab-daily.log (latest 20,000 lines and
# 4 MiB across all runs, with the latest run header) and rewrites one line in
# <git-common-dir>/alab-daily.status: when, exit status, origin/main, upstream/main,
# upstream commits still pending. A run that
# overlaps another daily run exits 75 without syncing or changing its status.
# Failures stay in the journal for another agent to revisit. Under the sync lock, a clean,
# idle sync checkout and its registered pristine checkouts are eligible for target cleanup.
# Before syncing, an eligible target larger than ALAB_TARGET_CAP_GB (default 40) is emptied.
# After each run, cleanup empties eligible pristine targets, then the eligible sync target
# when less than ALAB_KEEP_TARGET_GB (default 100) is free. Active Cargo locks keep a target.
# The sync's contained builds download crates into the clone's private cargo home
# (<git-common-dir>/alab-cargo-home, or ALAB_CARGO_HOME), not ~/.cargo; it is not capped.
# The plist starts a run at load and every ALAB_INTERVAL_SECONDS (default 14400); launchd
# skips an interval that falls during sleep. It carries PATH and HOME, and
# ALAB_SYNC_DIR, ALAB_CARGO, ALAB_ATERM, ALAB_ATPKG_PUBLISH, ATPKG_BIN, ALAB_TARGET_CAP_GB,
# ALAB_KEEP_TARGET_GB, ALAB_CONTAIN, ALAB_CARGO_HOME, ALAB_PYTHON, CARGO_BUILD_JOBS,
# CARGO_TARGET_DIR and RUST_TEST_THREADS when set.
set -euo pipefail

# Every lock re-entry, the sync child, and a generated LaunchAgent must select the
# same checkout as the caller, even when the caller supplied a relative path.
case "${ALAB_SYNC_DIR:-}" in "" | /*) ;; *) export ALAB_SYNC_DIR="${PWD}/${ALAB_SYNC_DIR}" ;; esac
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && printf '%s/' "${PWD}")"
repo_root="${repo_root%/}"
# shellcheck source=alab/lib.sh
. "${repo_root}/alab/lib.sh"
cd "${repo_root}"
# A parent Git command can name its own repository in the environment. Bind
# the scheduler's locks, records and sync child to the checkout of this script.
# Git's config parameters may carry transport settings used by the child.
local_git_vars="$(git rev-parse --local-env-vars)"
while IFS= read -r key; do
  case "${key}" in
    GIT_CONFIG | GIT_CONFIG_COUNT | GIT_CONFIG_PARAMETERS) ;;
    GIT_*) unset "${key}" ;;
  esac
done <<<"${local_git_vars}"
unset GIT_NAMESPACE GIT_QUARANTINE_PATH
selected="$(git rev-parse --show-toplevel && printf /)"
selected="${selected%$'\n'/}"
[[ "${selected}" -ef "${repo_root}" ]] || alab_die "Git selected a different scheduler checkout"
common_dir="$(git rev-parse --path-format=absolute --git-common-dir && printf /)"
common_dir="${common_dir%$'\n'/}"
[[ "${common_dir}" == /* && -d "${common_dir}" ]] || alab_die "cannot locate this clone's Git common directory"
wt="${ALAB_SYNC_DIR:-${common_dir}/alab-sync}"
log="${common_dir}/alab-daily.log"
status_file="${common_dir}/alab-daily.status"
label="systems.alab.codex-alab-sync"

xml() { sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g'; }

# Both cleanup phases run under the sync lock. Only a registered, available
# sync checkout of this clone with a landed HEAD and no merge is eligible.
cleanup_worktree_idle() {
  local key state admin changes rc=0
  # The lock and retention marker belong to this clone. Read its registrations
  # and the target's actual refs, independent of inherited Git redirects.
  for key in "${!GIT_@}"; do unset "${key}"; done
  export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 GIT_OPTIONAL_LOCKS=0
  export GIT_NO_REPLACE_OBJECTS=1 GIT_NO_LAZY_FETCH=1 GIT_TERMINAL_PROMPT=0
  # A standing preservation policy also avoids walking and sizing its targets.
  # drop_target checks it again immediately before each authorized removal.
  if ! "${ALAB_PYTHON:-/usr/bin/python3}" -I -B "${repo_root}/alab/retention.py" --repo "${repo_root}"; then
    alab_say "kept all target/ trees: cleanup policy preserves builds"
    return 1
  fi
  state="$("${ALAB_PYTHON:-/usr/bin/python3}" -I -B "${repo_root}/alab/sync_worktree.py" \
    "${common_dir}" "${wt}" start)" || return 1
  [[ "${state}" == present ]] || return 1
  # Process-use evidence names physical paths. Keep even a trailing newline in the path.
  wt="$(cd -P -- "${wt}" && printf '%s/' "${PWD}")" || return 1
  wt="${wt%/}"
  # --git-path MERGE_HEAD resolves a dangling link to its missing destination.
  # Inspect the entry inside the verified private registration itself.
  admin="$(git -C "${wt}" rev-parse --absolute-git-dir && printf /)" || return 1
  admin="${admin%$'\n'/}"
  [[ "${admin}" == /* && -d "${admin}" ]] || return 1
  if [[ -e "${admin}/MERGE_HEAD" || -L "${admin}/MERGE_HEAD" ]]; then
    alab_say "kept all target/ trees: an unlanded sync remains in ${wt}"
    return 1
  fi
  git -C "${wt}" merge-base --is-ancestor HEAD refs/remotes/origin/main || rc=$?
  case "${rc}" in
    0) ;;
    1) alab_say "kept all target/ trees: an unlanded sync remains in ${wt}"; return 1 ;;
    *) alab_say "kept all target/ trees: cannot read sync ancestry in ${wt} (git exit ${rc})"; return 1 ;;
  esac
  changes="$(git -C "${wt}" status --porcelain --untracked-files=normal)" || return 1
  if [[ -n "${changes}" ]]; then
    alab_say "kept all target/ trees: ${wt} has uncommitted or untracked files"
    return 1
  fi
  return 0
}

# Before a sync, reap over-cap targets belonging to its idle worktree.
reap_targets() {
  local tree target_dir size_kb
  cleanup_worktree_idle || return 0
  while IFS= read -r -d '' tree; do
    target_dir="${tree}/codex-rs/target"
    [[ -d "${target_dir}" && ! -L "${tree}/codex-rs" && ! -L "${target_dir}" ]] || continue
    size_kb="$(du -skx "${target_dir}" | awk 'NR == 1 { print $1 }')"
    if [[ "${size_kb}" -gt $((${ALAB_TARGET_CAP_GB:-40} * 1024 * 1024)) ]]; then
      # Manual builds hold no sync lock: use the same in-use check as final cleanup.
      drop_target "${target_dir%/codex-rs/target}" "${target_dir}" "the over-cap target"
    fi
  done < <(printf '%s\0' "${wt}"; alab_pristine_trees "${wt}")
}

# A manual build can keep using the tree after it releases its Cargo locks.
# Check process working directories and executables before authorizing removal.
in_use() {
  local tree="$1" target="$2" cwds progs
  cwds="$(lsof -a -d cwd -F n 2>/dev/null)" || return 0
  progs="$(ps -axo comm= 2>/dev/null)" || return 0
  # lsof -F n lines read n<path>: the tree itself, or a directory under it.
  awk -v t="n${tree}" '$0 == t || index($0, t "/") == 1 { found = 1 } END { exit !found }' \
    <<<"${cwds}" && return 0
  awk -v t="${target}/" 'index($0, t) == 1 { found = 1 } END { exit !found }' <<<"${progs}" && return 0
  return 1
}

# free_gb <dir>: whole GB free on the volume of <dir>.
free_gb() {
  df -k "$1" | awk 'NR == 2 { print int($4 / 1048576) }'
}

# drop_target <tree> <dir> <what>: empty the target/ <dir> of <tree> only when it is unused
# and the clone's shared retention policy permits cleanup, saying which.
drop_target() (
  local tree="$1" target="$2" what="$3" size_kb physical_tree prefix staged changed outcome
  local spec=':(top,literal)codex-rs/target'
  [[ -d "${target}" ]] || return 0
  if [[ -L "${tree}/codex-rs" || -L "${target}" ]] \
      || ! exec 7<"${tree}" 8<"${tree}/codex-rs" 9<"${target}" || in_use "${tree}" "${target}"; then
    echo "alab: kept ${what} ${target}: the target is linked or may be in use"
    return 0
  fi
  # Inspect the target relative to its physical workspace; the opened target
  # itself supplies the working directory for removing its children.
  cd -P -- "${tree}" || return 0
  physical_tree="${PWD}"
  cd -P -- codex-rs || return 0
  if [[ "${PWD}" != "${physical_tree}/codex-rs" || ! -d target || -L target ]]; then
    echo "alab: kept ${what} ${target}: cannot confirm its build directory"
    return 0
  fi
  # Git tracks case exactly even when the volume aliases these directory names.
  if [[ . -ef ../CODEX-RS || target -ef TARGET ]]; then
    spec=':(top,icase,literal)codex-rs/target'
  fi
  # A nested checkout or versioned files in target are not disposable build output.
  if [[ -e target/.git || -L target/.git ]] || ! prefix="$(git -C target rev-parse --show-prefix)" \
      || [[ "${prefix}" != codex-rs/target/ ]] \
      || ! staged="$(git ls-files --cached -- "${spec}")" || [[ -n "${staged}" ]] \
      || ! changed="$(git diff --cached --no-renames --no-ext-diff --no-textconv --ignore-submodules=none \
          --name-only HEAD -- "${spec}")" || [[ -n "${changed}" ]]; then
    echo "alab: kept ${what} ${target}: cannot establish that the target is untracked in this checkout"
    return 0
  fi
  size_kb="$(du -skx target | awk 'NR == 1 { print $1 }')" || return 0
  if ! "${ALAB_PYTHON:-/usr/bin/python3}" -IB "${repo_root}/alab/sync_worktree.py" \
      --check-target "${common_dir}" 7 8 9; then
    echo "alab: kept ${what} ${target}: could not confirm that the selected target is owned and unused"
    return 0
  fi
  if ! "${ALAB_PYTHON:-/usr/bin/python3}" -I "${repo_root}/alab/retention.py" --repo "${repo_root}"; then
    echo "alab: kept ${what} ${target}: cleanup policy preserves builds"
    return 0
  fi
  outcome="$("${ALAB_PYTHON:-/usr/bin/python3}" -IB "${repo_root}/alab/sync_worktree.py" \
      --clear-target 7 8 9)" || return $?
  if [[ "${outcome}" == cleared ]]; then
    echo "alab: emptied ${what} ${target} ($((size_kb / 1024 / 1024)) GB); $(free_gb .) GB free now"
  fi
)

# tidy_targets: after a run, landed or stopped: known pristine targets first, then the sync worktree's unless
# ALAB_KEEP_TARGET_GB (default 100) is free with the pristine one gone. Run only under the
# sync lock, so no sync builds in these targets while it goes; a person's own build is its lock.
tidy_targets() {
  local keep="${ALAB_KEEP_TARGET_GB:-100}" free pristine
  [[ "${keep}" =~ ^[0-9]{1,6}$ ]] || alab_die "ALAB_KEEP_TARGET_GB must be a whole number of GB (at most 6 digits), not '${keep}'"
  keep=$((10#${keep}))
  cleanup_worktree_idle || return 0
  while IFS= read -r -d '' pristine; do
    drop_target "${pristine}" "${pristine}/codex-rs/target" "the pristine tree's target"
  done < <(alab_pristine_trees "${wt}")
  [[ -d "${wt}/codex-rs/target" ]] || return 0
  free="$(free_gb "${wt}")"
  # An unreadable free space keeps the target: deleting is the step that cannot be undone.
  if [[ ! "${free}" =~ ^[0-9]+$ ]]; then
    echo "alab: kept the sync worktree's target ${wt}/codex-rs/target: free space unreadable"
  elif (( free >= keep )); then
    echo "alab: kept the sync worktree's target ${wt}/codex-rs/target: ${free} GB free, \
at least ALAB_KEEP_TARGET_GB=${keep}"
  else
    drop_target "${wt}" "${wt}/codex-rs/target" "the sync worktree's target"
  fi
}

case "${1:-}" in
  "")
    # Keep status reads and sync in one transaction. The separate
    # sync lock still coordinates builds and cleanup with manual sync invocations.
    alab_locked "${common_dir}/alab-daily.lockf" /bin/bash "${repo_root}/alab/daily.sh" --run-locked
    exit $?
    ;;
  --run-locked)
    # The producer owns the usual exit status. Drain all output and stage only its bounded tail.
    out="$(mktemp "${TMPDIR:-/tmp}/alab-daily.XXXXXX")"
    trap 'rm -f -- "${out}"' EXIT
    journal=("${ALAB_PYTHON:-/usr/bin/python3}" -IB "${repo_root}/alab/daily_journal.py")
    statuses=(0 0)
    /bin/bash "${repo_root}/alab/daily.sh" --collect-output 2>&1 \
      | "${journal[@]}" capture >"${out}" || statuses=("${PIPESTATUS[@]}")
    journal_rc="${statuses[1]}"
    if [[ "${journal_rc}" -eq 0 ]]; then
      "${journal[@]}" publish "${log}" "${out}" "${statuses[0]}" || journal_rc=$?
    fi
    [[ "${statuses[0]}" -eq 0 ]] || exit "${statuses[0]}"
    exit "${journal_rc}"
    ;;
  --collect-output) ;;
  --reap-locked)
    reap_targets
    exit 0
    ;;
  --tidy-locked)
    tidy_targets
    exit 0
    ;;
  --status)
    cat "${status_file}" 2>/dev/null || echo "no run recorded yet (${status_file})"
    exit 0
    ;;
  --launchd-plist)
    cat <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>${label}</string>
  <key>ProgramArguments</key>
  <array><string>/bin/bash</string><string>$(printf '%s' "${repo_root}/alab/daily.sh" | xml)</string></array>
  <key>StartInterval</key><integer>${ALAB_INTERVAL_SECONDS:-14400}</integer>
  <key>RunAtLoad</key><true/>
  <key>Nice</key><integer>10</integer>
  <key>LowPriorityIO</key><true/>
  <key>EnvironmentVariables</key>
  <dict>
    <key>PATH</key><string>$(printf '%s' "${PATH}" | xml)</string>
    <key>HOME</key><string>$(printf '%s' "${HOME}" | xml)</string>
$(for v in ALAB_SYNC_DIR ALAB_CARGO ALAB_ATERM ALAB_ATPKG_PUBLISH ATPKG_BIN ALAB_TARGET_CAP_GB ALAB_KEEP_TARGET_GB \
      ALAB_CONTAIN ALAB_CARGO_HOME ALAB_PYTHON CARGO_BUILD_JOBS CARGO_TARGET_DIR RUST_TEST_THREADS; do
      eval "val=\${${v}:-}"
      [[ -z "${val}" ]] || printf '    <key>%s</key><string>%s</string>\n' "${v}" "$(printf '%s' "${val}" | xml)"
    done)
  </dict>
  <key>StandardOutPath</key><string>$(printf '%s' "${common_dir}/alab-daily.launchd.log" | xml)</string>
  <key>StandardErrorPath</key><string>$(printf '%s' "${common_dir}/alab-daily.launchd.log" | xml)</string>
</dict>
</plist>
EOF
    exit 0
    ;;
  -h | --help)
    sed -n '5,/^set -euo pipefail$/p' "${repo_root}/alab/daily.sh" | sed -e '$d' -e 's/^# \{0,1\}//'
    exit 0
    ;;
  *) alab_die "unknown argument '$1' (see --help)" ;;
esac

# Reaping holds the sync lock: a sync holding it (a person's --continue, say) may be building
# in target/ right now (75: then nothing is reaped), and none can start while a target/ goes.
alab_locked "${common_dir}/alab-sync.lockf" /bin/bash "${repo_root}/alab/daily.sh" --reap-locked || true

rc=0
"${repo_root}/alab/sync.sh" --stepwise || rc=$?
# Then, holding the sync lock again, the target/ trees this run leaves (tidy_targets).
tidy_rc=0
alab_locked "${common_dir}/alab-sync.lockf" /bin/bash "${repo_root}/alab/daily.sh" --tidy-locked || tidy_rc=$?
case "${tidy_rc}" in
  0) ;;
  75) echo "alab: a sync holds the lock: no target/ removed after this run" ;;
  *) echo "alab: removing the target/ trees failed (exit ${tidy_rc}, above)" ;;
esac
# Status is a diagnostic record. No callback, notification or acknowledgement is needed.
printf 'when=%s exit=%s origin=%s upstream=%s pending=%s\n' \
  "$(date '+%Y-%m-%dT%H:%M:%S%z')" "${rc}" \
  "$(git rev-parse --short origin/main 2>/dev/null || echo -)" \
  "$(git rev-parse --short upstream/main 2>/dev/null || echo -)" \
  "$(git rev-list --count origin/main..upstream/main 2>/dev/null || echo -)" >"${status_file}.tmp" \
  && mv "${status_file}.tmp" "${status_file}"
exit "${rc}"
