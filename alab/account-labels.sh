#!/usr/bin/env bash
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
set -euo pipefail

if [[ $# -eq 1 && ( $1 == -h || $1 == --help ) ]]; then
  printf '%s\n' \
    'Usage: bash alab/account-labels.sh [FIRST_PROVIDER_EMAIL SECOND_PROVIDER_EMAIL]' \
    '' \
    'With no arguments, list all configured Codex slots in roster order as JSON.' \
    'Supply two distinct provider email addresses to look up their labels in input order.' \
    'Each slot has a slotLabel and an email (null if unconfigured). A distinct' \
    'configured sign-in address appears in loginEmail.'
  exit 0
fi

source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
python="$(alab_python_311)"
exec "$python" "$alab_library_root/alab/smoke/live_account_quota.py" --labels-only --show-logins -- "$@"
