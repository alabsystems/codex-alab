#!/bin/bash
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
# Execute one reviewed recipe using the candidate's own compiler and test helpers.
set -euo pipefail
umask 077
layer="$1" repo="$2" source_sha="$3" out="$4" stage="$5"
shift 5
script="$(cd "$(dirname "$0")" && pwd -P)/$(basename "$0")"
recipes="$(dirname "$script")"
# Outside the sandbox execute the collector's helpers. Candidate helpers run only
# after the normal protected profile has granted the exact source and output.
if [[ "$layer" == contained ]]; then
    . "$repo/alab/lib.sh"
else
    . "$recipes/../lib.sh"
fi

case "$layer" in
    outer)
        [[ "${ALAB_CONTAIN:-1}" != 0 ]] || { echo 'feature execution requires containment' >&2; exit 1; }
        [[ -x /usr/bin/sandbox-exec ]] || { echo 'no supported feature execution profile on this host' >&2; exit 1; }
        alab_project_exec "$repo" "$source_sha" /bin/bash "$script" selected "$repo" "$source_sha" "$out" "$stage" "$@"
        exit $?
        ;;
    selected)
        alab_project_selected
        ALAB_PYTHON="$(alab_python_311)" || exit $?
        export ALAB_PYTHON
        export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0
        cd "$repo/codex-rs"
        case "$stage" in
            package-inputs)
                # Run the reviewed host fixture that owns the one kernel profile.
                # The candidate dispatcher runs inside that profile.
                export ALAB_FEATURE_WORKSPACE="$repo"
                alab_build_command --result-file "$out/../guard.json" /bin/bash "$script" package-profile "$repo" "$source_sha" "$out" "$stage"
                exit $?
                ;;
            proof_build|qualification|upstream_gate|feature_gate|promote|version_lines|package_version|check_runtime|reset_notice_oracle|hook-controls)
                location="$repo/alab/tests"
                pattern="test_${stage}.py"
                if [[ "$stage" == hook-controls ]]; then
                    location="$recipes"
                    pattern=hook_controls.py
                elif [[ "$stage" == proof_build ]]; then
                    location="$recipes"
                    pattern=proof_controls.py
                fi
                export ALAB_FEATURE_WORKSPACE="$repo"
                # The outer guardian writes its completion receipt outside the test output.
                alab_build_command --result-file "$out/../guard.json" /bin/bash "$script" python-profile "$repo" "$source_sha" "$out" "$stage" \
                    "$ALAB_PYTHON" -I -B "$repo/alab/python_test_results.py" --start-directory "$location" --pattern "$pattern" --record "$out/tests.json"
                exit $?
                ;;
        esac
        # The selected controller owns this receipt outside the contained test's writable directory.
        CARGO_TARGET_DIR="$(alab_build_target_ready "$PWD" "$PWD/target" "$out/../cache-refused.status")" || exit $?
        export CARGO_TARGET_DIR
        build_profile=(/bin/bash "$script" build-profile "$repo" "$source_sha" "$out" "$stage" "$@")
        if capability="$(alab_exec_pkg claim --capabilities)"; then
            if [[ "$capability" == aterm-claim-exec-v1 ]]; then
                jobs="${CARGO_BUILD_JOBS:-4}"
                if [[ "$jobs" == default || "$jobs" =~ ^-[0-9]+$ ]]; then
                    jobs="$("$ALAB_PYTHON" -I -B -c '
