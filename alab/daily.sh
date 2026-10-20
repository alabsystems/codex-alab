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
# Each run appends its output to <git-common-dir>/alab-daily.log and rewrites one line in
# <git-common-dir>/alab-daily.status: when, exit status, origin/main, upstream/main,
# upstream commits still pending, and since when the sync has needed someone. A run that
# needs someone runs the shell command in ALAB_NOTIFY with the exit status as $1 and the
# run's output on stdin (unset: a row on aterm's message band, else a macOS notification,
# taken down when a later run lands): on every exit 3, 4 or 6 (each is a new stop: a sync
# still waiting exits 5), on exit 1 twice running, and once on exit 5 when the wait passes
# ALAB_ESCALATE_HOURS (default 48). Before syncing, under the sync lock, and unless the sync
# worktree is merging or holds commits main lacks, a target/ of it or of its pristine tree
# larger than ALAB_TARGET_CAP_GB (default 40, each) is deleted. After each run, landed or
# stopped, again under the sync lock: the pristine tree's target/ is deleted, and then the
# sync worktree's too unless at least ALAB_KEEP_TARGET_GB (default 100) is free on its
# volume; a target/ a cargo build holds (its .cargo-lock) is kept. The log says what went.
# The sync's contained builds download crates into the clone's private cargo home
# (<git-common-dir>/alab-cargo-home, or ALAB_CARGO_HOME), not ~/.cargo; it is not capped.
# The plist starts a run at load and every ALAB_INTERVAL_SECONDS (default 14400); launchd
# skips an interval that falls during sleep. It carries PATH and HOME, and ALAB_NOTIFY,
# ALAB_SYNC_DIR, ALAB_CARGO, ALAB_ESCALATE_HOURS, ALAB_TARGET_CAP_GB, ALAB_KEEP_TARGET_GB,
# ALAB_CONTAIN and ALAB_CARGO_HOME when set.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=alab/lib.sh
. "${repo_root}/alab/lib.sh"
cd "${repo_root}"
common_dir="$(git rev-parse --path-format=absolute --git-common-dir)"
wt="${ALAB_SYNC_DIR:-${common_dir}/alab-sync}"
log="${common_dir}/alab-daily.log"
status_file="${common_dir}/alab-daily.status"
label="systems.alab.codex-alab-sync"

