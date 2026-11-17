# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""History CLI accounting through actual main with inert process/listener seams."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SOURCE = Path(__file__).resolve().parents[1] / "smoke/history_pagination.py"
CHILD = r'''
import ast, hashlib, json, runpy, sys
from pathlib import Path
from types import SimpleNamespace
path, scenario, root = Path(sys.argv[1]), sys.argv[2], Path(sys.argv[3])
value = runpy.run_path(str(path))['main'].__globals__
api = scenario.startswith('api_')
if api: scenario = scenario[4:]
faults = {'zero': SystemExit(0), 'none': SystemExit(None), 'false': SystemExit(False),
          'wrapped': SystemExit(256), 'negative_wrapped': SystemExit(-256),
          'nonzero': SystemExit(47), 'floating_zero': SystemExit(0.0),
          'huge': SystemExit(sys.maxsize + 1), 'keyboard': KeyboardInterrupt('controlled interruption')}
primary = faults.get(scenario)
if scenario.startswith(('report_', 'output_')): primary = SystemExit(0)
if scenario == 'ordinary': primary = RuntimeError('controlled seed failure')
if scenario == 'unprintable':
    class Unprintable(RuntimeError):
        def __str__(self): raise SystemExit(0)
    primary = Unprintable()
if scenario in ('ordinary_output_zero', 'keyboard_output_zero', 'nonzero_output_zero'):
    primary = {'ordinary_output_zero': RuntimeError('controlled seed failure'),
               'keyboard_output_zero': KeyboardInterrupt('controlled interruption'),
               'nonzero_output_zero': SystemExit(47)}[scenario]
binary = root / 'unexecuted-codex'
binary.write_bytes(b'owned inert data; never executed\n')
initial = hashlib.sha256(binary.read_bytes()).hexdigest()
output = root / 'output'
witness = {'cleanup': [], 'process_attempts': [], 'controlled_boundaries_only': True,
           'original_entry_executed': not api}
model = None
def serve(actual):
    global model
    model = actual
    return (SimpleNamespace(server_port=1234, shutdown=lambda: witness['cleanup'].append('shutdown'),
                            server_close=lambda: witness['cleanup'].append('close')),
            SimpleNamespace(join=lambda: witness['cleanup'].append('join')))
def forbidden(*args, **kwargs):
    witness['process_attempts'].append('forbidden')
    raise AssertionError('controlled CLI fixture attempted an application process')
class Rpc:
    def __init__(self, *_args): pass
    def initialize(self): pass
    def request(self, method, *_args):
        return {'thread':{'id':'controlled-history'}} if method == 'thread/start' else {'data':[]}
    def turn(self, *_args):
        model.models.append({'controlled':True})
        return {'status':'completed'}
def run(_binary, _args, _root, _env, _label, *, interact):
    witness['seed_entered'] = True
    if primary is not None: raise primary
    interact(None, None)
    return 23 if scenario == 'native_refusal' else 0
def tui(_env, state):
    witness['tui_entered'] = True
    state.update(older_history_page_requests=3, tui_bytes=10)
value['weekly'].serve = serve
value['probe'].run = run
value['resume'].Rpc = Rpc
value['run_tui'] = tui
value['subprocess'].Popen = forbidden
value['pty'].fork = forbidden
report_fault = None
if scenario.startswith(('report_', 'output_')) or scenario.endswith('_output_zero'):
    report_fault = (SystemExit(0) if scenario.endswith('_output_zero') else
                    faults[scenario.split('_', 1)[1]])
    class FailingSink:
        def write(self, _text): raise report_fault
        def flush(self): pass
    if scenario.startswith('report_'): sys.stderr = FailingSink()
    else: sys.stdout = FailingSink()
arguments = [str(path), '--codex', str(binary), '--output', str(output)]
sys.argv = ([str(path), '--help'] if scenario == 'help' else
            [str(path)] if scenario == 'arguments' else arguments)
tree = ast.parse(path.read_bytes(), filename=str(path))
entries = [node for node in tree.body if isinstance(node, ast.If)
           and isinstance(node.test, ast.Compare) and isinstance(node.test.left, ast.Name)
           and node.test.left.id == '__name__']
assert len(entries) == 1
entry = ast.Module(body=entries, type_ignores=[])
witness['entry_ast_sha256'] = hashlib.sha256(ast.dump(entry).encode()).hexdigest()
value['__name__'] = '__main__'
try:
    if api: value['main']()
    else: exec(compile(entry, str(path), 'exec'), value)
except BaseException as error:
    witness['escaped_type'] = type(error).__name__
    witness['escaped_code'] = error.code if isinstance(error, SystemExit) else None
    witness['escaped_primary_identity'] = error is primary
    witness['escaped_report_fault_identity'] = error is report_fault
    raise
finally:
    witness['output_exists'] = output.exists()
    witness['report'] = (json.loads((output / 'result.json').read_text())
                         if (output / 'result.json').is_file() else None)
    witness['binary_unchanged'] = hashlib.sha256(binary.read_bytes()).hexdigest() == initial
    witness['policy_source'] = value['weekly'].CLI['__file__']
    (root / 'witness.json').write_text(json.dumps(witness))
'''


class HistoryCliStatusTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="alab-history-cli-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.sequence = 0

    def child(self, scenario):
        root = self.root / str(self.sequence)
        self.sequence += 1
        root.mkdir()
        script = root / "entry-child.py"
        script.write_text(CHILD)
        done = subprocess.run([sys.executable, "-I", "-B", str(script), str(SOURCE), scenario, str(root)],
                              cwd=root, capture_output=True, text=True, timeout=10)
        witness = json.loads((root / "witness.json").read_text())
        self.assertEqual(witness["process_attempts"], [])
        self.assertTrue(witness["binary_unchanged"])
        self.assertTrue(witness["controlled_boundaries_only"])
        self.assertEqual(len(witness["entry_ast_sha256"]), 64)
        self.assertEqual(Path(witness["policy_source"]).resolve(), SOURCE.parents[1] / "cli_status.py")
        return done, witness

    def assert_failed_observation(self, witness):
        self.assertTrue(witness["seed_entered"])
        self.assertEqual(witness["cleanup"], ["shutdown", "join", "close"])
        self.assertFalse(witness["report"]["passed"])

    def test_public_main_keeps_success_shaped_cancellation_identity(self):
        for scenario in ("api_zero", "api_none", "api_false", "api_wrapped", "api_negative_wrapped"):
            with self.subTest(scenario=scenario):
                done, witness = self.child(scenario)
                self.assertEqual(done.returncode, 0, done.stdout + done.stderr)
                self.assert_failed_observation(witness)
                self.assertFalse(witness["original_entry_executed"])
                self.assertTrue(witness["escaped_primary_identity"])

    def test_original_cli_refuses_success_shaped_observation_exits(self):
        for scenario in ("zero", "none", "false", "wrapped", "negative_wrapped"):
            with self.subTest(scenario=scenario):
                done, witness = self.child(scenario)
                self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
                self.assert_failed_observation(witness)
                self.assertTrue(witness["original_entry_executed"])
                self.assertFalse(witness["escaped_primary_identity"])
                self.assertIn("incomplete", done.stderr.lower())
                failed = [json.loads(line) for line in done.stdout.splitlines() if line]
                self.assertEqual(len(failed), 1)
                self.assertIn("failed", failed[0])

    def test_genuine_nonzero_and_keyboard_exits_keep_primary_identity(self):
        for scenario, status in (("nonzero", 47), ("floating_zero", 1), ("huge", 255),
                                 ("keyboard", -2), ("ordinary", 1), ("unprintable", 1)):
            with self.subTest(scenario=scenario):
                done, witness = self.child(scenario)
                self.assertEqual(done.returncode, status, done.stdout + done.stderr)
                self.assert_failed_observation(witness)
                self.assertTrue(witness["escaped_primary_identity"])

    def test_diagnostic_success_exits_cannot_hide_failed_observations(self):
        for scenario in ("report_zero", "report_none", "report_wrapped", "output_zero", "output_wrapped"):
            with self.subTest(scenario=scenario):
                done, witness = self.child(scenario)
                self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
                self.assert_failed_observation(witness)

    def test_failed_json_emission_cannot_replace_a_nonzero_primary(self):
        for scenario, status in (("ordinary_output_zero", 1), ("keyboard_output_zero", -2),
                                 ("nonzero_output_zero", 47)):
            with self.subTest(scenario=scenario):
                done, witness = self.child(scenario)
                self.assertEqual(done.returncode, status, done.stdout + done.stderr)
                self.assert_failed_observation(witness)
                self.assertTrue(witness["escaped_primary_identity"])

    def test_new_nonzero_reporting_interruptions_keep_their_identity(self):
        for scenario, status in (("report_nonzero", 47), ("report_keyboard", -2)):
            with self.subTest(scenario=scenario):
                done, witness = self.child(scenario)
                self.assertEqual(done.returncode, status, done.stdout + done.stderr)
                self.assert_failed_observation(witness)
                self.assertTrue(witness["escaped_report_fault_identity"])

    def test_help_and_argument_errors_do_not_start_observations(self):
        for scenario, status in (("help", 0), ("arguments", 2)):
            with self.subTest(scenario=scenario):
                done, witness = self.child(scenario)
                self.assertEqual(done.returncode, status, done.stdout + done.stderr)
                self.assertFalse(witness["output_exists"])
                self.assertNotIn("seed_entered", witness)
                self.assertEqual(witness["cleanup"], [])
                self.assertIsNone(witness["report"])
                self.assertNotIn('"failed"', done.stdout)

    def test_success_and_checked_process_refusal_keep_their_verdict(self):
        for scenario, status in (("healthy", 0), ("native_refusal", 1)):
            with self.subTest(scenario=scenario):
                done, witness = self.child(scenario)
                self.assertEqual(done.returncode, status, done.stdout + done.stderr)
                self.assertEqual(witness["report"]["passed"], scenario == "healthy")
                self.assertEqual(witness["cleanup"], ["shutdown", "join", "close"])
                self.assertEqual(witness.get("tui_entered", False), scenario == "healthy")

    def test_early_isolated_guardian_path_needs_no_adjacent_policy_or_app_modules(self):
        root = self.root / "guardian-only"
        root.mkdir()
        source = root / "history_pagination.py"
        source.write_bytes(SOURCE.read_bytes())
        raw, result, spec = root / "raw", root / "custody.json", root / "spec.json"
        raw.write_bytes(b"preserve prior evidence")
        spec.write_text(json.dumps({"argv": ["never-executed"], "cwd": str(root), "env": {},
                                    "raw": str(raw), "result": str(result)}))
        program = '''import pty, runpy, sys
def forbidden(): raise AssertionError("isolated guardian fixture attempted a fork")
pty.fork = forbidden
source, spec = sys.argv[1:]
sys.argv = [source, "--guardian", spec]
runpy.run_path(source, run_name="__main__")
'''
        done = subprocess.run([sys.executable, "-I", "-B", "-S", "-c", program, str(source), str(spec)],
                              cwd=root, stdin=subprocess.DEVNULL, capture_output=True,
                              text=True, timeout=10)
        self.assertEqual(done.returncode, 1)
        self.assertIn("FileExistsError", done.stderr)
        self.assertNotIn("ModuleNotFoundError", done.stderr)
        self.assertNotIn("attempted a fork", done.stderr)
        self.assertEqual(raw.read_bytes(), b"preserve prior evidence")
        self.assertFalse(result.exists())


if __name__ == "__main__":
    unittest.main()
