#!/usr/bin/env bash
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
# Native explicit-target admission preserves the held selection in pristine cwd.
set -euo pipefail
[[ "${ATPKG_PROJECT_FRONT:-}" == /* && -x "${ATPKG_PROJECT_FRONT}" \
  && "${ATPKG_PROJECT_TARGO:-}" == /* && -x "${ATPKG_PROJECT_TARGO}" ]] || {
  echo "alab: no native project compiler selection" >&2
  exit 1
}
exec "${ATPKG_PROJECT_FRONT}" __build_admit "${ATPKG_PROJECT_TARGO}" "$@"