xml() { sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g'; }

# reap_targets: delete a target/ of the sync worktree or its pristine tree that is over the cap,
# unless a resolver may be building in the sync worktree (a sync is merging, or holds commits
# main lacks). Run only under the sync lock (below), so no sync builds in a target/ while it
# is deleted.
reap_targets() {
  local target_dir size_kb
  [[ -e "${wt:?}/.git" ]] || return 0
  ! git -C "${wt}" rev-parse -q --verify MERGE_HEAD >/dev/null 2>&1 || return 0
  [[ -z "$(git -C "${wt}" rev-list -1 origin/main..HEAD 2>/dev/null)" ]] || return 0
  for target_dir in "${wt}/codex-rs/target" "$(alab_pristine "${wt}")/codex-rs/target"; do
    [[ -d "${target_dir}" ]] || continue
    size_kb="$(du -sk "${target_dir}" | awk '{ print $1 }')"
    if [[ "${size_kb}" -gt $((${ALAB_TARGET_CAP_GB:-40} * 1024 * 1024)) ]]; then
      echo "alab: ${target_dir} is $((size_kb / 1024 / 1024)) GB, over the cap: deleting it" >>"${log}"
      rm -rf "${target_dir:?}"
    fi
  done
}

# cargo_building <target-dir>: a cargo build holds a lock in <target-dir> now (cargo flocks
# <profile>/.cargo-lock, and <triple>/<profile>/.cargo-lock, for the whole build): a build
# started by hand in the sync worktree, which the sync lock does not cover.
cargo_building() {
  local lock rc
  while IFS= read -r lock; do
    rc=0
    alab_locked "${lock}" /usr/bin/true || rc=$?
    [[ "${rc}" -ne 75 ]] || return 0
  done < <(find "$1" -maxdepth 3 -name .cargo-lock -type f 2>/dev/null)
  return 1
}

# in_use <tree> <target-dir>: something may still use <target-dir>: a cargo build holds its lock
# (cargo_building), or a process works in <tree> or runs a program under <target-dir>. cargo
# drops its lock once compiling ends, so a `cargo test` or `cargo run` started by hand (a
# resolver running alab/check.sh here) runs its binaries from target/ with no lock held; the
# process table is what still shows it. Unreadable (no lsof, no ps) is in use: keep it.
in_use() {
  local tree="$1" target="$2" cwds progs
  cargo_building "${target}" && return 0
  cwds="$(lsof -a -d cwd -F n 2>/dev/null)" || return 0
  progs="$(ps -axo comm= 2>/dev/null)" || return 0
  # lsof -F n lines read n<path>: the tree itself, or a directory under it.
  awk -v t="n${tree}" '$0 == t || index($0, t "/") == 1 { found = 1 } END { exit !found }' \
    <<<"${cwds}" && return 0
  awk -v t="${target}/" 'index($0, t) == 1 { found = 1 } END { exit !found }' <<<"${progs}" && return 0
  return 1
}

# resolver_waits: a sync waits for a person in the sync worktree (a merge is under way, or it
# holds commits main lacks): their build is theirs to keep.
resolver_waits() {
  git -C "${wt}" rev-parse -q --verify MERGE_HEAD >/dev/null 2>&1 && return 0
  [[ -n "$(git -C "${wt}" rev-list -1 origin/main..HEAD 2>/dev/null)" ]] && return 0
  return 1
}

# free_gb <dir>: whole GB free on the volume of <dir>.
free_gb() {
  df -k "$1" | awk 'NR == 2 { print int($4 / 1048576) }'
}

# drop_target <tree> <dir> <what>: delete the target/ <dir> of <tree> unless something may still
# use it (in_use), saying which.
drop_target() {
  local size_kb
  [[ -d "$2" ]] || return 0
  if in_use "$1" "$2"; then
    echo "alab: kept ${3} ${2}: a build or a process in ${1} may still use it"
    return 0
  fi
  set -- "$2" "$3"
  size_kb="$(du -sk "$1" | awk '{ print $1 }')"
  rm -rf "${1:?}"
  echo "alab: removed ${2} ${1} ($((size_kb / 1024 / 1024)) GB); $(free_gb "$(dirname "$1")") GB free now"
}

# tidy_targets: after a run, landed or stopped (both target/ trees stay on disk otherwise: 11
# and 14 GB on this Mac, 97% full): the pristine tree's target/ always (only a check builds
# there, and it is rebuilt from upstream at the next one), then the sync worktree's unless
# ALAB_KEEP_TARGET_GB (default 100) is free with the pristine one gone. Run only under the
# sync lock, so no sync builds in either while it goes; a person's own build is its lock.
tidy_targets() {
  local keep="${ALAB_KEEP_TARGET_GB:-100}" free pristine
  [[ "${keep}" =~ ^[0-9]{1,6}$ ]] || alab_die "ALAB_KEEP_TARGET_GB must be a whole number of GB (at most 6 digits), not '${keep}'"
  keep=$((10#${keep}))
  [[ -e "${wt:?}/.git" ]] || return 0
  if resolver_waits; then
    echo "alab: kept both target/ trees: a sync waits for a person in ${wt}"
    return 0
  fi
  pristine="$(alab_pristine "${wt}")"
  drop_target "${pristine}" "${pristine}/codex-rs/target" "the pristine tree's target"
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
  "") ;;
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
$(for v in ALAB_NOTIFY ALAB_SYNC_DIR ALAB_CARGO ALAB_ESCALATE_HOURS ALAB_TARGET_CAP_GB ALAB_KEEP_TARGET_GB \
      ALAB_CONTAIN ALAB_CARGO_HOME; do
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

# field <name>: a field of the previous status line, or nothing.
field() {
  [[ -f "${status_file}" ]] || return 0
  sed -n "s/.* $1=\([^ ]*\).*/\1/p" "${status_file}" | head -n 1
}
prev_exit="$(field exit)"
prev_since="$(field needs-someone-since)"
prev_notified="$(field notified)"
[[ "${prev_since}" != - ]] || prev_since=""
[[ "${prev_notified}" != - ]] || prev_notified=""

# Reaping holds the sync lock: a sync holding it (a person's --continue, say) may be building
# in target/ right now (75: then nothing is reaped), and none can start while a target/ goes.
alab_locked "${common_dir}/alab-sync.lockf" /bin/bash "${repo_root}/alab/daily.sh" --reap-locked || true

out="$(mktemp "${TMPDIR:-/tmp}/alab-daily.XXXXXX")"
trap 'rm -f "${out}"' EXIT
rc=0
"${repo_root}/alab/sync.sh" --stepwise >"${out}" 2>&1 || rc=$?
{
  echo "== $(date '+%Y-%m-%dT%H:%M:%S%z') alab/daily.sh: exit ${rc}"
  cat "${out}"
} >>"${log}"
# Then, holding the sync lock again, the target/ trees this run leaves (tidy_targets).
tidy_rc=0
alab_locked "${common_dir}/alab-sync.lockf" /bin/bash "${repo_root}/alab/daily.sh" --tidy-locked \
  >>"${log}" 2>&1 || tidy_rc=$?
case "${tidy_rc}" in
  0) ;;
  75) echo "alab: a sync holds the lock: no target/ removed after this run" >>"${log}" ;;
  *) echo "alab: removing the target/ trees failed (exit ${tidy_rc}, above)" >>"${log}" ;;
esac
# Keep the log to its newest 20000 lines.
if [[ "$(wc -l <"${log}")" -gt 20000 ]]; then
  tail -n 20000 "${log}" >"${log}.tmp" && mv "${log}.tmp" "${log}"
fi

now="$(date +%s)"
since=""
notify=""
case "${rc}" in
  0) ;;
  3 | 4 | 6)
    # sync.sh exits 3, 4 or 6 only when this very run stopped for a person (a sync still
    # waiting is exit 5), so each is a new state, even when a person landed the previous one
    # with --continue and this runner never saw an exit 0 in between.
    since="${now}"
    notify="exit${rc}@${since}"
    ;;
  5)
    since="${prev_since:-${now}}"
    if [[ $(((now - since) / 3600)) -ge "${ALAB_ESCALATE_HOURS:-48}" \
      && "${prev_notified}" != "waiting@${since}" ]]; then
      notify="waiting@${since}"
    fi
    ;;
  *)
    since="${prev_since}"
    [[ "${prev_exit}" != "${rc}" || "${prev_notified}" == "error@${prev_exit}" ]] || notify="error@${rc}"
    ;;
