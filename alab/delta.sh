#!/usr/bin/env bash
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
#
# alab/delta.sh — the ALab layer, measured: how <rev> (default HEAD) differs from the
# newest upstream commit it contains.
#
#   alab/delta.sh [<rev>]            summary, then the upstream files ALab modifies, riskiest first
#   alab/delta.sh --patch [<rev>]    the diff of just those modifications (ALab's hook points)
#   alab/delta.sh --topics [<rev>]   every ALAB(<topic>) hook marker, grouped by topic (Python 3.11+)
#
# risk = upstream commits touching the file in the last 90 days x lines ALab changed in it.
# The top of the list is where the next sync conflict comes from, and where the occasional
# refactor (move the change into an ALab-owned crate behind a small hook) pays most.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=alab/lib.sh
. "${repo_root}/alab/lib.sh"
cd "${repo_root}"

patch=0
topics=0
rev=HEAD
for arg in "$@"; do
  case "${arg}" in
    --patch) patch=1 ;;
    --topics) topics=1 ;;
    -h | --help)
      sed -n '5,14p' "${repo_root}/alab/delta.sh" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    -*) alab_die "unknown option '${arg}' (see --help)" ;;
    *) rev="${arg}" ;;
  esac
done
g() { git -c core.quotepath=off "$@"; }

if [[ "${topics}" -eq 1 ]]; then
  # One shared parser keeps exact topic identities separate from display text.
  python="$(alab_python_311)" || exit 1
  exec "${python}" -I "${repo_root}/alab/hook_topics.py" --repo "${repo_root}" --source "${rev}"
fi

alab_upstream_ready

# The newest upstream commit <rev> contains. Its last sync merge names it too, so the answer
# holds in a clone whose upstream/main is older than what main merged (not fetched lately).
last_sync="$(git log -1 --first-parent --merges --grep='^alab sync:' --format=%H "${rev}" --)"
base="$(git merge-base "${rev}" upstream/main ${last_sync:+"${last_sync}^2"})" \
  || alab_die "${rev} shares no history with upstream/main"

# Derived files (merge=alab-upstream in alab/merge-attributes) are re-resolved by every sync,
# so they are reported apart from the hand-maintained conflict surface.
derived_specs=()
while IFS= read -r line; do derived_specs+=("${line}"); done < <(
  awk '!/^[[:space:]]*(#|$)/ && /merge=alab-upstream/ { print $1 }' "${repo_root}/alab/merge-attributes" 2>/dev/null
)
not_derived=()
for line in ${derived_specs[@]+"${derived_specs[@]}"}; do not_derived+=(":(exclude)${line}"); done
modified="$(g diff --name-only --no-renames --diff-filter=MT "${base}" "${rev}" -- . ${not_derived[@]+"${not_derived[@]}"})"
if [[ "${patch}" -eq 1 ]]; then
  [[ -z "${modified}" ]] && exit 0
  printf '%s\n' "${modified}" | tr '\n' '\0' | xargs -0 git diff "${base}" "${rev}" --
  exit 0
fi

specs=()
while IFS= read -r line; do
  specs+=("${line}")
done < <(alab_specs "${repo_root}/alab/removed-paths")

deleted="$(g diff --name-only --no-renames --diff-filter=D "${base}" "${rev}" | sort)"
covered=""
if [[ ${#specs[@]} -gt 0 ]]; then
  covered="$(g diff --name-only --no-renames --diff-filter=D "${base}" "${rev}" -- "${specs[@]}" | sort)"
fi
uncovered="$(comm -23 <(printf '%s\n' "${deleted}" | sed '/^$/d') <(printf '%s\n' "${covered}" | sed '/^$/d'))"

count() { printf '%s\n' "$1" | sed '/^$/d' | wc -l | tr -d ' '; }
lines() { # <diff-filter> [pathspec...]: "+added/-deleted" over the files they select
  local filter="$1"
  shift
  g diff --numstat --no-renames --diff-filter="${filter}" "${base}" "${rev}" -- "$@" \
    | awk -F'\t' '{ if ($1 != "-") { a += $1; d += $2 } } END { printf "+%d/-%d", a, d }'
}

echo "ALab layer of ${rev} ($(git rev-parse --short "${rev}")) over upstream $(git log -1 --format='%h (%cs)' "${base}")"
ahead="$(git rev-list --count "${base}..upstream/main")"
if [[ "${ahead}" -gt 0 ]]; then
  echo "  upstream/main is ${ahead} commit(s) ahead: alab/sync.sh merges them"
fi
echo "  ALab-owned  $(count "$(g diff --name-only --no-renames --diff-filter=A "${base}" "${rev}")") file(s) added, $(lines A)"
echo "  removed     $(count "${deleted}") upstream file(s), $(count "${covered}") under alab/removed-paths"
if [[ -n "${uncovered}" ]]; then
  echo "              $(count "${uncovered}") deleted outside alab/removed-paths (each can conflict at a sync):"
  printf '%s\n' "${uncovered}" | sed 's/^/                /'
fi
if [[ ${#derived_specs[@]} -gt 0 ]]; then
  derived="$(g diff --name-only --no-renames --diff-filter=MT "${base}" "${rev}" -- "${derived_specs[@]}")"
  [[ -z "${derived}" ]] || echo "  derived     $(count "${derived}") file(s), $(lines MT "${derived_specs[@]}"), re-resolved by every sync: $(printf '%s' "${derived}" | tr '\n' ' ')"
fi
echo "  modified    $(count "${modified}") upstream file(s), $(lines MT . ${not_derived[@]+"${not_derived[@]}"})  <- the conflict surface"
[[ -z "${modified}" ]] && exit 0

echo
printf '  %6s %6s  %-12s %s\n' risk churn '+/-' file
{
  g log --since=90.days.ago --format= --name-only "${base}" \
    | sed '/^$/d' | sort | uniq -c | awk '{ c = $1; $1 = ""; sub(/^ /, ""); print "C\t" c "\t" $0 }'
  g diff --numstat --no-renames --diff-filter=MT "${base}" "${rev}" -- . ${not_derived[@]+"${not_derived[@]}"} \
    | awk -F'\t' '{ print "N\t" $1 "\t" $2 "\t" $3 }'
} | awk -F'\t' '
  $1 == "C" { churn[$3] = $2; next }
  $1 == "N" {
    a = ($2 == "-") ? 0 : $2; d = ($3 == "-") ? 0 : $3; c = ($4 in churn) ? churn[$4] : 0
    printf "%d\t  %6d %6d  %-12s %s\n", c * (a + d), c * (a + d), c, "+" a "/-" d, $4
  }' | sort -t$'\t' -k1,1nr | cut -f2-
