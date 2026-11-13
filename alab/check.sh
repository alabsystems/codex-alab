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
#   4. tippy finds no warning or error in ALab's code: the alab-* crates, the glue upstream
#      crates compile (codex-rs/alab-*/glue), the sources alab/ carries (alab/vendor), and
#      each upstream line that carries an ALAB marker. It lints those crates and every hooked
#      upstream package, all targets, in a target dir of its own (<target>/tippy), and sets
#      aside what it finds in upstream's code, which stays upstream's to fix. A diagnostic
#      whose span takes in a hook line it does not start on, a target that does not build,
#      and a build script of one of those crates that does not run (which leaves the whole
#      crate unlinted, and which cargo's JSON does not report), are judged against pristine
#      upstream (as in 5): each gates unless upstream alone has it too
#   5. every upstream package ALab hooks into passes its unit tests (--lib), where the
#      layer is to blame: upstream runs them with nextest, a process per test, and libtest's
#      process per crate fails some on upstream alone (a stack overflow, timing under load,
#      insta's one-assertion-per-process rule). Native runtime helpers are prepared before
#      testing. A failure is rerun alone; one that still fails is rerun alone on pristine
#      upstream (git merge-base HEAD upstream/main, in this checkout's pristine tree and its
#      own target dir: lib.sh, alab_pristine). It gates unless it fails there too.
# Newly integrated upstream commits also need committed intake reviews and complete
# declared execution evidence for every affected feature's scenarios.
# Execution order: hooks, intake/evidence admission, full tippy classification, CLI build/version, ALab tests,
# then hooked upstream unit tests. Check numbers above retain their existing identities.
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

