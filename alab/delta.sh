#!/usr/bin/env bash
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
#
# alab/delta.sh — the ALab layer, measured: how <rev> (default HEAD) differs from the
# newest upstream commit it contains, or an explicitly selected ancestor.
#
#   alab/delta.sh [<rev>]            summary, then the upstream files ALab modifies, riskiest first
#   alab/delta.sh --patch [<rev>]    the diff of just those modifications (ALab's hook points)
#   alab/delta.sh --topics [<rev>]   every ALAB(<topic>) hook marker, grouped by topic (Python 3.11+)
#   alab/delta.sh --base=<rev> [<rev>]  measure an explicit ancestor without upstream refs
#
# risk = base-history commits touching the file in the last 90 days x lines ALab changed in it.
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
base_revision=
for arg in "$@"; do
  case "${arg}" in
    --patch) patch=1 ;;
    --topics) topics=1 ;;
    --base=*)
      [[ -z "${base_revision}" ]] || alab_die "--base may be specified only once"
      base_revision="${arg#--base=}"
      [[ -n "${base_revision}" ]] || alab_die "--base requires a commit revision"
      ;;
    -h | --help)
      sed -n '5,15p' "${repo_root}/alab/delta.sh" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    -*) alab_die "unknown option '${arg}' (see --help)" ;;
    *) rev="${arg}" ;;
  esac
done
g() { git -c core.quotepath=off "$@"; }

if [[ "${topics}" -eq 1 ]]; then
  [[ -z "${base_revision}" ]] || alab_die "--base cannot be combined with --topics"
  # One shared parser keeps exact topic identities separate from display text.
  python="$(alab_python_311)" || exit 1
  exec "${python}" -I "${repo_root}/alab/hook_topics.py" --repo "${repo_root}" --source "${rev}"
fi

source="$(git rev-parse --verify --end-of-options "${rev}^{commit}")" \
  || alab_die "cannot resolve source ${rev}"

# Optional manifests belong to the measured tree, never the current worktree.
source_manifest() {
  local path="$1" entry
  entry="$(git ls-tree "${source}" -- "${path}")" || return 1
  [[ -n "${entry}" ]] || return 0
  case "${entry}" in
    "100644 blob "* | "100755 blob "*) git show "${source}:${path}" ;;
    *) alab_die "source manifest ${path} must be a regular file" ;;
  esac
}

base_label=upstream
base_files=upstream
added_label=ALab-owned
surface_label="the conflict surface"
uncovered_label="deleted outside alab/removed-paths (each can conflict at a sync)"
if [[ -n "${base_revision}" ]]; then
  base="$(git rev-parse --verify --end-of-options "${base_revision}^{commit}")" \
    || alab_die "cannot resolve explicit base ${base_revision}"
  git merge-base --is-ancestor "${base}" "${source}" \
    || alab_die "explicit base ${base_revision} is not an ancestor of ${rev}"
  base_label="explicit base"
  base_files=baseline
  added_label=source
  surface_label="the measured change surface"
  uncovered_label="deleted outside alab/removed-paths"
else
  alab_upstream_ready
  # The newest upstream commit <rev> contains. Its last sync merge names it too, so the answer
  # holds in a clone whose upstream/main is older than what main merged (not fetched lately).
  last_upstream="$(alab_last_sync_upstream "${source}")" || exit $?
  base="$(git merge-base "${source}" upstream/main ${last_upstream:+"${last_upstream}"})" \
    || alab_die "${rev} shares no history with upstream/main"
fi

# Derived files (merge=alab-upstream in alab/merge-attributes) are re-resolved by every sync,
# so they are reported apart from the hand-maintained conflict surface.
attributes="$(source_manifest alab/merge-attributes)" || exit 1
derived_specs=()
while IFS= read -r line; do derived_specs+=("${line}"); done < <(
  printf '%s\n' "${attributes}" | awk '!/^[[:space:]]*(#|$)/ && /merge=alab-upstream/ { print $1 }'
)
not_derived=()
for line in ${derived_specs[@]+"${derived_specs[@]}"}; do not_derived+=(":(exclude)${line}"); done
if [[ "${patch}" -eq 1 ]]; then
  # Let Git retain literal paths; its display quoting is not a pathspec format.
  g diff --no-renames --diff-filter=MT "${base}" "${source}" -- . ${not_derived[@]+"${not_derived[@]}"}
  exit 0
