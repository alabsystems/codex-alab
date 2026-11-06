#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Check which of tippy's diagnostics alab/lint.jq (alab/check.sh, step 4) calls ALab's."""

import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
REPO = "/work/codex-alab/"
LOGICAL = "/logical/codex-alab/"
PRISTINE = "/work/codex-alab/.git/alab-pristine/"
HOOKS = (
    "codex-rs/cli/src/main.rs:40:ALAB(cli):\n"
    "codex-rs/config/src/strict_config.rs:126:ALAB(progress):\n"
)


def span(file_name, line_start, line_end=None, primary=True, expansion=None):
    return {
        "file_name": file_name, "line_start": line_start,
        "line_end": line_start if line_end is None else line_end,
        "column_start": 5, "is_primary": primary,
        "expansion": None if expansion is None else {"span": expansion},
    }


def message(text, spans, level="warning", code="clippy::map_clone",
            manifest=REPO + "codex-rs/cli/Cargo.toml", kind="lib", name="codex_cli"):
    return json.dumps({
        "reason": "compiler-message", "manifest_path": manifest,
        "target": {"kind": [kind], "name": name, "src_path": "x.rs", "test": True},
        "message": {"level": level, "message": text, "rendered": text + "\n",
                    "code": None if code is None else {"code": code}, "spans": spans},
    })


def artifact(package, manifest, kind="custom-build", name="build-script-build"):
    """cargo's record of a unit it built (a build script compiled, for custom-build)."""
    return json.dumps({
        "reason": "compiler-artifact", "package_id": package, "manifest_path": manifest,
        "target": {"kind": [kind], "name": name, "src_path": "build.rs", "test": False},
        "filenames": [], "fresh": True,
    })


def executed(package):
    """cargo's record of a build script that ran to completion (fresh or not)."""
    return json.dumps({"reason": "build-script-executed", "package_id": package,
                       "linked_libs": [], "linked_paths": [], "cfgs": [], "env": [],
                       "out_dir": "/out"})


CLI = "path+file:///work/codex-alab/codex-rs/cli#codex-cli@0.0.0"
CLI_UPSTREAM = "path+file:///work/codex-alab/.git/alab-pristine/codex-rs/cli#codex-cli@0.0.0"
CORE = "path+file:///work/codex-alab/codex-rs/core#codex-core@0.0.0"
SCRIPTED = [REPO + "codex-rs/cli/Cargo.toml", LOGICAL + "codex-rs/alab-core/Cargo.toml"]


