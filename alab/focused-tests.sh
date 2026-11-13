#!/usr/bin/env bash
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
# Capture both streams and the real pipeline status in the caller's admitted target.
# The caller supplies its already-authorized command; this helper grants no lane.

alab_run_focused() (
  set -o pipefail
  local minimum="${1:?minimum suite count required}"
  shift
  local -a requirements=(--minimum-suites "$minimum")
  if [[ "${1:-}" == --* ]]; then
    while [[ "$#" -gt 0 && "$1" != -- ]]; do
      case "$1" in
        --require-filter|--require-test)
          [[ "$#" -ge 2 && -n "$2" && "$2" != -* ]] || {
            echo "focused tests: $1 requires a nonempty value that is not an option" >&2
            return 1
          }
          requirements+=("$1" "$2")
          shift 2
          ;;
        *)
          echo "focused tests: expected -- before command" >&2
          return 1
          ;;
      esac
    done
    [[ "${1:-}" == -- ]] || {
      echo "focused tests: expected -- before command" >&2
      return 1
    }
    shift
  fi
  [[ "$#" -gt 0 && "${CARGO_TARGET_DIR:-}" == /* && -d "${CARGO_TARGET_DIR}" ]] || {
    echo "focused tests: command and existing absolute target directory required" >&2
    return 1
  }
  local result status=0
  result="$(mktemp "${CARGO_TARGET_DIR}/alab-focused-results.XXXXXX")" || return "$?"
  "$@" </dev/null 2>&1 | tee "$result" || status=$?
  [[ "$status" == 0 ]] || return "$status"
  "${ALAB_PYTHON:?managed Python required}" -I \
    "$(dirname "${BASH_SOURCE[0]}")/focused_results.py" \
    --log "$result" --status "$status" "${requirements[@]}"
)
