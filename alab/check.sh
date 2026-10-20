#!/usr/bin/env bash
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
#
# alab/check.sh — what must hold before alab/sync.sh lands a merge on main:
#   1. no ALab hook went missing: each ALAB(<topic>) marker origin/main has in an upstream
#      file is still there (ALAB_CHECK_BASE names another base; ALAB_ALLOW_HOOK_LOSS=1
#      accepts a loss the resolver meant, such as a hook upstream made redundant)
#   2. the codex binary builds from the committed lockfile and runs
#   3. every ALab-owned crate (codex-rs/alab-*) passes its tests
#   4. every upstream package ALab hooks into passes its unit tests (--lib), where the
#      layer is to blame: upstream runs them with nextest, a process per test, and libtest's
#      process per crate fails some on upstream alone (a stack overflow, timing under load,
#      insta's one-assertion-per-process rule, a test that needs a binary --lib does not
#      build). So a failure is rerun alone; one that still fails is rerun alone on pristine
#      upstream (git merge-base HEAD upstream/main, in this checkout's pristine tree and its
#      own target dir: lib.sh, alab_pristine). It gates unless it fails there too.
# Upstream's integration suites stay upstream's gate: several need the network or are
# timing-sensitive, and they cannot tell an ALab fault from an upstream one.
#
# It takes no options: any other argument than --help is refused. Codex and its tests run
# with a scratch CODEX_HOME, never the ~/.codex that holds this machine's login, and builds
# skip incremental caches and debug info (CARGO_INCREMENTAL=0, CARGO_PROFILE_DEV_DEBUG=0): a
# sync recompiles the workspace anyway, and with them the hooked crates' test builds grew one
# target/ to 52 GB (this Mac's disk ran out). It raises its open-file limit first: launchd
# starts alab/daily.sh with a soft limit of 256, under which libtest's parallel tests fail with
# "Too many open files" (829 codex-tui tests did, 2026-10-16).
#
# Exit status: 0 holds; 2 step 2 failed (alab/sync.sh then builds pristine upstream to tell
# an ALab fault from a toolchain fault); 1 any other step failed.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=alab/lib.sh
. "${repo_root}/alab/lib.sh"
if [[ $# -eq 1 && ( "$1" == -h || "$1" == --help ) ]]; then
  sed -n '5,/^set -euo pipefail$/p' "${repo_root}/alab/check.sh" | sed '$d' | sed 's/^# \{0,1\}//'
  exit 0
fi
[[ $# -eq 0 ]] || alab_die "alab/check.sh takes no arguments (see --help)"
cargo="${ALAB_CARGO:-targo}"
cd "${repo_root}"

scratch="$(mktemp -d "${TMPDIR:-/tmp}/alab-check.XXXXXX")"
trap 'rm -rf "${scratch}"' EXIT
mkdir "${scratch}/codex-home"
export CODEX_HOME="${scratch}/codex-home"
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0
# The soft open-file limit, up to what the kernel allows one process (launchd's 256 is far too
# few for a test run). A limit that cannot be raised that far is said, not hidden.
nofile_want="$(sysctl -n kern.maxfilesperproc 2>/dev/null || echo 10240)"
(( nofile_want > 65536 )) && nofile_want=65536
if (( $(ulimit -n) < nofile_want )) && ! ulimit -n "${nofile_want}" 2>/dev/null; then
  alab_say "open-file limit stays $(ulimit -n) (could not raise it to ${nofile_want}): tests may fail with EMFILE"
fi

# hook_counts <rev>: "<count> <marker>" per distinct marker in upstream-owned files. git grep
# exits 1 for no match; anything else (an object it cannot read) is no count at all.
hook_counts() {
  local found rc=0
  found="$(git grep -h -o -E "${ALAB_MARKER_RE}" "$1" -- ':!alab' ':!AGENTS.md' ':!codex-rs/alab-*')" \
    || rc=$?
  [[ "${rc}" -le 1 ]] || alab_die "git grep for ALab's hooks failed on $1 (exit ${rc})"
  [[ -z "${found}" ]] || printf '%s\n' "${found}" | sort | uniq -c
}

base="${ALAB_CHECK_BASE:-origin/main}"
git rev-parse -q --verify "${base}^{commit}" >/dev/null || alab_die "no commit '${base}' to compare \
ALab's hooks with (git fetch origin, or name one in ALAB_CHECK_BASE)"
if [[ "$(git rev-parse HEAD)" != "$(git rev-parse "${base}^{commit}")" ]]; then
  now_counts="$(hook_counts HEAD)"
  base_counts="$(hook_counts "${base}")"
  lost="$(awk 'NR == FNR { now[$2] = $1; next } ($2 in now ? now[$2] : 0) < $1 {
      print "  " $2 " " $1 " -> " ($2 in now ? now[$2] : 0) }' \
    <(printf '%s\n' "${now_counts}") <(printf '%s\n' "${base_counts}"))"
  if [[ -n "${lost}" && "${ALAB_ALLOW_HOOK_LOSS:-0}" != 1 ]]; then
    printf 'alab/check.sh: ALab hooks went missing since %s (marker: count there -> here):\n%s\n' \
      "${base}" "${lost}" >&2
    echo "Re-apply each where upstream moved the code (git grep -n 'ALAB' ${base} shows where it was)," >&2
    echo "or, if upstream made one redundant, rerun with ALAB_ALLOW_HOOK_LOSS=1." >&2
    exit 1
  fi
fi

cd "${repo_root}/codex-rs"
# passed <status> <exit>: exit <exit> for a failed step, but a cargo killed by a signal (memory
# pressure, Ctrl-C) gave no verdict: its own status goes up, and alab/sync.sh checks again later.
passed() { [[ "$1" -le 128 ]] || exit "$1"; exit "$2"; }
"${cargo}" --unverified run --quiet --locked -p codex-cli --bin codex -- --version || passed $? 2

# package_name <manifest>: the [package] name, or nothing for a virtual manifest.
package_name() {
  awk '/^\[package\]/ { p = 1; next } /^\[/ { p = 0 }
    p && /^[[:space:]]*name[[:space:]]*=/ { sub(/^[^"]*"/, ""); sub(/".*$/, ""); print; exit }' "$1"
}

alab_packages=()
for manifest in alab-*/Cargo.toml; do
  [[ -f "${manifest}" ]] || continue
  name="$(package_name "${manifest}")"
  [[ -n "${name}" ]] || {
    echo "alab/check.sh: no package name in codex-rs/${manifest}" >&2
    exit 1
  }
  alab_packages+=(-p "${name}")
done
if [[ ${#alab_packages[@]} -gt 0 ]]; then
  "${cargo}" --unverified test --locked --no-fail-fast "${alab_packages[@]}" || passed $? 1
fi

# The crate around each hooked upstream file, if it has a library to unit-test.
hooked_packages=()
hooked_dirs=()
while IFS= read -r dir; do
  [[ -f "${dir}/src/lib.rs" ]] || grep -q '^\[lib\]' "${dir}/Cargo.toml" || continue
  name="$(package_name "${dir}/Cargo.toml")"
  [[ -n "${name}" ]] || continue
  hooked_packages+=(-p "${name}")
  hooked_dirs+=("${dir}")
done < <(
  { git grep -l -E "${ALAB_MARKER_RE}" -- . ':!alab-*' || true; } | while IFS= read -r file; do
    dir="$(dirname "${file}")"
    while [[ "${dir}" != . && ! -f "${dir}/Cargo.toml" ]]; do dir="$(dirname "${dir}")"; done
    [[ "${dir}" != . ]] && echo "${dir}"
  done | sort -u
)
[[ ${#hooked_packages[@]} -gt 0 ]] || exit 0

# unit_tests <tree> <target-dir> [libtest args...]: the unit tests of the packages in the array
# `packages` in <tree>, as upstream's `just test` runs them where it matters: an 8 MiB stack (a
# codex-core test overflows the default 2 MiB, which aborts every test in that crate) and every
# package even after one fails.
unit_tests() {
  local tree="$1" target_dir="$2"
  shift 2
  (cd "${tree}/codex-rs" && RUST_MIN_STACK=8388608 CARGO_TARGET_DIR="${target_dir}" \
    "${cargo}" --unverified test --locked --no-fail-fast --lib "${packages[@]}" -- "$@")
}
# reported <log> <test> <ok|FAILED>: the log says that test ran with that result. Tests that
# draw a TUI can leave escape bytes before the line, so it is matched anywhere in a line.
reported() { [[ "$(copies "$@")" != 0 ]]; }
# copies <log> <test> <ok|FAILED>: how many times the log reports that test with that result.
copies() { LC_ALL=C grep -a -c -F "test $2 ... $3" "$1" || true; }
# failing_in <log> <test>: the crates whose copy of that test the log reports FAILED, one per
# line: cargo names each test binary (".../deps/<crate>-<hash>") before its results, and two
# packages can share a test name.
failing_in() {
  LC_ALL=C awk -v t="test $2 ... FAILED" '/Running unittests / { c = $0; sub(/.*deps\//, "", c)
    sub(/-[0-9a-f]+\).*$/, "", c) } index($0, t) { print c }' "$1" | sort -u
}

own_target="${CARGO_TARGET_DIR:-${repo_root}/codex-rs/target}"
packages=("${hooked_packages[@]}")
log="${scratch}/unit.log"
rc=0
unit_tests "${repo_root}" "${own_target}" 2>&1 | tee "${log}" || rc=$?
[[ "${rc}" -eq 0 ]] && exit 0
[[ "${rc}" -le 128 ]] || exit "${rc}"
failed=()
while IFS= read -r t; do failed+=("${t}"); done < <(LC_ALL=C grep -a -o -E \
  'test [A-Za-z_][A-Za-z0-9_:]* \.\.\. FAILED' "${log}" | awk '{ print $2 }' | sort -u)
counted="$({ LC_ALL=C grep -a -o -E 'test result: FAILED\. [0-9]+ passed; [0-9]+ failed' "${log}" \
  || true; } | awk '{ n += $6 } END { print n + 0 }')"
# A crash or a build error names no test, and a reading that disagrees with libtest's own count
# cannot be trusted: either way the result stands, unexplained.
# Each FAILED line, not each name: two packages can share a test name.
failed_lines="$({ LC_ALL=C grep -a -o -E 'test [A-Za-z_][A-Za-z0-9_:]* \.\.\. FAILED' "${log}" || true; } | wc -l | tr -d ' ')"
if [[ "${counted}" -eq 0 || "${counted}" -ne "${failed_lines}" || "${counted}" -gt 60 ]] \
  || LC_ALL=C grep -a -q -E "process didn't exit successfully: .*\(signal: " "${log}"; then
  echo "alab/check.sh: the unit tests of ${hooked_packages[*]} failed (above), and not test by test" >&2
  exit 1
fi

alone=() still=() still_crates=()
for t in "${failed[@]}"; do
  if unit_tests "${repo_root}" "${own_target}" --exact "${t}" >"${scratch}/alone.log" 2>&1 \
    && reported "${scratch}/alone.log" "${t}" ok; then
    alone+=("${t}")
  else
    # Which packages' copies of the name fail alone (two packages can share a test name).
    still+=("${t}") still_crates+=("$(failing_in "${scratch}/alone.log" "${t}" | tr '\n' ' ')")
  fi
done
[[ ${#alone[@]} -eq 0 ]] || printf 'alab/check.sh: passes alone, as upstream runs it (not gating): %s\n' "${alone[@]}" >&2
[[ ${#still[@]} -gt 0 ]] || exit 0

upstream_base="$(git merge-base HEAD upstream/main 2>/dev/null || true)"
if [[ -z "${upstream_base}" ]]; then
  printf 'alab/check.sh: fails alone, and no upstream/main to compare with (alab/setup.sh): %s\n' "${still[@]}" >&2
  exit 1
fi
pristine="$(alab_pristine "${repo_root}")"
echo "alab/check.sh: rerunning ${#still[@]} test(s) on pristine upstream ${upstream_base:0:9} in ${pristine}" >&2
alab_pristine_at "${repo_root}" "${upstream_base}" || {
  echo "alab/check.sh: pristine upstream ${upstream_base:0:9} could not be checked out at ${pristine}" >&2
  exit 1
}
# Only the hooked packages upstream has too: one ALab added or upstream dropped would stop
# the run before any test, and every failure would then gate.
packages=()
for i in "${!hooked_dirs[@]}"; do
  [[ -f "${pristine}/codex-rs/${hooked_dirs[$i]}/Cargo.toml" \
    && "$(package_name "${pristine}/codex-rs/${hooked_dirs[$i]}/Cargo.toml")" == "${hooked_packages[$((2 * i + 1))]}" ]] \
    && packages+=(-p "${hooked_packages[$((2 * i + 1))]}")
done
gating=0
for i in "${!still[@]}"; do
  t="${still[$i]}"
  if [[ ${#packages[@]} -gt 0 ]]; then
    unit_tests "${pristine}" "${pristine}/codex-rs/target" --exact "${t}" >"${scratch}/pristine.log" 2>&1 || true
  else
    : >"${scratch}/pristine.log"
  fi
  # Upstream's own failure: each package's copy that fails with ALab's layer fails there too (a
  # name two packages share can fail in one on both sides and be broken by ALab in the other).
  # When the alone run named no failing copy, any copy failing there counts.
  own=0
  if [[ -n "${still_crates[$i]}" ]]; then
    [[ -n "$(comm -23 <(printf '%s\n' ${still_crates[$i]} | sort -u) <(failing_in "${scratch}/pristine.log" "${t}"))" ]] || own=1
  elif reported "${scratch}/pristine.log" "${t}" FAILED; then
    own=1
  fi
  if [[ "${own}" -eq 1 ]]; then
    why="fails on upstream ${upstream_base:0:9} alone too (not gating)"
  elif reported "${scratch}/pristine.log" "${t}" ok; then
    why="fails with ALab's layer, passes on upstream ${upstream_base:0:9}"
    gating=1
  else
    tail -n 20 "${scratch}/pristine.log" >&2
    why="fails with ALab's layer, and did not run on upstream ${upstream_base:0:9} (above)"
    gating=1
  fi
  printf 'alab/check.sh: %s: %s\n' "${why}" "${t}" >&2
done
exit "${gating}"
