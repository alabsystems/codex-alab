#!/bin/bash
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
# Produce the release worker's controller from its frozen, private source checkout.
set -euo pipefail
umask 077
script="$(cd "$(dirname "$0")" && pwd -P)/$(basename "$0")"
repo="$(cd "$(dirname "$script")/.." && pwd -P)"
. "$repo/alab/lib.sh"

[[ $# == 4 ]] || { echo "usage: $0 build SOURCE TARGET-DIR EMPTY-OUTPUT-DIR" >&2; exit 64; }
phase="$1" source_oid="$2" target="$3" out="$4"
[[ "$source_oid" =~ ^[a-f0-9]{40}$ && "$target" == /* && "$out" == /* && -d "$out" ]]
[[ "$(alab_project_source "$repo" HEAD)" == "$source_oid" ]]
cd "$repo/codex-rs"

case "$phase" in
    build)
        [[ -z "$(ls -A "$out")" && "$(uname -s)" == Darwin ]]
        mkdir -p /tmp/.ubd
        scratch="$(mktemp -d /tmp/.ubd/crc.XXXXXX)"
        trap 'rm -rf "$scratch"' EXIT
        mkdir "$scratch/compiled"
        alab_project_run "$repo" "$source_oid" /bin/bash "$script" selected "$source_oid" "$target" "$scratch/compiled"
        # Only the compiler output directory was writable to the source build.
        # Run the normal outer kernel probes on the retained bytes.
        "${ALAB_PYTHON:-/usr/bin/python3}" -IB "$repo/alab/retain_release_controller.py" "$scratch/compiled" "$out"
        TMPDIR="$scratch" "$out/codex-release" verify-sandbox
        "$out/codex-release" cycle --help >/dev/null
        ;;
    selected)
        alab_project_selected
        CARGO_TARGET_DIR="$(alab_build_target_ready "$PWD" "$target")"
        export CARGO_TARGET_DIR
        alab_contained --read "$ATPKG_PROJECT_FRONT" --read "$ATPKG_PROJECT_EXECUTION" \
            "$CARGO_TARGET_DIR" "$out" -- /bin/bash "$script" contained "$source_oid" "$CARGO_TARGET_DIR" "$out"
        ;;
    contained)
        alab_project_selected
        alab_build_command "$ALAB_CARGO" metadata --locked --no-deps --format-version=1 > "$out/metadata.json"
        alab_build_command "$ALAB_CARGO" --unverified test --locked -p codex-alab-release --lib
        alab_build_command "$ALAB_CARGO" --unverified build --locked -p codex-alab-release \
            --bin codex-release --message-format=json-render-diagnostics > "$out/artifacts.jsonl"
        selected="$("${ALAB_PYTHON:-/usr/bin/python3}" -IB "$repo/alab/native_helpers.py" \
            --repo "$repo" --target-dir "$target" --metadata "$out/metadata.json" \
            --helper codex-alab-release/codex-release < "$out/artifacts.jsonl")"
        image="$(printf '%s\n' "$selected" | "${ALAB_PYTHON:-/usr/bin/python3}" -IB -c 'import json,sys; print(json.load(sys.stdin)["codex-release"])')"
        /usr/bin/install -m 0500 "$image" "$out/codex-release"
        ;;
    *) exit 64 ;;
esac
