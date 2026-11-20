#!/usr/bin/env bash
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
#
# Run intentional native failures through the selected Trust unit-test runner.
# A successful probe means their bounded diagnostics survived normal protected
# scratch cleanup; each child's recorded Cargo outcome remains 101.
#
# Usage: alab/native_diagnostics_probe.sh [all|shutdown|incomplete]
# CARGO_TARGET_DIR may select an admitted target, as for alab/check.sh.
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
# shellcheck source=alab/lib.sh
. "${repo}/alab/lib.sh"

case "${1:-all}" in
  --help|-h)
    sed -n '5,10p' "$0" | sed 's/^# \{0,1\}//'
    exit 0
    ;;
  all|shutdown|incomplete)
    [[ $# -le 1 ]] || alab_die "see --help"
    [[ "${ALAB_CONTAIN:-1}" != 0 && -x /usr/bin/sandbox-exec ]] \
      || alab_die "the diagnostic probe requires the maintained macOS protected profile"
    [[ -z "${ALAB_TEST_SCRATCH:-}" ]] \
      || alab_die "run the diagnostic probe outside an existing contained command"
    ALAB_PYTHON="$(alab_python_311)"
    export ALAB_PYTHON
    alab_project_run "${repo}" "" /bin/bash "$0" --selected "${1:-all}"
    ;;
  --selected)
    [[ $# -eq 2 ]] || alab_die "invalid selected diagnostic probe"
    phase="$2"
    alab_project_selected
    target="$(alab_build_target_ready "${repo}/codex-rs" \
      "${CARGO_TARGET_DIR:-${repo}/codex-rs/target}")"
    export CARGO_TARGET_DIR="${target}" CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0
    export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}" RUST_TEST_THREADS=1
    alab_native_test_nofile
    "${ALAB_PYTHON}" -I -B "${repo}/alab/retention.py" --repo "${repo}" \
      || alab_die "the existing preservation policy prevents a scratch-cleanup probe"
    state="$(mktemp -d "${target}/alab-native-diagnostics-probe.XXXXXX")"
    printf 'ALAB_NATIVE_DIAGNOSTICS_PROBE_STATE %s\n' "${state}"
    status=0
    alab_verified_contained --read "${ATPKG_PROJECT_FRONT}" \
      --read "${ATPKG_PROJECT_EXECUTION}" "${target}" -- \
      /bin/bash "$0" --contained "${phase}" "${target}" "${state}" || status=$?
    printf '%s\n' "${status}" >"${state}/contained.status"
    [[ "${status}" -eq 101 ]] \
      || alab_die "expected intentional native failure 101; got ${status}; evidence: ${state}"
    phases=("${phase}")
    [[ "${phase}" != all ]] || phases=(shutdown incomplete)
    for phase in "${phases[@]}"; do
      actual="$(cat "${state}/${phase}.status")"
      "${ALAB_PYTHON}" -I -B "${repo}/alab/unit_results.py" collect \
        --state "${state}/${phase}" --log "${state}/${phase}.log" \
        --status "${actual}" --out "${state}/${phase}.collected.json"
      "${ALAB_PYTHON}" -I -B "${repo}/alab/tests/test_native_failure_diagnostics.py" \
        --verify "${state}" "${phase}"
    done
    printf 'ALAB_NATIVE_DIAGNOSTICS_PROBE_OK %s\n' "${state}"
    ;;
  --contained)
    [[ $# -eq 4 ]] || alab_die "invalid contained diagnostic probe"
    phase="$2" target="$3" state="$4"
    alab_project_selected
    alab_contained_scratch_path "${ALAB_TEST_SCRATCH:-}" \
      || alab_die "the diagnostic child requires an allocated protected scratch"
    printf '%s\n' "${ALAB_TEST_SCRATCH}" >"${state}/contained-scratch"
    export CODEX_HOME="${ALAB_TEST_SCRATCH}/probe-home"
    mkdir "${CODEX_HOME}"
    cargo="${ALAB_CARGO}"
    # shellcheck source=alab/unit-tests.sh
    . "${repo}/alab/unit-tests.sh"
    packages=(-p codex-alab-autopilot)
    phases=("${phase}")
    [[ "${phase}" != all ]] || phases=(shutdown incomplete)
    for phase in "${phases[@]}"; do
      case "${phase}" in
        shutdown) fixture=shutdown_returned_failure_fixture ;;
        incomplete) fixture=incomplete_failure_fixture ;;
        *) alab_die "invalid diagnostic fixture phase" ;;
      esac
      export ALAB_NATIVE_DIAGNOSTICS_PROBE="${phase}"
      status=0
      unit_tests "${repo}" "${target}" --record-results "${state}/${phase}" \
        "extension::upgrade_handoff_tests::native_failure_diagnostics_tests::${fixture}" \
        --ignored --exact --test-threads=1 >"${state}/${phase}.log" 2>&1 || status=$?
      printf '%s\n' "${status}" >"${state}/${phase}.status"
      cat "${state}/${phase}.log"
      [[ "${status}" -eq 101 ]] || exit "${status}"
      # Cargo also returns 101 for compilation failure. Do not launch another
      # intentional failure unless this phase reached a recorded Cargo round.
      [[ -f "${state}/${phase}/state.json" && -f "${state}/${phase}/round-0.status" ]] \
        || alab_die "the ${phase} diagnostic fixture never reached a recorded Cargo round"
    done
    exit 101
    ;;
  *) alab_die "see --help" ;;
esac