import os, sys
offset = 0 if sys.argv[1] == "default" else int(sys.argv[1])
print(max(1, (os.cpu_count() or 1) + offset))
' "$jobs")"
                fi
                # The host sees the account CPU slots. Its claim lives through the
                # protected worker and bounds Cargo jobs even under an ancestor grant.
                # Match the worker's marker-free lookup of memory-backed ancestors.
                # Preserve an explicit request for independent admission.
                if [[ "${ATERM_CAPACITY_COVER:-}" != none ]]; then unset ATERM_CAPACITY_COVER; fi
                # Transfer the original /dev/fd/N from process substitution. A duplicate
                # would outlive native admission and keep the status reader in the build.
                exec_build_claim() {
                    local status_pipe="$1"
                    alab_exec_pkg claim --what "Codex feature $stage" --key "codex-alab-feature-$stage" \
                        --checkout "$repo" --rank ordinary --cores "$jobs" --wait-ms 60000 \
                        --status-fd "${status_pipe##*/}" -- "${build_profile[@]}"
                }
                exec_build_claim >(/bin/cat >/dev/null)
                exit $?
            fi
        else
            status=$?
            case "$status" in
                1|2|64|127) ;; # A host without this verb can use per-build admission.
                *) exit "$status" ;;
            esac
        fi
        # Older hosts can still admit individual builds from inside the profile.
        exec "${build_profile[@]}"
        ;;
    build-profile)
        alab_native_test_nofile
        cd "$repo/codex-rs"
        threads="${RUST_TEST_THREADS:-4}"
        jobs="${CARGO_BUILD_JOBS:-}"
        if [[ "$jobs" =~ ^[1-9][0-9]*$ && "$threads" =~ ^[+]?0*([1-9][0-9]*)$ ]]; then
            # libtest accepts unsigned widths larger than Bash's signed integers.
            threads="${BASH_REMATCH[1]}"
            if [[ ${#threads} -gt ${#jobs} || ( ${#threads} -eq ${#jobs} && "$threads" > "$jobs" ) ]]; then
                export RUST_TEST_THREADS="$jobs"
            fi
        fi
        collector_reads=(--read "$recipes")
        if [[ "$stage" == *wire && $# -gt 0 ]]; then
            collector_reads+=(--read "${recipes%/*}/python_test_results.py" --read "${recipes%/*}/cli_status.py")
        fi
        if [[ "$stage" == native-ui-wire ]]; then
            while IFS= read -r helper; do
                collector_reads+=(--read "${recipes%/*}/$helper")
            done < "$recipes/queued_refusal.inputs"
        fi
        if [[ "$stage" == code-mode-stdio ]]; then
            collector_reads+=(--read "$recipes/../native_helpers.py")
        fi
        alab_contained "${collector_reads[@]}" --read "$ATPKG_PROJECT_FRONT" --read "$ATPKG_PROJECT_EXECUTION" "$CARGO_TARGET_DIR" "$out" -- /bin/bash "$script" contained "$repo" "$source_sha" "$out" "$stage" "$@"
        exit $?
        ;;
    package-profile)
        exec "$ALAB_PYTHON" -I -B "$recipes/../python_test_results.py" --start-directory "$recipes" --pattern package_input_controls.py --record "$out/tests.json"
        ;;
    python-profile)
        alab_contained --read "$recipes" --read "$ATPKG_PROJECT_FRONT" --read "$ATPKG_PROJECT_EXECUTION" "$out" -- "$@"
        exit $?
        ;;
    contained) alab_project_selected ;;
    *) exit 64 ;;
esac
cd "$repo/codex-rs"
CODEX_HOME="$(mktemp -d "$TMPDIR/feature-home.XXXXXX")" || exit $?
export CODEX_HOME
export RUST_MIN_STACK=8388608
# Match the native gate: parallel fixtures share one libtest process's file descriptors.
export RUST_TEST_THREADS="${RUST_TEST_THREADS:-4}"
. "$repo/alab/unit-tests.sh"
. "$repo/alab/focused-tests.sh"
cargo="$ALAB_CARGO"
case "$stage" in
    connector-logging)
        [[ "$#" == 1 ]] || exit 64
        alab_run_focused 1 --require-test "$1" -- alab_build_command "$cargo" --unverified test --locked \
            -p codex-api --test responses_websocket_logging -- "$1" --exact --show-output
        exit $?
        ;;
    websocket) packages=(-p codex-websocket-client) ;;
    transport) packages=(-p codex-api -p codex-alab-api-tests -p codex-alab-state-contracts) ;;
    state|aterm|autopilot-library|recovery|goal-extension)
        [[ $# -gt 0 ]] || { echo "$stage requires exact test names" >&2; exit 64; }
        case "$stage" in
            state) packages=(-p codex-state) ;;
            aterm) packages=(-p codex-alab-aterm) ;;
            autopilot-library) packages=(-p codex-alab-autopilot) ;;
            recovery) packages=(-p codex-alab-quota-recovery) ;;
            goal-extension) packages=(-p codex-goal-extension) ;;
        esac
        requirements=()
        for test_name in "$@"; do requirements+=(--require-test "$test_name"); done
        alab_run_focused 1 "${requirements[@]}" -- unit_tests "$repo" "$CARGO_TARGET_DIR" "$@" --exact --show-output
        exit $?
        ;;
    core) packages=(-p codex-core) ;;
    tui) packages=(-p codex-tui) ;;
    boot) packages=(-p codex-alab-core) ;;
    api-fixtures) packages=(-p codex-alab-api-tests) ;;
    watch-fixtures) packages=(-p codex-exec-server) ;;
    # The TUI's test dependency selects alab-tui/test-support for the diagnostic fixtures.
    tui-fixtures) packages=(-p codex-alab-tui -p codex-tui) ;;
    unit-comparison)
        # Exercise the actual native unit-result adapter with independent candidate and pristine copies.
        "$cargo" metadata --locked --no-deps --format-version 1 > "$out/metadata.json"
        alab_build_command "$cargo" --unverified build --locked -p codex-alab-unit-runner --bin alab-unit-runner --message-format=json-render-diagnostics > "$out/runner.jsonl"
        selected="$("$ALAB_PYTHON" -I -B "$repo/alab/native_helpers.py" --repo "$repo" --target-dir "$CARGO_TARGET_DIR" --metadata "$out/metadata.json" --helper codex-alab-unit-runner/alab-unit-runner < "$out/runner.jsonl")"
        ALAB_TEST_UNIT_RUNNER="$(printf '%s\n' "$selected" | jq -er '.["alab-unit-runner"]')"
        export ALAB_TEST_UNIT_RUNNER ALAB_FEATURE_WORKSPACE="$repo"
        alab_build_command --result-file "$out/guard.json" "$ALAB_PYTHON" -I -B "$repo/alab/python_test_results.py" --start-directory "$recipes" --pattern unit_comparison_controls.py --record "$out/tests.json"
        exit $?
        ;;
    app-server) packages=(-p codex-app-server) ;;
    code-mode-stdio)
        [[ $# == 0 ]] || exit 64
        helper_root="$out/helpers"
        mkdir "$helper_root" "$out/bin"
        tree_config=() test_command=(/usr/bin/env)
        need_metadata=1 need_cli=1 need_stdio=0 need_host=1 need_v8=1
        native_test_prerequisites "$repo" "$CARGO_TARGET_DIR" "$helper_root" 1 1
        for name in codex codex-code-mode-host; do
            if [[ "$name" == codex ]]; then
                package=codex-cli
                messages="$helper_root/helper-cli.jsonl"
            else
                package=codex-code-mode-host
                messages="$helper_root/helper-native.jsonl"
            fi
            selected="$("$ALAB_PYTHON" -I -B "$repo/alab/native_helpers.py" --repo "$repo" \
                --target-dir "$CARGO_TARGET_DIR" --metadata "$helper_root/metadata.json" --helper "$package/$name" < "$messages")"
            binary="$(printf '%s\n' "$selected" | jq -er --arg name "$name" '.[$name]')"
            cp "$binary" "$out/bin/$name"
            chmod 0555 "$out/bin/$name"
            cmp -s "$binary" "$out/bin/$name"
        done
        host="$out/bin/codex-code-mode-host"
        name=remote_session_persists_values_forwards_delegates_and_controls_cells
        # Cargo sets its own package's CARGO_BIN_EXE values when it launches tests.
        # Select the integration harness, then launch it with the retained host.
        alab_build_command "${test_command[@]}" "$cargo" --unverified test --locked --no-run \
            -p codex-code-mode-host --test stdio --message-format=json-render-diagnostics > "$helper_root/stdio.jsonl"
        selected="$("$ALAB_PYTHON" -I -B "$recipes/../native_helpers.py" --repo "$repo" --target-dir "$CARGO_TARGET_DIR" \
            --metadata "$helper_root/metadata.json" --integration-test codex-code-mode-host/stdio < "$helper_root/stdio.jsonl")"
        harness="$(printf '%s\n' "$selected" | jq -er .stdio)"
        alab_run_focused 1 --require-test "$name" -- alab_build_command "${test_command[@]}" \
            "CARGO_BIN_EXE_codex-code-mode-host=$host" "CARGO_BIN_EXE_codex_code_mode_host=$host" \
            "$harness" "$name" --exact --show-output
        exit $?
        ;;
    queue-review|login-reader|cloud-reader)
        [[ $# -gt 0 ]] || { echo "$stage requires exact test names" >&2; exit 64; }
        requirements=()
        for test_name in "$@"; do requirements+=(--require-test "$test_name"); done
        case "$stage" in
            queue-review) target=(-p codex-app-server --test alab_queue_misalignment) ;;
            login-reader) target=(-p codex-login --lib) ;;
            cloud-reader) target=(-p codex-alab-cloud-contracts --test reader) ;;
        esac
        alab_run_focused 1 "${requirements[@]}" -- alab_build_command "$cargo" --unverified test --locked \
            "${target[@]}" -- "$@" --exact --show-output
        exit $?
        ;;
    exec) packages=(-p codex-exec) ;;
    alab-tui) packages=(-p codex-alab-tui) ;;
    models-manager) packages=(-p codex-models-manager) ;;
    kimi-library) packages=(-p codex-alab-kimi) ;;
    autopilot-integration|autopilot-turn|goal-admission|core-integration|tool-recovery-cli|wire|model-wire|product-wire|skill-wire|native-ui-wire|execution-wire|fleet-cli)
        tree_config=() test_command=(/usr/bin/env) requirements=()
        helper_root="$out/helpers"
        mkdir "$helper_root"
        if [[ "$stage" == *wire || "$stage" == fleet-cli ]]; then
            # The wire fixtures and codex-cli's alab_fleet_model test launch
            # only the CLI. Use the helper inputs supported by frozen candidates.
            need_metadata=1 need_cli=1 need_stdio=0 need_host=0 need_v8=0
            native_test_prerequisites "$repo" "$CARGO_TARGET_DIR" "$helper_root" 1 1
            if [[ "$stage" == fleet-cli ]]; then
                alab_run_focused 1 -- alab_build_command "${test_command[@]}" "$cargo" --unverified test --locked -p codex-cli --test alab_fleet_model -- "$@" --show-output
                exit $?
            fi
            selected="$("$ALAB_PYTHON" -I -B "$repo/alab/native_helpers.py" --repo "$repo" --target-dir "$CARGO_TARGET_DIR" --metadata "$helper_root/metadata.json" --helper codex-cli/codex < "$helper_root/helper-cli.jsonl")"
            binary="$(printf '%s\n' "$selected" | jq -er .codex)"
            export ALAB_FEATURE_WORKSPACE="$repo" ALAB_FEATURE_BINARY="$binary" ALAB_FEATURE_RESULTS="$out"
            pattern=wire_controls.py
            case "$stage" in
                model-wire) pattern=model_wire_controls.py ;;
                product-wire) pattern=product_wire_controls.py ;;
                skill-wire) pattern=skill_wire_controls.py ;;
                native-ui-wire) pattern=native_ui_controls.py ;;
                execution-wire) pattern=execution_wire_controls.py ;;
            esac
            runner="$repo/alab/python_test_results.py"
            if [[ $# -gt 0 ]]; then
                # The reviewed collector can select exact fixture names on older candidates.
                runner="${recipes%/*}/python_test_results.py"
                set -- --tests "$@"
            fi
            alab_build_command --result-file "$out/guard.json" "$ALAB_PYTHON" -I -B "$runner" --start-directory "$recipes" --pattern "$pattern" --record "$out/tests.json" "$@"
            exit $?
        fi
        case "$stage" in
            autopilot-integration)
                packages=(-p codex-alab-autopilot)
                targets=(--test capacity_retry --test compaction_recovery --test retry_deadlines --test websocket_return)
                ;;
            autopilot-turn)
                # This binary also contains unrelated end-to-end suites.
                [[ $# -gt 0 ]] || exit 64
                packages=(-p codex-alab-autopilot)
                targets=(--test autopilot)
                set -- "$@" --exact
                ;;
            goal-admission)
                [[ $# -gt 0 ]] || { echo 'goal admission requires exact test names' >&2; exit 64; }
                packages=(-p codex-alab-autopilot)
                targets=(--test goal_admission)
                for test_name in "$@"; do requirements+=(--require-test "$test_name"); done
                set -- "$@" --exact
                ;;
            core-integration)
                packages=(-p codex-core)
                targets=(--test all)
                ;;
            tool-recovery-cli)
                [[ $# -gt 0 ]] || { echo 'CLI recovery requires exact test names' >&2; exit 64; }
                packages=(-p codex-cli)
                targets=(--test alab_tool_result_recovery)
                for test_name in "$@"; do requirements+=(--require-test "$test_name"); done
                set -- "$@" --exact
                ;;
        esac
        if [[ "$stage" == tool-recovery-cli ]]; then
            # Select the candidate CLI together with the normal core integration helpers.
            need_metadata=1 need_cli=1 need_stdio=1 need_host=1 need_v8=1
        else
            native_test_requirements default
        fi
        native_test_prerequisites "$repo" "$CARGO_TARGET_DIR" "$helper_root" 0 0
        alab_run_focused "$((${#targets[@]} / 2))" ${requirements[@]+"${requirements[@]}"} -- alab_build_command "${test_command[@]}" "$cargo" --unverified test --locked --no-fail-fast "${packages[@]}" "${targets[@]}" -- "$@" --show-output
        exit $?
        ;;
    *) exit 64 ;;
esac
alab_run_focused "$((${#packages[@]} / 2))" -- unit_tests "$repo" "$CARGO_TARGET_DIR" "$@" --show-output