fi
modified="$(g diff --name-only --no-renames --diff-filter=MT "${base}" "${source}" -- . ${not_derived[@]+"${not_derived[@]}"})"

removals="$(source_manifest alab/removed-paths)" || exit 1
specs=()
while IFS= read -r line; do
  specs+=("${line}")
done < <(printf '%s\n' "${removals}" | alab_spec_lines)

deleted="$(g diff --name-only --no-renames --diff-filter=D "${base}" "${source}" | sort)"
covered=""
if [[ ${#specs[@]} -gt 0 ]]; then
  covered="$(g diff --name-only --no-renames --diff-filter=D "${base}" "${source}" -- "${specs[@]}" | sort)"
fi
uncovered="$(comm -23 <(printf '%s\n' "${deleted}" | sed '/^$/d') <(printf '%s\n' "${covered}" | sed '/^$/d'))"

count() { printf '%s\n' "$1" | sed '/^$/d' | wc -l | tr -d ' '; }
lines() { # <diff-filter> [pathspec...]: "+added/-deleted" over the files they select
  local filter="$1"
  shift
  g diff --numstat --no-renames --diff-filter="${filter}" "${base}" "${source}" -- "$@" \
    | awk -F'\t' '{ if ($1 != "-") { a += $1; d += $2 } } END { printf "+%d/-%d", a, d }'
}

echo "ALab layer of ${rev} ($(git rev-parse --short "${source}")) over ${base_label} $(git log -1 --format='%h (%cs)' "${base}")"
if [[ -n "${base_revision}" ]]; then
  echo "  upstream/main lag is unknown (explicit baseline measurement)"
else
  ahead="$(git rev-list --count "${base}..upstream/main")"
  if [[ "${ahead}" -gt 0 ]]; then
    echo "  upstream/main is ${ahead} commit(s) ahead: alab/sync.sh merges them"
  fi
fi
echo "  ${added_label}  $(count "$(g diff --name-only --no-renames --diff-filter=A "${base}" "${source}")") file(s) added, $(lines A)"
echo "  removed     $(count "${deleted}") ${base_files} file(s), $(count "${covered}") under alab/removed-paths"
if [[ -n "${uncovered}" ]]; then
  echo "              $(count "${uncovered}") ${uncovered_label}:"
  printf '%s\n' "${uncovered}" | sed 's/^/                /'
fi
if [[ ${#derived_specs[@]} -gt 0 ]]; then
  derived="$(g diff --name-only --no-renames --diff-filter=MT "${base}" "${source}" -- "${derived_specs[@]}")"
  [[ -z "${derived}" ]] || echo "  derived     $(count "${derived}") file(s), $(lines MT "${derived_specs[@]}"), re-resolved by every sync: $(printf '%s' "${derived}" | tr '\n' ' ')"
fi
echo "  modified    $(count "${modified}") ${base_files} file(s), $(lines MT . ${not_derived[@]+"${not_derived[@]}"})  <- ${surface_label}"
[[ -z "${modified}" ]] && exit 0

echo
printf '  %6s %6s  %-12s %s\n' risk churn '+/-' file
{
  g log --since=90.days.ago --format= --name-only "${base}" \
    | sed '/^$/d' | sort | uniq -c | awk '{ c = $1; sub(/^[[:space:]]*[0-9]+ /, ""); print "C\t" c "\t" $0 }'
  g diff --numstat --no-renames --diff-filter=MT "${base}" "${source}" -- . ${not_derived[@]+"${not_derived[@]}"} \
    | awk -F'\t' '{ print "N\t" $1 "\t" $2 "\t" $3 }'
} | awk -F'\t' '
  $1 == "C" { churn[$3] = $2; next }
  $1 == "N" {
    a = ($2 == "-") ? 0 : $2; d = ($3 == "-") ? 0 : $3; c = ($4 in churn) ? churn[$4] : 0
    printf "%d\t  %6d %6d  %-12s %s\n", c * (a + d), c * (a + d), c, "+" a "/-" d, $4
  }' | sort -t$'\t' -k1,1nr | cut -f2-
