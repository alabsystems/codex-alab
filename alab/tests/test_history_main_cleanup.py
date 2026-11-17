# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""History main cleanup/publication with finite inert API and original-entry controls."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import test_history_cli_status as history_cli

HOOK = r'''
config = json.loads(Path(sys.argv[4]).read_text())
_faults = {name: {'io': OSError('controlled ' + name + ' failure'),
                  'zero': SystemExit(0), 'cancel': KeyboardInterrupt('controlled ' + name + ' cancellation')}[kind]
           for name, kind in config.get('failures', {}).items()}
_after_fault = {'zero': SystemExit(0), 'cancel': KeyboardInterrupt('controlled result cancellation')}.get(config.get('result_after'))
_emit_fault = {'zero': SystemExit(0), 'cancel': KeyboardInterrupt('controlled final emission'),
               'io': OSError('controlled final emission failure')}.get(config.get('emit_fault'))
_correction_fault = OSError('controlled corrective save failure')
_serve = value['weekly'].serve
def controlled_serve(actual):
    if 'serve' in _faults:
        witness['cleanup'].append('serve')
        raise _faults['serve']
    server, thread = _serve(actual)
    for owner, attribute, name in ((server, 'shutdown', 'shutdown'), (thread, 'join', 'join'),
                                    (server, 'server_close', 'close')):
        original = getattr(owner, attribute)
        def action(original=original, name=name):
            if name in _faults:
                witness['cleanup'].append(name)
                raise _faults[name]
            return original()
        setattr(owner, attribute, action)
    if 'after-transfer' in _faults:
        caller = sys._getframe(1)
        def interrupt_transfer(frame, event, _arg):
            if frame is caller and event == 'line':
                witness['cleanup'].append('after-transfer')
                raise _faults['after-transfer']
            return interrupt_transfer
        caller.f_trace = interrupt_transfer
        sys.settrace(interrupt_transfer)
    return server, thread
value['weekly'].serve = controlled_serve
_write = Path.write_text
def controlled_write(path, *args, **kwargs):
    phase = {'http.json': 'http', 'result.json': 'result'}.get(path.name) if path.parent == output else None
    if phase:
        witness['cleanup'].append(phase)
        if phase == 'result':
            witness['result_writes'] = witness.get('result_writes', 0) + 1
            assert witness['result_writes'] <= 3, 'unbounded corrective publication'
            if witness['result_writes'] > 1 and config.get('correction_fails'):
                raise _correction_fault
        if phase in _faults: raise _faults[phase]
    result = _write(path, *args, **kwargs)
    if phase == 'result' and _after_fault is not None: raise _after_fault
    return result
Path.write_text = controlled_write
_digest = value['weekly'].file_sha256
def controlled_digest(*args, **kwargs):
    witness['identity_checks'] = witness.get('identity_checks', 0) + 1
    if witness['identity_checks'] > 1:
        witness['cleanup'].append('identity')
        if 'identity' in _faults: raise _faults['identity']
    return _digest(*args, **kwargs)
value['weekly'].file_sha256 = controlled_digest
_tui = value['run_tui']
def controlled_tui(*args):
    _tui(*args)
    if config.get('change_binary'): binary.write_bytes(b'controlled changed inert binary')
value['run_tui'] = controlled_tui
_emit = value['emit']
def controlled_emit(packet):
    if packet.get('passed') is True:
        witness['success_emit_attempted'] = True
        witness['saved_at_success_emit'] = json.loads((output / 'result.json').read_text())['passed']
        if _emit_fault is not None:
            if config.get('emit_after'): _emit(packet)
            raise _emit_fault
    _emit(packet)
value['emit'] = controlled_emit
'''


class HistoryMainCleanupTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="alab-history-cleanup-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.sequence = 0

    def child(self, scenario="api_healthy", **config):
        root = self.root / str(self.sequence)
        self.sequence += 1
        root.mkdir()
        script, setup = root / "entry-child.py", root / "faults.json"
        setup.write_text(json.dumps(config))
        source = history_cli.CHILD
        self.assertEqual(source.count("report_fault = None"), 1)
        source = source.replace("report_fault = None", HOOK + "\nreport_fault = None")
        source = source.replace("    witness['escaped_type'] = type(error).__name__", """    witness['escaped_cleanup_identity'] = any(error is item for item in _faults.values())
    witness['escaped_publication_identity'] = error is _after_fault or error is _emit_fault
    witness['escaped_type'] = type(error).__name__""")
        script.write_text(source)
        done = subprocess.run([sys.executable, "-I", "-B", str(script), str(history_cli.SOURCE),
                               scenario, str(root), str(setup)], cwd=root,
                              capture_output=True, text=True, timeout=10)
        witness = json.loads((root / "witness.json").read_text())
        self.assertEqual(witness["process_attempts"], [])
        self.assertTrue(witness["controlled_boundaries_only"])
        self.assertEqual(witness["binary_unchanged"], not config.get("change_binary", False))
        return done, witness

    def assert_cleanup(self, witness, *, shutdown_failed=False, completed=False):
        expected = ["shutdown", *([] if shutdown_failed else ["join"]), "close", "http"]
        if completed:
            expected.append("identity")
        self.assertEqual(witness["cleanup"][:len(expected)], expected)
        self.assertIn("result", witness["cleanup"])

    def test_clean_main_publishes_success_only_after_complete_cleanup(self):
        done, witness = self.child()
        self.assertEqual(done.returncode, 0, done.stdout + done.stderr)
        self.assert_cleanup(witness, completed=True)
        self.assertTrue(witness["report"]["passed"])
        self.assertTrue(witness["saved_at_success_emit"])
        self.assertEqual(witness["result_writes"], 1)

    def test_failed_listener_transfer_saves_failed_state_without_unowned_cleanup(self):
        for scenario in ("api_healthy", "healthy"):
            for kind, status in (("io", 1), ("zero", 0), ("cancel", -2)):
                with self.subTest(scenario=scenario, kind=kind):
                    done, witness = self.child(scenario, failures={"serve": kind})
                    self.assertEqual(done.returncode, 1 if scenario == "healthy" and kind == "zero" else status,
                                     done.stdout + done.stderr)
                    self.assertEqual(witness["cleanup"], ["serve", "http", "result"])
                    self.assertFalse(witness["report"]["passed"])
                    self.assertNotIn("seed_entered", witness)
                    self.assertNotIn("success_emit_attempted", witness)
                    if scenario.startswith("api_"):
                        self.assertTrue(witness["escaped_cleanup_identity"])

    def test_cancellation_after_transfer_is_inside_the_ownership_guard(self):
        for kind, status in (("zero", 0), ("cancel", -2)):
            with self.subTest(kind=kind):
                done, witness = self.child(failures={"after-transfer": kind})
                self.assertEqual(done.returncode, status, done.stdout + done.stderr)
                self.assertTrue(witness["escaped_cleanup_identity"])
                self.assertEqual(witness["cleanup"], ["after-transfer", "shutdown", "join", "close", "http", "result"])
                self.assertFalse(witness["report"]["passed"])
                self.assertTrue(witness["report"]["interrupted"])
                self.assertNotIn("seed_entered", witness)

    def test_body_cancellation_survives_each_ordinary_cleanup_or_export_failure(self):
        for scenario, status in (("api_zero", 0), ("api_nonzero", 47), ("api_keyboard", -2)):
            for phase in ("shutdown", "join", "close", "http"):
                with self.subTest(scenario=scenario, phase=phase):
                    done, witness = self.child(scenario, failures={phase: "io"})
                    self.assertEqual(done.returncode, status, done.stdout + done.stderr)
                    self.assertTrue(witness["escaped_primary_identity"])
                    self.assert_cleanup(witness, shutdown_failed=phase == "shutdown")
                    self.assertFalse(witness["report"]["passed"])
                    self.assertTrue(witness["report"]["interrupted"])
                    self.assertIn("controlled " + phase, json.dumps(witness["report"]))

    def test_ordinary_body_and_cleanup_failures_leave_failed_reports(self):
        for scenario in ("api_ordinary", "api_healthy"):
            for phase in ("shutdown", "join", "close", "http", "identity"):
                if scenario == "api_ordinary" and phase == "identity":
                    continue
                with self.subTest(scenario=scenario, phase=phase):
                    done, witness = self.child(scenario, failures={phase: "io"})
                    self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
                    self.assert_cleanup(witness, shutdown_failed=phase == "shutdown",
                                        completed=scenario == "api_healthy")
                    self.assertFalse(witness["report"]["passed"])
                    self.assertNotIn("success_emit_attempted", witness)
                    if scenario == "api_ordinary":
                        self.assertTrue(witness["escaped_primary_identity"])

    def test_cleanup_control_promotes_over_an_ordinary_body_error(self):
        done, witness = self.child("api_ordinary", failures={"close": "cancel"})
        self.assertEqual(done.returncode, -2, done.stdout + done.stderr)
        self.assertTrue(witness["escaped_cleanup_identity"])
        self.assertFalse(witness["escaped_primary_identity"])
        self.assert_cleanup(witness)
        self.assertFalse(witness["report"]["passed"])
        self.assertTrue(witness["report"]["interrupted"])
        self.assertIn("controlled seed failure", json.dumps(witness["report"]))

    def test_selected_cancellation_survives_a_failed_final_save(self):
        for scenario, status in (("api_zero", 0), ("api_nonzero", 47), ("api_keyboard", -2)):
            for after in (False, True):
                with self.subTest(scenario=scenario, after=after):
                    config = {"result_after": "zero"} if after else {"failures": {"result": "io"}}
                    done, witness = self.child(scenario, **config)
                    self.assertEqual(done.returncode, status, done.stdout + done.stderr)
                    self.assertTrue(witness["escaped_primary_identity"])
                    self.assert_cleanup(witness)
                    self.assertEqual(witness["result_writes"], 1)
                    if after:
                        self.assertFalse(witness["report"]["passed"])
                    else:
                        self.assertIsNone(witness["report"])
                    self.assertNotIn("success_emit_attempted", witness)

    def test_interrupted_successful_result_save_gets_one_failed_correction(self):
        for kind, status in (("zero", 0), ("cancel", -2)):
            with self.subTest(kind=kind):
                done, witness = self.child(result_after=kind)
                self.assertEqual(done.returncode, status, done.stdout + done.stderr)
                self.assertTrue(witness["escaped_publication_identity"])
                self.assert_cleanup(witness, completed=True)
                self.assertFalse(witness["report"]["passed"])
                self.assertTrue(witness["report"]["interrupted"])
                self.assertEqual(witness["result_writes"], 2)
                self.assertNotIn("success_emit_attempted", witness)

    def test_final_success_emit_cancellation_corrects_report_and_keeps_identity(self):
        for kind, status in (("zero", 0), ("cancel", -2)):
            for after in (False, True):
                with self.subTest(kind=kind, after=after):
                    done, witness = self.child(emit_fault=kind, emit_after=after)
                    self.assertEqual(done.returncode, status, done.stdout + done.stderr)
                    self.assertTrue(witness["escaped_publication_identity"])
                    self.assertTrue(witness["saved_at_success_emit"])
                    self.assertFalse(witness["report"]["passed"])
                    self.assertEqual(witness["result_writes"], 2)

    def test_original_cli_never_qualifies_failed_cleanup_or_publication(self):
        controls = [
            ("zero", {"failures": {"shutdown": "io"}}, 1),
            ("nonzero", {"failures": {"close": "io"}}, 47),
            ("keyboard", {"failures": {"http": "io"}}, -2),
            ("healthy", {"failures": {"join": "io"}}, 1),
            ("healthy", {"result_after": "zero"}, 1),
            ("healthy", {"emit_fault": "zero", "emit_after": True}, 1),
            ("healthy", {"result_after": "zero", "correction_fails": True}, 1),
            ("healthy", {"emit_fault": "zero", "correction_fails": True}, 1),
        ]
        for scenario, config, status in controls:
            with self.subTest(scenario=scenario, config=config):
                done, witness = self.child(scenario, **config)
                self.assertEqual(done.returncode, status, done.stdout + done.stderr)
                self.assertTrue(witness["original_entry_executed"])
                self.assertFalse(done.returncode == 0 and witness["report"]
                                 and witness["report"]["passed"])
                if not config.get("correction_fails"):
                    self.assertIsNotNone(witness["report"])
                    self.assertFalse(witness["report"]["passed"])

    def test_completed_identity_check_still_fails_after_an_export_failure(self):
        done, witness = self.child(change_binary=True, failures={"http": "io"})
        self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
        self.assert_cleanup(witness, completed=True)
        self.assertFalse(witness["report"]["passed"])
        self.assertIn("immutable binary SHA changed", json.dumps(witness["report"]))
        self.assertNotIn("success_emit_attempted", witness)


if __name__ == "__main__":
    unittest.main()
