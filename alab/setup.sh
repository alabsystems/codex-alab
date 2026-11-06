#!/usr/bin/env bash
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
#
# alab/setup.sh — make a clone of codex-alab ready to sync with openai/codex.
# Idempotent: run it in every new clone, and again whenever in doubt.
#
#   upstream remote   openai/codex, fetch-only (its push URL is a refusal); main is fetched
#                     fast-forward only, so a rewritten upstream history stops the sync
#   upstream refs     upstream/main, plus release tags as upstream-tags/rust-v*
#                     (kept out of refs/tags so they never mix with ours or get pushed)
#   rerere            on, auto-staging: a resolution is replayed when a sync is redone
#   info/exclude      insta's leftovers (*.snap.new, .*.pending-snap) are never committed: a
#                     stray pending file makes insta tidy up on a pass, which the gate's
#                     sandbox refuses, so a test fails there that upstream passes
#
# --train-rerere also teaches this clone's rerere the resolutions recorded in origin/main's
# 30 newest `alab sync:` merges, so a sync redone in a fresh clone replays them too.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=alab/lib.sh
. "${repo_root}/alab/lib.sh"
cd "${repo_root}"

train=0
for arg in "$@"; do
  case "${arg}" in
    --train-rerere) train=1 ;;
    -h | --help)
      sed -n '5,15p' "${repo_root}/alab/setup.sh" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *) alab_die "unknown argument '${arg}' (see --help)" ;;
  esac
done

if git remote get-url upstream >/dev/null 2>&1; then
  git remote set-url upstream "${ALAB_UPSTREAM_URL}"
else
  git remote add upstream "${ALAB_UPSTREAM_URL}"
fi
git remote set-url --push upstream "NEVER-PUSH-TO-UPSTREAM"
git config remote.upstream.tagOpt --no-tags
git config --unset-all remote.upstream.fetch || true
# No '+': should upstream ever rewrite main, the fetch refuses to follow it and alab/sync.sh
# stops for a person instead of merging a rewritten history.
git config --add remote.upstream.fetch 'refs/heads/main:refs/remotes/upstream/main'
git config --add remote.upstream.fetch '+refs/tags/rust-v*:refs/upstream-tags/rust-v*'

git config rerere.enabled true
git config rerere.autoUpdate true
git config merge.conflictStyle zdiff3

exclude="$(git rev-parse --path-format=absolute --git-common-dir)/info/exclude"
mkdir -p "$(dirname "${exclude}")"
for pattern in '*.snap.new' '.*.pending-snap'; do
  grep -qxF "${pattern}" "${exclude}" 2>/dev/null || printf '%s\n' "${pattern}" >>"${exclude}"
done

alab_say "fetching upstream (openai/codex main + release tags)"
"${ALAB_PYTHON:-/usr/bin/python3}" -I -B "${repo_root}/alab/command_guard.py" \
  --inherited-group-timeout 120 -- "${ALAB_PYTHON:-/usr/bin/python3}" -I -B \
  "${repo_root}/alab/github.py" --repo "${repo_root}" --anonymous fetch upstream \
  '+refs/tags/rust-v*:refs/upstream-tags/rust-v*' \
  'refs/heads/main:refs/remotes/upstream/main'
alab_say "upstream/main is $(git log -1 --format='%h %cs %s' upstream/main)"

if [[ "${train}" -eq 1 ]]; then
  # The same replay git's contrib/rerere-train.sh does, limited to sync merges: redo each
  # merge in a scratch worktree, let rerere record the conflict, then show it the
  # committed resolution so it records that too.
  scratch="$(mktemp -d "${TMPDIR:-/tmp}/alab-rerere.XXXXXX")"
  trap 'git worktree remove --force "${scratch}" >/dev/null 2>&1 || true; rm -rf "${scratch}"' EXIT
  # main's sync merges as origin has them, whatever this checkout has checked out.
  from=origin/main
  git rev-parse -q --verify "${from}" >/dev/null || from=HEAD
  git worktree add --quiet --detach "${scratch}" "${from}"
  learned=0
  # The newest 30 are plenty: a merge-forward sync rarely meets an old conflict again.
  for merge in $(git rev-list -n 30 --first-parent --merges --grep='^alab sync:' "${from}"); do
    git -C "${scratch}" checkout --quiet --detach "${merge}^1"
    if ! git -C "${scratch}" merge --no-commit --no-ff --quiet "${merge}^2" >/dev/null 2>&1; then
      if [[ -s "$(git -C "${scratch}" rev-parse --git-path MERGE_RR)" ]]; then
        git -C "${scratch}" rerere
        git -C "${scratch}" checkout --quiet "${merge}" -- .
        git -C "${scratch}" rerere
        learned=$((learned + 1))
      fi
    fi
    git -C "${scratch}" merge --abort >/dev/null 2>&1 || git -C "${scratch}" reset --quiet --hard
  done
  alab_say "rerere learned from ${learned} sync merge(s)"
fi

alab_say "ready: alab/sync.sh merges upstream; alab/delta.sh shows the ALab layer"