esac
notified="${notify:-${prev_notified}}"
[[ "${rc}" -ne 0 ]] || notified=""

# notify_default <exit>: what runs when ALAB_NOTIFY is unset: a row on aterm's message band
# (key codex-alab-sync), or, when no aterm answers, a macOS notification. The run's output is
# on stdin; its last "alab:" line is the reason.
notify_default() {
  local title why
  case "$1" in
    3) title="codex-alab sync has conflicts" ;;
    4) title="codex-alab sync check failed" ;;
    6) title="codex-alab upstream build broken" ;;
    5) title="codex-alab sync still waiting" ;;
    *) title="codex-alab sync keeps failing" ;;
  esac
  why="$(grep '^alab: ' | tail -n 1 | cut -c 7-400)"
  why="${why:-exit $1} (log: ${log}; alab/sync.sh --status)"
  aterm ctl notice post codex-alab sev=warn key=codex-alab-sync "${title}" -- "${why}" 2>/dev/null \
    | grep -q '^OK' && return 0
  /usr/bin/osascript -e 'on run argv' -e 'display notification (item 2 of argv) with title (item 1 of argv)' \
    -e 'end run' "${title}" "${why}"
}

if [[ -n "${notify}" ]]; then
  if [[ -n "${ALAB_NOTIFY:-}" ]]; then
    sh -c "${ALAB_NOTIFY}" alab-notify "${rc}" <"${out}" >>"${log}" 2>&1 \
      || echo "alab: ALAB_NOTIFY failed" >>"${log}"
  else
    notify_default "${rc}" <"${out}" >>"${log}" 2>&1 || echo "alab: notifying failed" >>"${log}"
  fi
elif [[ "${rc}" -eq 0 && -n "${prev_notified}" && -z "${ALAB_NOTIFY:-}" ]]; then
  # Landed after a stop it reported: take the row down.
  aterm ctl notice done codex-alab-sync withdraw >/dev/null 2>&1 || true
fi

printf 'when=%s exit=%s origin=%s upstream=%s pending=%s needs-someone-since=%s notified=%s\n' \
  "$(date '+%Y-%m-%dT%H:%M:%S%z')" "${rc}" \
  "$(git rev-parse --short origin/main 2>/dev/null || echo -)" \
  "$(git rev-parse --short upstream/main 2>/dev/null || echo -)" \
  "$(git rev-list --count origin/main..upstream/main 2>/dev/null || echo -)" \
  "${since:--}" "${notified:--}" >"${status_file}"
exit "${rc}"
