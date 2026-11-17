#!/usr/bin/env bash
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
# Compile shipping application targets through the native, fail-closed proof lane.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
. "${repo_root}/alab/lib.sh"
# Compiler-command statuses pass through unchanged but are not source-attributable.
# Keep every helper/precompiler refusal on setup/internal status 2 without hiding
# a resource or signal stop from the caller.
proof_die() {
  local proof_die_status=$?
  if [[ "${proof_die_status}" -eq 75 || "${proof_die_status}" -gt 128 ]]; then
    exit "${proof_die_status}"
  fi
  alab_say "$*" || :
  exit 2
}
[[ $# -ge 2 ]] || proof_die "certified-build.sh REPORT-DIRECTORY SOURCE [--pristine-upstream] [build arguments...]"
report="$1" source="$2"
shift 2
# This is the application-build boundary, not a general Targo front door.
# A caller must not replace the policy file, select another manifest/target,
# hide options behind `--`, or redirect the report with passthrough arguments.
proof_build_args=()
proof_config_args=()
proof_locked=0 proof_packages=0 proof_binaries=0 proof_pristine=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --pristine-upstream)
      [[ "${proof_pristine}" -eq 0 ]] || proof_die "--pristine-upstream may be supplied only once"
      proof_pristine=1
      shift
      ;;
    --locked) proof_locked=1; proof_build_args+=("$1"); shift ;;
    --release) proof_build_args+=("$1"); shift ;;
    -p | --package | --bin)
      [[ $# -ge 2 && -n "$2" && "$2" != -* ]] || proof_die "$1 requires a literal build input"
      [[ "$1" != --bin ]] || proof_binaries=$((proof_binaries + 1))
      [[ "$1" == --bin ]] || proof_packages=$((proof_packages + 1))
      proof_build_args+=("$1" "$2")
      shift 2
      ;;
    *) proof_die "unsupported certified application build argument: $1" ;;
  esac
done
[[ "${proof_locked}" -eq 1 && "${proof_packages}" -gt 0 && "${proof_binaries}" -gt 0 ]] \
  || proof_die "certified application builds require --locked and explicit packages/binaries"
# Even an empty flag variable can shadow Cargo's configured requirements.
# The protected gate/package controllers already scrub these; refuse them at
# this shared boundary too instead of silently changing a caller's policy.
while IFS= read -r proof_env_key; do
  case "${proof_env_key}" in
    RUSTFLAGS | RUSTDOCFLAGS | CARGO_ENCODED_RUSTFLAGS | CARGO_ENCODED_RUSTDOCFLAGS \
      | CARGO_*_RUSTFLAGS | CARGO_*_RUSTDOCFLAGS | TARGO_*_RUSTFLAGS | TARGO_*_RUSTDOCFLAGS \
      | CARGO_BUILD_TARGET | TARGO_BUILD_TARGET | TRUSTFLAGS | CARGO_ENCODED_TRUSTFLAGS)
      proof_die "${proof_env_key} may replace certified build requirements; remove it"
      ;;
  esac
done < <(compgen -e)
[[ "${source}" =~ ^[0-9a-f]{40}$ && "$(git rev-parse HEAD)" == "${source}" ]] \
  || proof_die "certified build requires its exact checked-out source commit"
proof_source_root="$(git rev-parse --show-toplevel)" \
  || proof_die "certified build cannot resolve its source worktree"
proof_source_root="$(cd "${proof_source_root}" && pwd -P)" \
  || proof_die "certified build cannot resolve its source worktree"
[[ "$(pwd -P)" == "${proof_source_root}/codex-rs" ]] \
  || proof_die "certified application build must run in the source's codex-rs workspace"
if [[ "${proof_pristine}" -eq 1 ]]; then
  proof_pristine_root="$(alab_pristine "${repo_root}" "${source}")" \
    || proof_die "certified pristine build cannot resolve its exact upstream worktree"
  [[ -d "${proof_pristine_root}" && ! -L "${proof_pristine_root}" ]] \
    || proof_die "certified pristine build needs its prepared exact upstream worktree"
  proof_pristine_root="$(cd "${proof_pristine_root}" && pwd -P)" \
    || proof_die "certified pristine build cannot resolve its exact upstream worktree"
  [[ "${proof_source_root}" == "${proof_pristine_root}" ]] \
    || proof_die "certified pristine build must run in the prepared exact upstream worktree"
  alab_pristine_clean "${proof_pristine_root}" "${source}" "${repo_root}" \
    || proof_die "certified pristine build requires a clean exact upstream worktree"
  proof_vendor_config="$(alab_pristine_config "${repo_root}")" \
    || proof_die "certified pristine build cannot derive its vendor configuration"
  [[ -z "${proof_vendor_config}" ]] || proof_config_args=(--config "${proof_vendor_config}")
fi
[[ "${report}" == /* && ! -e "${report}" && ! -L "${report}" ]] \
  || proof_die "certified build needs a fresh absolute report directory"
alab_project_selected || proof_die "certified build requires native compiler selection"
# Certify rejects unknown, unsupported, assumed and runtime-checked obligations.
# Dependency inclusion puts codex-tui inside the proof frontier even when the
# selected primary target is codex-cli. A new live compiler session owns the
# verdict; saved report bytes never grant verification authority.
"${ALAB_CARGO}" trust build --certify --report-dir "${report}" \
  --config "${repo_root}/alab/proof-policy.toml" \
  ${proof_config_args[@]+"${proof_config_args[@]}"} "${proof_build_args[@]}"
if [[ ! -f "${report}/report.json" || -L "${report}/report.json" ]]; then
  proof_die "certified build produced no retained canonical report"
fi
alab_say "live certified build passed for ${source}; observational report retained at ${report}"
