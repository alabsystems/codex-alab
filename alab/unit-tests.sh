# shellcheck shell=bash
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0

# native_v8_env <tree>: the trusted pair for this actual tree and Cargo host.
# The gate already runs contained. Use its private writable Cargo home, never
# the package producer's cache; no nested sandbox or ambient V8 override is used.
native_v8_env() (
  local tree="$1" resolver native_target python pkg_prefix p cache
  resolver="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/v8.py" || return $?
  cd "${tree}/codex-rs" || return $?
  native_target="$("${cargo}" --version --verbose)" || return $?
  native_target="$(printf '%s\n' "${native_target}" | awk '$1 == "host:" { print $2 }')"
  python="${ALAB_PYTHON:-}"
  if [[ -z "${python}" ]]; then
    pkg_prefix="$(dirname "${ATPKG_BIN:-${HOME}/Library/Application Support/aterm/pkg/bin}")"
    for p in /opt/homebrew/bin/python3.14 /opt/homebrew/bin/python3.13 /opt/homebrew/bin/python3.12 \
      /opt/homebrew/bin/python3.11 \
      "${pkg_prefix}/abrew/bin/python3.14" "${pkg_prefix}/abrew/bin/python3.13" \
      "${pkg_prefix}/abrew/bin/python3.12" "${pkg_prefix}/abrew/bin/python3.11" \
      "${HOME}/.local/share/uv/python"/cpython-3.*/bin/python3.[0-9]* \
      python3; do
      [[ "${p}" != *-config ]] || continue
      if command -v "${p}" >/dev/null && "${p}" -I -c 'import sys; sys.exit(sys.version_info < (3, 11))' 2>/dev/null; then
        python="${p}"
        break
      fi
    done
  fi
  cache="$(alab_cargo_home)" || return $?
  "${python:-python3}" -I "${resolver}" --repo "${tree}" \
    --target "${native_target}" --cache "${cache}/alab-v8-inputs"
)