@unittest.skipUnless(shutil.which("jq"), "requires jq")
class LintOwnedTests(unittest.TestCase):
    def verdict(self, lines, pristine=None, scripted=()):
        with tempfile.TemporaryDirectory(prefix="alab-lint-") as directory:
            hooks = Path(directory) / "hooks"
            hooks.write_text(HOOKS)
            theirs = Path(directory) / "pristine"
            theirs.write_text("" if pristine is None else "\n".join(pristine) + "\n")
            result = subprocess.run(
                ["jq", "-R", "-n", "-c", "-f", str(ROOT / "alab/lint.jq"),
                 "--rawfile", "hooks", str(hooks), "--rawfile", "pristine", str(theirs),
                 "--argjson", "roots", json.dumps([REPO, LOGICAL]),
                 "--argjson", "proots", json.dumps([PRISTINE]),
                 "--arg", "ws", "codex-rs/",
                 "--argjson", "compared", "false" if pristine is None else "true",
                 "--argjson", "scripted", json.dumps(list(scripted))],
                input="\n".join(lines) + "\n", capture_output=True, text=True,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            return json.loads(result.stdout)

    def test_glue_reached_through_an_upstream_crate_is_alabs(self):
        verdict = self.verdict([
            message("glue", [span("tui/src/../../alab-tui/glue/x.rs", 3)]),
            message("crate", [span("alab-core/src/lib.rs", 9)]),
        ])
        self.assertEqual(verdict["owned"], ["crate\n", "glue\n"])
        self.assertEqual(verdict["set_aside"], 0)

    def test_a_package_outside_the_workspace_is_named_absolutely_and_alab_carries_it(self):
        verdict = self.verdict([
            message("vendored", [span(REPO + "alab/vendor/allocative/src/lib.rs", 7)]),
            message("logical", [span(LOGICAL + "alab/vendor/allocative/build.rs", 2)]),
            message("registry", [span("/home/u/.cargo/registry/src/alab/lib.rs", 1)]),
        ])
        self.assertEqual(verdict["owned"], ["logical\n", "vendored\n"])
        self.assertEqual(verdict["set_aside"], 1)

    def test_a_span_that_starts_on_a_hook_line_is_alabs(self):
        verdict = self.verdict([
            message("on the hook", [span("cli/src/main.rs", 40)]),
            message("next to it", [span("cli/src/main.rs", 41)]),
        ])
        self.assertEqual(verdict["owned"], ["on the hook\n"])
        self.assertEqual(verdict["spanning"], [])
        self.assertEqual(verdict["set_aside"], 1)

    def test_a_macro_called_from_glue_is_alabs(self):
        call_site = span("tui/src/../../alab-tui/glue/x.rs", 12, primary=False)
        verdict = self.verdict([
            message("in expansion", [span("/rustlib/src/fmt.rs", 400, expansion=call_site)]),
        ])
        self.assertEqual(verdict["owned"], ["in expansion\n"])

    def test_a_chain_a_hook_joins_midway_waits_for_pristine_upstream(self):
        chain = message("chain", [span("config/src/strict_config.rs", 123, 128)],
                        manifest=REPO + "codex-rs/config/Cargo.toml")
        verdict = self.verdict([chain])
        self.assertEqual(verdict["owned"], [])
        self.assertEqual(verdict["spanning"], [
            {"rendered": "chain\n", "manifest": "codex-rs/config/Cargo.toml", "ours": None}])
        self.assertEqual(verdict["set_aside"], 0)

    def test_a_chain_upstream_lints_too_is_set_aside(self):
        chain = message("chain", [span("config/src/strict_config.rs", 123, 128)],
                        manifest=REPO + "codex-rs/config/Cargo.toml")
        upstream = message("chain", [span("config/src/strict_config.rs", 123, 127)],
                           manifest=PRISTINE + "codex-rs/config/Cargo.toml")
        verdict = self.verdict([chain], pristine=[upstream])
        self.assertEqual(verdict["spanning"][0]["ours"], False)
        self.assertEqual(verdict["set_aside"], 1)

    def test_a_chain_only_alabs_layer_lints_gates(self):
        chain = message("chain", [span("config/src/strict_config.rs", 123, 128)],
                        manifest=REPO + "codex-rs/config/Cargo.toml")
        elsewhere = message("chain", [span("config/src/strict_config.rs", 20)],
                            manifest=REPO + "codex-rs/config/Cargo.toml")
        upstream = message("chain", [span("config/src/strict_config.rs", 20)],
                           manifest=PRISTINE + "codex-rs/config/Cargo.toml")
        verdict = self.verdict([chain, elsewhere], pristine=[upstream])
        self.assertEqual(verdict["spanning"][0]["ours"], True)
        self.assertEqual(verdict["set_aside"], 1)

    def test_a_target_that_fails_on_upstream_too_is_upstreams(self):
        error = message("broken bench", [span("core/benches/b.rs", 4)], level="error",
                        code="E0308", manifest=REPO + "codex-rs/core/Cargo.toml",
                        kind="bench", name="b")
        summary = message("aborting", [], level="error", code=None,
                          manifest=REPO + "codex-rs/core/Cargo.toml", kind="bench", name="b")
        alone = message("broken bench", [span("core/benches/b.rs", 4)], level="error",
                        code="E0308", manifest=PRISTINE + "codex-rs/core/Cargo.toml",
                        kind="bench", name="b")
        before = self.verdict([error, summary])
        self.assertEqual(before["owned"], [])
        self.assertEqual(before["failing"], [{
            "manifest": "codex-rs/core/Cargo.toml", "target": {"kind": "bench", "name": "b"},
            "errors": ["aborting\n", "broken bench\n"], "upstream_too": None}])
        self.assertEqual(self.verdict([error, summary], pristine=[alone])["failing"][0]["upstream_too"], True)
        self.assertEqual(self.verdict([error, summary], pristine=[])["failing"][0]["upstream_too"], False)

    def test_a_build_script_that_did_not_run_is_its_own_finding(self):
        # cli's build script compiled and failed when it ran, so cargo built nothing else of
        # cli and said so only in prose; core's bench fails on upstream too.
        bench = message("broken bench", [span("core/benches/b.rs", 4)], level="error",
                        code="E0308", manifest=REPO + "codex-rs/core/Cargo.toml",
                        kind="bench", name="b")
        bench_upstream = message("broken bench", [span("core/benches/b.rs", 4)], level="error",
                                 code="E0308", manifest=PRISTINE + "codex-rs/core/Cargo.toml",
                                 kind="bench", name="b")
        ours = [artifact(CLI, REPO + "codex-rs/cli/Cargo.toml"),
                artifact(CORE, REPO + "codex-rs/core/Cargo.toml"), executed(CORE), bench]
        alab_core = {"manifest": "codex-rs/alab-core/Cargo.toml", "built": False}
        before = self.verdict(ours, scripted=SCRIPTED)
        self.assertEqual(before["owned"], [])
        self.assertEqual(before["scripts"], [
            alab_core | {"upstream_too": None},
            {"manifest": "codex-rs/cli/Cargo.toml", "built": True, "upstream_too": None}])
        cli_upstream = PRISTINE + "codex-rs/cli/Cargo.toml"
        runs_there = self.verdict(ours, scripted=SCRIPTED, pristine=[
            artifact(CLI_UPSTREAM, cli_upstream), executed(CLI_UPSTREAM), bench_upstream])
        self.assertEqual(runs_there["failing"][0]["upstream_too"], True)
        self.assertEqual([s["upstream_too"] for s in runs_there["scripts"]], [False, False])
        fails_there = self.verdict(ours, scripted=SCRIPTED, pristine=[
            artifact(CLI_UPSTREAM, cli_upstream), bench_upstream])
        self.assertEqual(fails_there["scripts"][1]["upstream_too"], True)
        # Only evidence counts: a package pristine upstream did not build gates.
        self.assertEqual(fails_there["scripts"][0]["upstream_too"], False)
        self.assertEqual(self.verdict(ours, scripted=SCRIPTED, pristine=[])["scripts"][1]
                         ["upstream_too"], False)

    def test_a_build_script_that_ran_or_is_not_linted_is_no_finding(self):
        registry = "registry+https://github.com/rust-lang/crates.io-index#ring@0.17.8"
        verdict = self.verdict([
            artifact(CLI, REPO + "codex-rs/cli/Cargo.toml"), executed(CLI),
            artifact("path+file:///logical/codex-alab/codex-rs/alab-core#codex-alab-core@0.1.0",
                     LOGICAL + "codex-rs/alab-core/Cargo.toml"),
            executed("path+file:///logical/codex-alab/codex-rs/alab-core#codex-alab-core@0.1.0"),
            # A dependency's script that failed is no linted package's.
            artifact(registry, "/home/u/.cargo/registry/src/ring-0.17.8/Cargo.toml"),
        ], scripted=SCRIPTED)
        self.assertEqual(verdict["scripts"], [])
        # Another package's run is not this one's.
        self.assertEqual(self.verdict([artifact(CLI, REPO + "codex-rs/cli/Cargo.toml"),
                                       executed(CORE)], scripted=SCRIPTED[:1])["scripts"],
                         [{"manifest": "codex-rs/cli/Cargo.toml", "built": True,
                           "upstream_too": None}])

    def test_a_build_script_that_does_not_compile_is_a_failing_target(self):
        error = message("bad build.rs", [span("cli/build.rs", 2)], level="error", code="E0425",
                        kind="custom-build", name="build-script-build")
        verdict = self.verdict([error], scripted=SCRIPTED[:1])
        self.assertEqual(verdict["scripts"], [])
        self.assertEqual(verdict["failing"][0]["target"],
                         {"kind": "custom-build", "name": "build-script-build"})

    def test_each_place_counts_once_and_other_lines_are_skipped(self):
        twice = message("twice", [span("tui/src/a.rs", 5)])
        verdict = self.verdict([twice, twice, "   Compiling x v1", message("note", [], level="note")])
        self.assertEqual(verdict["set_aside"], 1)
        self.assertEqual(verdict["failing"], [])


if __name__ == "__main__":
    unittest.main()