# The sourceable body remains useful for fixture tests. The executable entry
# always obtains native authority before this body can start a compiler.
check_main() {
if [[ $# -eq 1 && ( "$1" == -h || "$1" == --help ) ]]; then
  sed -n '5,/^set -euo pipefail$/p' "${repo_root}/alab/check.sh" | sed '$d' | sed 's/^# \{0,1\}//'
  exit 0
fi
[[ $# -eq 0 ]] || alab_die "alab/check.sh takes no arguments (see --help)"
cargo="${ALAB_CARGO:-targo}"
cd "${repo_root}"

# Keep socket fixtures below macOS's 104-byte sun_path limit. Under the
# maintained sandbox, stay within its already admitted private temporary root.
# Direct invocations get a fresh private leaf in the non-indexed scratch area.
if alab_contained_scratch_path "${TMPDIR:-}"; then
  scratch="$(mktemp -d "${TMPDIR%/}/t.XXXXXX")"
else
  mkdir -p /tmp/.ubd || alab_die "cannot prepare the temporary scratch area"
  [[ -d /tmp/.ubd && ! -L /tmp/.ubd ]] || alab_die "temporary scratch area must be a real directory"
  scratch="$(mktemp -d /tmp/.ubd/alab.XXXXXX)"
fi
export TMPDIR="${scratch}"
check_scratch_cleanup() {
  if "${ALAB_PYTHON:-/usr/bin/python3}" -I "${repo_root}/alab/retention.py" --repo "${repo_root}"; then
    rm -rf "${scratch:?}"
  else
    alab_say "kept gate scratch ${scratch}: cleanup policy preserves builds"
  fi
}
trap check_scratch_cleanup EXIT
unit_root="${scratch}"
if [[ -n "${ALAB_UNIT_RESULTS_DIR:-}" ]]; then
  [[ "${ALAB_UNIT_RESULTS_DIR}" == /* ]] || alab_die "ALAB_UNIT_RESULTS_DIR must be absolute"
  # Exclusive creation never overwrites an earlier run or the person's files.
  mkdir -m 700 "${ALAB_UNIT_RESULTS_DIR}" || alab_die "cannot create fresh unit-results directory ${ALAB_UNIT_RESULTS_DIR}"
  unit_root="${ALAB_UNIT_RESULTS_DIR}"
fi
mkdir "${scratch}/codex-home"
export CODEX_HOME="${scratch}/codex-home"
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0
# A fresh launchd job has no machine allocation. Bound native builds and tests
# there too, while retaining each nonempty limit an admitting caller supplied.
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}" RUST_TEST_THREADS="${RUST_TEST_THREADS:-4}"
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

intake_target="$(git merge-base HEAD upstream/main)" \
  || alab_die "cannot identify the integrated upstream frontier before review admission"
intake_base="$(git rev-parse -q --verify refs/remotes/origin/main^{commit})" \
  || alab_die "no origin/main for upstream review admission"
new_upstream="$(git rev-list -1 "${intake_base}..${intake_target}")" \
  || alab_die "cannot determine which upstream commits need review admission"
if [[ -n "${new_upstream}" ]]; then
  intake_python="$(alab_python_311)" || alab_die "upstream review admission needs a TOML-capable interpreter"
  "${intake_python}" -I "${repo_root}/alab/upstream_gate.py" \
    --repo "${repo_root}" --base "${intake_base}" --source HEAD --upstream "${intake_target}" \
    || alab_die "upstream review admission failed; commit complete intake decisions before checking again"
  "${intake_python}" -I "${repo_root}/alab/feature_gate.py" \
    --repo "${repo_root}" --base "${intake_base}" --source HEAD --upstream "${intake_target}" \
    || alab_die "feature evidence admission failed; complete the exact-source affected-feature bundle before checking again (alab/design/upstream-alignment.md)"
fi

cd "${repo_root}/codex-rs"
# passed <status> <exit>: exit <exit> for a failed step, but a cargo killed by a signal (memory
# pressure, Ctrl-C) gave no verdict: its own status goes up, and alab/sync.sh checks again later.
# Disk resource stops (75) and signals decide nothing about either source tree.
stopped() { [[ "$1" -ne 75 && "$1" -le 128 ]] || exit "$1"; }
passed() { stopped "$1"; exit "$2"; }
# Resolve Cargo's own config/env precedence before the first compiler starts.
# Every phase must present the same explicit target to the native owner gate:
# making an implicit target explicit only for unit helpers fails after earlier
# phases have already filled it with unowned outputs.
command -v jq >/dev/null 2>&1 || alab_die "alab/check.sh reads Cargo metadata and tippy's messages with jq, which is not installed"
rc=0
alab_build_command "${cargo}" metadata --locked --no-deps --format-version 1 >"${scratch}/metadata.json" || rc=$?
stopped "${rc}"
[[ "${rc}" -eq 0 ]] || alab_die "${cargo} metadata failed (exit ${rc}): no compiler started"
own_target="$(jq -e -r '.target_directory | strings | select(length > 0)' "${scratch}/metadata.json")" \
  || alab_die "${cargo} metadata did not name a target directory; no compiler started"
export CARGO_TARGET_DIR="${own_target}"
# pristine_upstream: put this checkout's pristine tree (lib.sh, alab_pristine) at the upstream
# commit HEAD is built on (git merge-base HEAD upstream/main), and set upstream_base and
# pristine. When it cannot, it says why and fails.
pristine_upstream() {
  local config
  config="$(alab_pristine_config "${repo_root}")" || return 1
  pristine_config=()
  [[ -z "${config}" ]] || pristine_config=(--config "${config}")
  upstream_base="$(git merge-base HEAD upstream/main 2>/dev/null || true)"
  if [[ -z "${upstream_base}" ]]; then
    echo "alab/check.sh: no upstream/main to compare with (alab/setup.sh)" >&2
    return 1
  fi
  pristine="$(alab_pristine "${repo_root}" "${upstream_base}")" || return 1
  alab_pristine_at "${repo_root}" "${upstream_base}" && return 0
  echo "alab/check.sh: pristine upstream ${upstream_base:0:9} could not be checked out at ${pristine}" >&2
  return 1
}

pristine_upstream || alab_die "cannot prepare pristine dependency graph; no compiler started"
alab_check_dependencies_ready "${repo_root}" "${pristine}" || exit $?

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

# The crate around each hooked upstream file: tippy lints every one (step 4), and each with a
# library to unit-test runs its unit tests (step 5).
lint_packages=(${alab_packages[@]+"${alab_packages[@]}"})
lint_names=()
for ((i = 1; i < ${#alab_packages[@]}; i += 2)); do lint_names+=("${alab_packages[$i]}"); done
hooked_packages=()
hooked_dirs=()
while IFS= read -r dir; do
  name="$(package_name "${dir}/Cargo.toml")"
  [[ -n "${name}" ]] || continue
  lint_packages+=(-p "${name}")
  lint_names+=("${name}")
  [[ -f "${dir}/src/lib.rs" ]] || grep -q '^\[lib\]' "${dir}/Cargo.toml" || continue
  hooked_packages+=(-p "${name}")
  hooked_dirs+=("${dir}")
done < <(
  { git grep -l -E "${ALAB_MARKER_RE}" -- . ':!alab-*' || true; } | while IFS= read -r file; do
    dir="$(dirname "${file}")"
    while [[ "${dir}" != . && ! -f "${dir}/Cargo.toml" ]]; do dir="$(dirname "${dir}")"; done
    [[ "${dir}" != . ]] && echo "${dir}"
  done | sort -u
)

# shellcheck source=alab/unit-tests.sh
. "${repo_root}/alab/unit-tests.sh"

# lint <tree> <target-dir> <-p package>...: tippy's JSON messages for those packages in <tree>,
# all targets. --cap-lints warn keeps a lint upstream denies (needless_late_init in codex-tui)
# from failing a crate, which would leave every crate built on it unlinted, and --keep-going
# lints every target one failed build leaves buildable.
lint() {
  local tree="$1" target_dir="$2" v8 archive binding
  shift 2
  v8="$(native_v8_env "${tree}")" || return $?
  archive="$(printf '%s\n' "${v8}" | jq -e -r '.RUSTY_V8_ARCHIVE | strings | select(length > 0)')" || return 1
  binding="$(printf '%s\n' "${v8}" | jq -e -r '.RUSTY_V8_SRC_BINDING_PATH | strings | select(length > 0)')" || return 1
  (cd "${tree}/codex-rs" && CARGO_TARGET_DIR="${target_dir}" \
    RUSTY_V8_ARCHIVE="${archive}" RUSTY_V8_SRC_BINDING_PATH="${binding}" alab_build_command "${cargo}" tippy --locked \
    --keep-going --no-deps --all-targets --message-format=json "$@" -- --cap-lints warn)
}

# lint_verdict <json> <pristine-json> <compared>: alab/lint.jq's reading of <json> (ALab's
# tree), with <pristine-json> from pristine upstream when <compared> is true, into verdict.json.
lint_verdict() {
  local roots=("$(cd "${repo_root}" && pwd -P)/" "${repo_root}/") proots=()
  [[ -z "${pristine:-}" ]] || proots=("$(cd "${pristine}" && pwd -P)/" "${pristine}/")
  jq -R -n -c -f "${repo_root}/alab/lint.jq" --rawfile hooks "${scratch}/hook-lines" \
    --rawfile pristine "$2" --argjson roots "$(jq -n -c '$ARGS.positional' --args "${roots[@]}")" \
    --argjson proots "$(jq -n -c '$ARGS.positional' --args ${proots[@]+"${proots[@]}"})" \
    --arg ws codex-rs/ --argjson compared "$3" --argjson scripted "${scripted}" \
    <"$1" >"${scratch}/verdict.json" \
    || alab_die "alab/check.sh could not read tippy's messages (alab/lint.jq failed)"
}
# verdict <filter>: that jq filter's raw output on verdict.json.
verdict() { jq -r "$1" "${scratch}/verdict.json"; }
# script_lines <suffix> <filter>: a line for each build script in verdict.json's .scripts that
# the jq <filter> selects, saying it did not run, then <suffix>.
script_lines() {
  while IFS=$'\t' read -r manifest built; do
    if [[ "${built}" == true ]]; then
      echo "alab/check.sh: the build script of ${manifest} was built and did not run to completion (cargo's error above names it)$1"
    else
      echo "alab/check.sh: the build script of ${manifest} was never built (something it needs failed, above)$1"
    fi
  done < <(verdict ".scripts[] | select($2) | [.manifest, .built] | @tsv")
}

# 4. tippy lints ALab's crates and every hooked upstream crate (the ones that compile glue,
# codex-tui, codex-core and codex-cli, among them), all targets, tests included, in a target
# dir of its own: tippy's rustc fingerprint differs from the build's, so in a shared one the
# build-script units both use would be rebuilt, with everything built on them, each time one
# followed the other. What gates is ALab's code (alab/lint.jq): cargo's JSON names each
# diagnostic's file and lines, so no prose is read, and upstream's own diagnostics are
# upstream's to fix. A diagnostic whose span takes in a hook line it does not start on (a
# method chain is reported from its receiver), and a target that fails to build with no error
# in ALab's code, are judged against pristine upstream: each gates unless upstream alone has
# as many of that diagnostic, or fails to build that target too. So is a build script of a
# linted package (ALab's or a hooked one) that does not run: cargo's JSON records each script
# that ran and none that failed, the package's targets are then never built, and so no
# diagnostic names it; with another target failing on upstream too, nothing else would gate.
if [[ ${#lint_packages[@]} -gt 0 ]]; then
  # The linted packages with a build script, as cargo itself finds them (build.rs, or the
  # [package] build key). Through a file in the scratch dir, never a here-document: macOS's
  # bash 3.2 writes those to the system temp dir, not TMPDIR, which alab_contained does not
  # let a contained step write ("cannot create temp file for here document").
  scripted="$(jq -c '[.packages[] | select(.name | IN($ARGS.positional[]))
      | select(any(.targets[]; .kind | index("custom-build"))) | .manifest_path]' \
    --args ${lint_names[@]+"${lint_names[@]}"} <"${scratch}/metadata.json")" \
    || alab_die "alab/check.sh could not read ${cargo} metadata's packages"
  rc=0
  git grep -n --full-name -o -E "${ALAB_MARKER_RE}" -- . ':!alab-*' >"${scratch}/hook-lines" || rc=$?
  [[ "${rc}" -le 1 ]] || alab_die "git grep for ALab's hook lines failed (exit ${rc})"
  rc=0
  lint "${repo_root}" "$(alab_check_lint_target "${own_target}")" "${lint_packages[@]}" >"${scratch}/lint.json" || rc=$?
  stopped "${rc}"
  : >"${scratch}/pristine-lint.json"
  lint_verdict "${scratch}/lint.json" "${scratch}/pristine-lint.json" false
  if [[ "$(verdict '.owned | length')" -gt 0 ]]; then
    verdict '.owned | join("")' >&2
    echo "alab/check.sh: tippy reports the warnings and errors above in ALab's code" >&2
    exit 1
  fi
  if [[ "${rc}" -ne 0 && "$(verdict '(.failing | length) + (.scripts | length)')" -eq 0 ]]; then
    echo "alab/check.sh: tippy failed (exit ${rc}) with no error in any target: cargo says why above" >&2
    exit 1
  fi
  if [[ "$(verdict '(.spanning | length) + (.failing | length) + (.scripts | length)')" -gt 0 ]]; then
    if ! pristine_upstream; then
      verdict '[.spanning[].rendered, .failing[].errors[]] | join("")' >&2
      script_lines "" true >&2
      echo "alab/check.sh: tippy reports the above in upstream's code next to ALab's (a span over a \
hook line, a target that does not build, or a build script that does not run), and cannot \
compare it with upstream alone" >&2
      exit 1
    fi
    # The packages to compare: the ones upstream has too, under the same name.
    packages=()
    while IFS= read -r manifest; do
      name="$(package_name "${repo_root}/${manifest}")"
      [[ -f "${pristine}/${manifest}" && "$(package_name "${pristine}/${manifest}")" == "${name}" ]] \
        && packages+=(-p "${name}")
    done < <(verdict '[.spanning[].manifest, .failing[].manifest, .scripts[].manifest] | unique[]')
    echo "alab/check.sh: comparing tippy's findings next to ALab's code with pristine upstream \
${upstream_base:0:9} in ${pristine}" >&2
    if [[ ${#packages[@]} -gt 0 ]]; then
      rc=0
      lint "${pristine}" "$(alab_check_lint_target "${pristine}/codex-rs/target")" ${pristine_config[@]+"${pristine_config[@]}"} "${packages[@]}" \
        >"${scratch}/pristine-lint.json" || rc=$?
      stopped "${rc}"
    fi
    lint_verdict "${scratch}/lint.json" "${scratch}/pristine-lint.json" true
    while IFS=$'\t' read -r kind name manifest; do
      echo "alab/check.sh: ${kind} ${name} (${manifest}) does not build on upstream ${upstream_base:0:9} alone either (not gating)" >&2
    done < <(verdict '.failing[] | select(.upstream_too) | [.target.kind, .target.name, .manifest] | @tsv')
    script_lines ", and does not run on upstream ${upstream_base:0:9} alone either (not gating)" .upstream_too >&2
    if [[ "$(verdict '[(.spanning[] | select(.ours)), (.failing[] | select(.upstream_too | not)),
        (.scripts[] | select(.upstream_too | not))] | length')" -gt 0 ]]; then
      verdict '[(.spanning[] | select(.ours) | .rendered), (.failing[] | select(.upstream_too | not) | .errors[])] | join("")' >&2
      while IFS=$'\t' read -r kind name manifest; do
        echo "alab/check.sh: ${kind} ${name} (${manifest}) builds on upstream ${upstream_base:0:9} alone, not with ALab's layer" >&2
      done < <(verdict '.failing[] | select(.upstream_too | not) | [.target.kind, .target.name, .manifest] | @tsv')
      script_lines ", and upstream ${upstream_base:0:9} alone shows no such failure" '.upstream_too | not' >&2
      echo "alab/check.sh: tippy reports the above with ALab's layer, and upstream ${upstream_base:0:9} alone does not" >&2
      exit 1
    fi
  fi
  echo "alab/check.sh: tippy finds nothing in ALab's code ($(verdict .set_aside) diagnostics in upstream's set aside)" >&2
fi

alab_build_command "${cargo}" --unverified run --quiet --locked -p codex-cli --bin codex -- --version || passed $? 2
if [[ ${#alab_packages[@]} -gt 0 ]]; then
  alab_build_command "${cargo}" --unverified test --locked --no-fail-fast "${alab_packages[@]}" || passed $? 1
fi
# Compile the fixed native kernel-probe driver under the same protected compiler
# authority. macOS refuses sandbox_apply inside an existing sandbox, so the
# authenticated parent runs its two real boundary controls after this child drains.
if [[ "$(uname -s)" == Darwin && -f "${repo_root}/codex-rs/alab-release/Cargo.toml" ]]; then
  alab_build_command "${cargo}" --unverified build --locked -p codex-alab-release --bin codex-release || passed $? 1
fi

[[ ${#hooked_packages[@]} -gt 0 ]] || exit 0

packages=("${hooked_packages[@]}")
log="${unit_root}/unit.log"
unit_reader="${repo_root}/alab/unit_results.py"
unit_python="${ALAB_PYTHON:-/usr/bin/python3}"
unit_state="${unit_root}/unit-main"
rc=0
unit_tests "${repo_root}" "${own_target}" --record-results "${unit_state}" 2>&1 | tee "${log}" || rc=$?
stopped "${rc}"
"${unit_python}" -I "${unit_reader}" collect --state "${unit_state}" --log "${log}" \
  --status "${rc}" --out "${unit_root}/unit-main.json"
[[ "$(jq '.failed | length' "${unit_root}/unit-main.json")" -gt 0 ]] || exit 0

# Keep Cargo's complete feature graph and runtime context, but prepare once and
# execute only the original failed package/library/test copies in fresh processes.
rc=0
unit_state="${unit_root}/unit-alone"
unit_tests "${repo_root}" "${own_target}" --compare-results "${unit_root}/unit-main.json" \
  "${unit_state}" 2>&1 | tee "${unit_root}/alone.log" || rc=$?
stopped "${rc}"
"${unit_python}" -I "${unit_reader}" collect --state "${unit_state}" --log "${unit_root}/alone.log" \
  --status "${rc}" --out "${unit_root}/unit-alone.json"
while IFS= read -r copy; do
  printf 'alab/check.sh: passes alone, as upstream runs it (not gating): %s\n' "${copy}" >&2
done < <(jq -r '.passed[] | "\(.package)/\(.target): \(.test)"' "${unit_root}/unit-alone.json")
[[ "$(jq '.failed | length' "${unit_root}/unit-alone.json")" -gt 0 ]] || exit 0

if ! pristine_upstream; then
  echo 'alab/check.sh: failed unit-test copies cannot be compared with upstream' >&2
  exit 1
fi
echo "alab/check.sh: rerunning $(jq '.failed | length' "${unit_root}/unit-alone.json") test copy(s) on pristine upstream ${upstream_base:0:9} in ${pristine}" >&2
# Only the hooked packages upstream has too: one ALab added or upstream dropped would stop
# the run before any test, and every failure would then gate.
packages=()
for i in "${!hooked_dirs[@]}"; do
  [[ -f "${pristine}/codex-rs/${hooked_dirs[$i]}/Cargo.toml" \
    && "$(package_name "${pristine}/codex-rs/${hooked_dirs[$i]}/Cargo.toml")" == "${hooked_packages[$((2 * i + 1))]}" ]] \
    && packages+=(-p "${hooked_packages[$((2 * i + 1))]}")
done
rc=0
unit_state="${unit_root}/unit-pristine"
unit_tests "${pristine}" "${pristine}/codex-rs/target" ${pristine_config[@]+"${pristine_config[@]}"} --compare-results \
  "${unit_root}/unit-alone.json" "${unit_state}" 2>&1 | tee "${unit_root}/pristine.log" || rc=$?
stopped "${rc}"
"${unit_python}" -I "${unit_reader}" collect --state "${unit_state}" --log "${unit_root}/pristine.log" \
  --status "${rc}" --out "${unit_root}/unit-pristine.json"
while IFS= read -r copy; do
  printf 'alab/check.sh: fails on upstream %s alone too (not gating): %s\n' "${upstream_base:0:9}" "${copy}" >&2
done < <(jq -r '.failed[] | "\(.package)/\(.target): \(.test)"' "${unit_root}/unit-pristine.json")
while IFS= read -r copy; do
  printf 'alab/check.sh: fails with ALab\047s layer, passes on upstream %s: %s\n' "${upstream_base:0:9}" "${copy}" >&2
done < <(jq -r '.passed[] | "\(.package)/\(.target): \(.test)"' "${unit_root}/unit-pristine.json")
[[ "$(jq '.passed | length' "${unit_root}/unit-pristine.json")" -eq 0 ]]

}

check_selected() {
  alab_project_selected || return $?
  local upstream_base pristine build_target pristine_target common feature_reads=()
  cd "${repo_root}"
  upstream_base="$(git merge-base HEAD upstream/main)" || return $?
  pristine="$(alab_pristine "${repo_root}" "${upstream_base}")" || return $?
  alab_pristine_at "${repo_root}" "${upstream_base}" || return $?
  build_target="$(alab_check_targets_ready "${repo_root}" "${pristine}" \
    "${CARGO_TARGET_DIR:-${repo_root}/codex-rs/target}")" || return $?
  pristine_target="$(alab_real "${pristine}/codex-rs/target")" || return $?
  export CARGO_TARGET_DIR="${build_target}"
  if [[ -z "${ALAB_FEATURE_BUNDLE:-}" ]]; then
    common="$(git rev-parse --path-format=absolute --git-common-dir)" || return $?
    ALAB_FEATURE_BUNDLE="${common}/alab-feature-evidence/$(git rev-parse HEAD)" || return $?
    export ALAB_FEATURE_BUNDLE
  fi
  [[ "${ALAB_FEATURE_BUNDLE}" == /* ]] || alab_die "ALAB_FEATURE_BUNDLE must be absolute"
  if [[ -d "${ALAB_FEATURE_BUNDLE}" ]]; then
    feature_reads=(--read "${ALAB_FEATURE_BUNDLE}")
  fi
  alab_contained --pristine "${pristine}" --read "${ATPKG_PROJECT_FRONT}" \
    --read "${ATPKG_PROJECT_EXECUTION}" ${feature_reads[@]+"${feature_reads[@]}"} "${build_target}" "${pristine_target}" -- \
    /bin/bash -c '. "$1"; shift; check_main "$@"' alab-check "${repo_root}/alab/check.sh" || return $?
  if [[ "$(uname -s)" == Darwin && -f "${repo_root}/codex-rs/alab-release/Cargo.toml" ]]; then
    local kernel_scratch kernel_status=0
    mkdir -p /tmp/.ubd || return $?
    kernel_scratch="$(mktemp -d /tmp/.ubd/cks.XXXXXX)" || return $?
    TMPDIR="${kernel_scratch}" "${build_target}/debug/codex-release" verify-sandbox || kernel_status=$?
    if [[ "${kernel_status}" -ne 0 ]]; then
      alab_say "native sandbox controls failed; kept ${kernel_scratch}"
      return "${kernel_status}"
    fi
    # This fixed native probe joined every env/cat/touch child; it forks no workers.
    rm -rf "${kernel_scratch}"
  fi
}

check_entry() {
  if [[ $# -ne 0 ]]; then
    check_main "$@"
    return
  fi
  ALAB_PYTHON="$(alab_python_311)" || return $?
  export ALAB_PYTHON
  alab_project_run "${repo_root}" "" /bin/bash -c \
    '. "$1"; shift; check_selected "$@"' alab-check "${repo_root}/alab/check.sh"
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  check_entry "$@"
fi