# unit_tests <tree> <target-dir> [--config <value>]... [libtest args...]: the packages in
# `packages`, with the upstream native runtime prerequisites in that same tree and target.
# cargo and packages are supplied by check.sh, including its pristine comparisons, whose
# --config every cargo call in the tree takes (lib.sh, alab_pristine_config).
unit_tests() (
  local tree="$1" target_dir="$2" package artifacts stdio host cli v8 archive binding
  local result_mode="" result_state="" result_plan="" result_parser result_runner result_python runner_config
  local runner_key result_round result_rounds result_status result_overall=0
  local proxy_home
  local test_command=(/usr/bin/env) tree_config=()
  shift 2
  while [[ "${1:-}" == --config ]]; do
    [[ $# -ge 2 ]] || return 1
    tree_config+=(--config "$2")
    shift 2
  done
  local cargo_options=(--unverified test --locked ${tree_config[@]+"${tree_config[@]}"} --no-fail-fast --lib)
  case "${1:-}" in
    --record-results)
      [[ $# -ge 2 ]] || return 1
      result_mode=record result_state="$2"
      shift 2
      ;;
    --compare-results)
      [[ $# -ge 3 ]] || return 1
      result_mode=compare result_plan="$2" result_state="$3"
      shift 3
      ;;
  esac
  if [[ -n "${result_mode}" ]]; then
    [[ ${#packages[@]} -gt 0 ]] || return 1
    mkdir -p "${result_state}" || return $?
    result_state="$(cd "${result_state}" && pwd -P)" || return $?
  fi
  cd "${tree}/codex-rs" || return $?
  export RUST_MIN_STACK=8388608 CARGO_TARGET_DIR="${target_dir}"
  for package in "${packages[@]}"; do
    if [[ "${package}" == codex-network-proxy ]]; then
      # Managed MITM tests create CA files through find_codex_home. Keep both
      # candidate and pristine runs inside their admitted target, never the owner home.
      proxy_home="$(mktemp -d "${target_dir}/alab-network-proxy-home.XXXXXX")" || return $?
      trap 'rm -rf -- "${proxy_home}"' EXIT
      test_command+=("CODEX_HOME=${proxy_home}")
      break
    fi
  done
  # Pin the native shim's layout across --no-run and the later --config runner.
  # Targets already inside .noindex need no placement; preserve caller overrides.
  if [[ -z "${CARGO_BUILD_BUILD_DIR:-}" && "${target_dir}/" != *.noindex/* ]]; then
    export CARGO_BUILD_BUILD_DIR="${target_dir}/build.noindex"
  fi
  for package in "${packages[@]}"; do
    case "${package}" in
      codex-core | codex-app-server | codex-code-mode-host | codex-code-mode-runtime)
        v8="$(native_v8_env "${tree}")" || return $?
        archive="$(printf '%s\n' "${v8}" | jq -e -r '.RUSTY_V8_ARCHIVE | strings | select(length > 0)')" || return 1
        binding="$(printf '%s\n' "${v8}" | jq -e -r '.RUSTY_V8_SRC_BINDING_PATH | strings | select(length > 0)')" || return 1
        export RUSTY_V8_ARCHIVE="${archive}" RUSTY_V8_SRC_BINDING_PATH="${binding}"
        test_command+=("RUSTY_V8_ARCHIVE=${archive}" "RUSTY_V8_SRC_BINDING_PATH=${binding}")
        break
        ;;
    esac
  done
  for package in "${packages[@]}"; do
    case "${package}" in
      codex-core | codex-app-server)
        # These suites declare both helpers in upstream's BUILD.bazel and use
        # them through cargo_bin. A --lib build does not build runtime binaries.
        artifacts="$(RUSTY_V8_ARCHIVE="${archive}" RUSTY_V8_SRC_BINDING_PATH="${binding}" \
          alab_build_command "${cargo}" --unverified build --locked ${tree_config[@]+"${tree_config[@]}"} \
          -p codex-rmcp-client --bin test_stdio_server \
          -p codex-code-mode-host --bin codex-code-mode-host \
          --message-format=json-render-diagnostics)" || return $?
        if [[ -n "${result_mode}" ]]; then
          printf '%s\n' "${artifacts}" >"${result_state}/helper-native.jsonl" || return $?
        fi
        stdio="$(printf '%s\n' "${artifacts}" | jq -e -r -s '
          [.[] | select(.reason == "compiler-artifact" and .target.name == "test_stdio_server"
            and (.target.kind | index("bin"))) | .executable | select(type == "string")]
          | unique | select(length == 1) | .[0]')" || {
          echo "alab/check.sh: no unique test_stdio_server executable in helper build artifacts" >&2
          return 1
        }
        host="$(printf '%s\n' "${artifacts}" | jq -e -r -s '
          [.[] | select(.reason == "compiler-artifact" and .target.name == "codex-code-mode-host"
            and (.target.kind | index("bin"))) | .executable | select(type == "string")]
          | unique | select(length == 1) | .[0]')" || {
          echo "alab/check.sh: no unique codex-code-mode-host executable in helper build artifacts" >&2
          return 1
        }
        [[ -f "${stdio}" && -x "${stdio}" && -f "${host}" && -x "${host}" ]] || {
          echo "alab/check.sh: a prepared native test helper is not executable" >&2
          return 1
        }
        # cargo_bin checks the original name before its underscore alias. Bind
        # both to the actual artifact so inherited overrides cannot cross trees.
        test_command+=("CARGO_BIN_EXE_test_stdio_server=${stdio}"
          "CARGO_BIN_EXE_codex-code-mode-host=${host}"
          "CARGO_BIN_EXE_codex_code_mode_host=${host}")
        break
        ;;
    esac
  done
  for package in "${packages[@]}"; do
    case "${package}" in
      codex-mcp)
        # Its sandbox-capability tests need the full CLI, not the libtest binary.
        artifacts="$(alab_build_command "${cargo}" --unverified build --locked ${tree_config[@]+"${tree_config[@]}"} \
          -p codex-cli --bin codex --message-format=json-render-diagnostics)" || return $?
        if [[ -n "${result_mode}" ]]; then
          printf '%s\n' "${artifacts}" >"${result_state}/helper-cli.jsonl" || return $?
        fi
        cli="$(printf '%s\n' "${artifacts}" | jq -e -r -s '
          [.[] | select(.reason == "compiler-artifact" and .target.name == "codex"
            and (.target.kind | index("bin"))) | .executable | select(type == "string")]
          | unique | select(length == 1) | .[0]')" || {
          echo "alab/check.sh: no unique codex executable in helper build artifacts" >&2
          return 1
        }
        [[ -f "${cli}" && -x "${cli}" ]] || {
          echo "alab/check.sh: the prepared codex test helper is not executable" >&2
          return 1
        }
        # A fresh macOS executable can spend longer in first-launch loading than
        # the capability probe's timeout. Verify this exact artifact is ready.
        "${cli}" --version >/dev/null || {
          echo "alab/check.sh: the prepared codex test helper failed to start" >&2
          return 1
        }
        test_command+=("CARGO_BIN_EXE_codex=${cli}")
        break
        ;;
    esac
  done
  if [[ -n "${result_mode}" ]]; then
    result_parser="${alab_library_root}/alab/unit_results.py"
    result_python="${ALAB_PYTHON:-/usr/bin/python3}"
    # The runtime adapter belongs to ALab even when testing pristine upstream.
    # Its native C entrypoint avoids interpreter/Rust startup signal changes and
    # execs inside Cargo's own child; prepare/collect Python never enters it.
    artifacts="$(alab_build_command "${cargo}" --unverified build --locked \
      --manifest-path "${alab_library_root}/codex-rs/Cargo.toml" \
      -p codex-alab-unit-runner --bin alab-unit-runner \
      --message-format=json-render-diagnostics)" || return $?
    printf '%s\n' "${artifacts}" >"${result_state}/helper-runner.jsonl" || return $?
    result_runner="$(printf '%s\n' "${artifacts}" | jq -e -r -s '
      [.[] | select(.reason == "compiler-artifact" and .target.name == "alab-unit-runner"
        and (.target.kind | index("bin"))) | .executable | select(type == "string")]
      | unique | select(length == 1) | .[0]')" || return $?
    [[ -f "${result_runner}" && -x "${result_runner}" ]] || return 1
    "${cargo}" metadata --locked ${tree_config[@]+"${tree_config[@]}"} --no-deps --format-version 1 \
      >"${result_state}/metadata.json" || return $?
    # Keep the whole selected feature graph. Artifact provenance is captured before
    # Cargo invokes the runner; shared/custom library names never need guessing.
    alab_build_command "${test_command[@]}" "${cargo}" --unverified test --locked ${tree_config[@]+"${tree_config[@]}"} \
      --no-fail-fast --no-run --lib "${packages[@]}" --message-format=json-render-diagnostics \
      >"${result_state}/artifacts.jsonl" \
      || return $?
    if [[ "${result_mode}" == compare ]]; then
      "${result_python}" -I "${result_parser}" prepare --repo "${tree}" --target-dir "${target_dir}" \
        --metadata "${result_state}/metadata.json" --artifacts "${result_state}/artifacts.jsonl" \
        --state "${result_state}" --plan "${result_plan}" "${packages[@]}" || return $?
    else
      "${result_python}" -I "${result_parser}" prepare --repo "${tree}" --target-dir "${target_dir}" \
        --metadata "${result_state}/metadata.json" --artifacts "${result_state}/artifacts.jsonl" \
        --state "${result_state}" "${packages[@]}" || return $?
    fi
    runner_key="$(jq -e -r '.runner_cfg' "${result_state}/state.json")" || return $?
  fi
  # Match upstream's 8 MiB stack, detach from any controlling terminal (/dev/tty)
  # and stdin so TUI probes and crossterm tests never block on terminal queries
  # or input, and keep testing packages after one fails.
  invoke_unit_cargo() {
    # The guardian starts Cargo as its own session leader. No detached Cargo
    # escapes the disk observer, and runtime stdin stays disconnected.
    alab_build_command "${test_command[@]}" "${cargo}" "${cargo_options[@]}" "${packages[@]}" -- "$@" </dev/null
  }
  if [[ -z "${result_mode}" ]]; then
    invoke_unit_cargo "$@"
    return $?
  fi
  result_rounds="$(jq -e -r '.rounds' "${result_state}/state.json")" || return $?
  for ((result_round = 0; result_round < result_rounds; result_round++)); do
    runner_config="$(jq -n -c '$ARGS.positional' --args -- \
      "${result_runner}" --state "${result_state}" --round "${result_round}")" || return $?
    # A unique always-true cfg cannot replace an existing cfg runner. Existing
    # matching cfg runners conflict; a triple runner wins and missing receipts refuse.
    cargo_options=(--unverified test --locked ${tree_config[@]+"${tree_config[@]}"} --no-fail-fast --lib
      --config "target.'${runner_key}'.runner=${runner_config}")
    printf 'ALAB_UNIT_ROUND %s\n' "${result_round}"
    result_status=0
    invoke_unit_cargo "$@" || result_status=$?
    printf '%s\n' "${result_status}" >"${result_state}/round-${result_round}.status" || return $?
    [[ "${result_status}" -eq 0 || "${result_status}" -eq 101 ]] || return "${result_status}"
    [[ "${result_status}" -eq 0 ]] || result_overall="${result_status}"
  done
  return "${result_overall}"
)
