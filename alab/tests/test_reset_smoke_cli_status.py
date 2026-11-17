# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Original CLI entries must not turn incomplete inert observations into success."""
import importlib.util
import io
import json
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

SMOKE = Path(__file__).resolve().parents[1] / "smoke"
sys.path.insert(0, str(SMOKE))
MODULES = {}
for name in ("weekly_reset", "quota_wait"):
    spec = importlib.util.spec_from_file_location("controlled_api_" + name, SMOKE / (name + ".py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    MODULES[name] = module

CHILD = r'''
import ast, hashlib, importlib.util, json, sys
from pathlib import Path
path, scenario, root = Path(sys.argv[1]), sys.argv[2], Path(sys.argv[3])
sys.path.insert(0, str(path.parent))
spec = importlib.util.spec_from_file_location('controlled_reset_smoke', path)
value = importlib.util.module_from_spec(spec)
spec.loader.exec_module(value)
weekly = path.stem == 'weekly_reset'
names = ['late-credit', 'lost-ack'] if weekly else ['repeated-stale-reset', 'cancel-distant-reset']
faults = {'zero': SystemExit(0), 'none': SystemExit(None), 'false': SystemExit(False),
          'wrapped': SystemExit(256), 'negative_wrapped': SystemExit(-256),
          'nonzero': SystemExit(47), 'floating_zero': SystemExit(0.0),
          'huge': SystemExit(sys.maxsize + 1), 'keyboard': KeyboardInterrupt('controlled interruption')}
primary = faults.get(scenario)
if scenario.startswith(('report_', 'output_')):
    primary = SystemExit(0)
if scenario == 'ordinary':
    primary = RuntimeError('controlled case refusal')
report_fault = None
binary = root / 'unexecuted-codex'
binary.write_bytes(b'owned fixture data; never executed\n')
initial = hashlib.sha256(binary.read_bytes()).hexdigest()
output = root / 'output'
witness = {'called': [], 'returned': [], 'controlled_cases_only': True}
def controlled_case(_codex, destination, selector):
    name = selector if weekly else names[int(selector)]
    witness['called'].append(name)
    (destination / name).mkdir()
    if len(witness['called']) == 2 and primary is not None:
        raise primary
    witness['returned'].append(name)
    return {'case': name, 'passed': not (scenario == 'returned_refusal'
                                        and len(witness['called']) == 2)}
value.case = controlled_case
if scenario.startswith(('report_', 'output_')):
    report_fault = faults[scenario.split('_', 1)[1]]
    class FailingSink:
        def write(self, _text): raise report_fault
        def flush(self): pass
    if scenario.startswith('report_'):
        sys.stderr = FailingSink()
    else:
        sys.stdout = FailingSink()
arguments = [str(path), '--codex', str(binary), '--output', str(output)]
if weekly:
    arguments += ['--case', names[0], '--case', names[1]]
sys.argv = ([str(path), '--help'] if scenario == 'help' else
            [str(path)] if scenario == 'arguments' else arguments)
tree = ast.parse(path.read_bytes(), filename=str(path))
entries = [node for node in tree.body if isinstance(node, ast.If)
           and isinstance(node.test, ast.Compare) and isinstance(node.test.left, ast.Name)
           and node.test.left.id == '__name__']
assert len(entries) == 1
entry = ast.Module(body=entries, type_ignores=[])
witness['entry_ast_sha256'] = hashlib.sha256(ast.dump(entry).encode()).hexdigest()
value.__name__ = '__main__'
try:
    exec(compile(entry, str(path), 'exec'), value.__dict__)
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
    (root / 'witness.json').write_text(json.dumps(witness))
'''


class FailingSink(io.StringIO):
    def write(self, _text):
        raise SystemExit(256)


class ResetSmokeCliStatusTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="alab-reset-cli-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.sequence = 0

    def child(self, name, scenario):
        root = self.root / str(self.sequence)
        self.sequence += 1
        root.mkdir()
        script = root / "entry-child.py"
        script.write_text(CHILD)
        done = subprocess.run([sys.executable, "-I", "-B", str(script),
                               str(SMOKE / (name + ".py")), scenario, str(root)],
                              cwd=root, capture_output=True, text=True, timeout=10)
        witness = json.loads((root / "witness.json").read_text())
        self.assertTrue(witness["binary_unchanged"])
        self.assertTrue(witness["controlled_cases_only"])
        self.assertEqual(len(witness["entry_ast_sha256"]), 64)
        return done, witness

    def assert_partial(self, name, witness):
        names = ["late-credit", "lost-ack"] if name == "weekly_reset" else [
            "repeated-stale-reset", "cancel-distant-reset"]
        self.assertEqual(witness["called"], names)
        self.assertEqual(witness["returned"], names[:1])
        report = witness["report"]
        self.assertFalse(report["passed"])
        self.assertTrue(report["interrupted"])
        self.assertEqual(report["failedCase" if name == "weekly_reset" else "activeCase"], names[1])
        self.assertEqual(report["cases"], [{"case": names[0], "passed": True}])

    def api_interruption(self, name, primary, sink=None):
        root = self.root / str(self.sequence)
        self.sequence += 1
        root.mkdir()
        binary = root / "unexecuted-codex"
        binary.write_bytes(b"owned inert fixture\n")
        output = root / "output"
        names = ["late-credit", "lost-ack"] if name == "weekly_reset" else [
            "repeated-stale-reset", "cancel-distant-reset"]
        called, returned = [], []

        def case(_codex, destination, selector):
            selected = selector if name == "weekly_reset" else names[int(selector)]
            called.append(selected)
            (destination / selected).mkdir()
            if len(called) == 2:
                raise primary
            returned.append(selected)
            return {"case": selected, "passed": True}

        args = [name + ".py", "--codex", str(binary), "--output", str(output)]
        if name == "weekly_reset":
            args += ["--case", names[0], "--case", names[1]]
        with patch.object(sys, "argv", args), patch.object(MODULES[name], "case", case), \
                redirect_stdout(io.StringIO() if sink is None else sink), redirect_stderr(io.StringIO()):
            with self.assertRaises(BaseException) as caught:
                MODULES[name].main()
        self.assertIs(caught.exception, primary)
        self.assert_partial(name, {"called": called, "returned": returned,
                                   "report": json.loads((output / "result.json").read_text())})

    def test_api_main_keeps_success_shaped_cancellation_identity(self):
        for name in MODULES:
            for code in (0, None, False, 256, -256):
                with self.subTest(name=name, code=code):
                    self.api_interruption(name, SystemExit(code))

    def test_api_main_keeps_nonzero_and_keyboard_identity_despite_output_failure(self):
        for name in MODULES:
            for primary in (SystemExit(47), KeyboardInterrupt("controlled"), SystemExit(0)):
                with self.subTest(name=name, primary=repr(primary)):
                    self.api_interruption(name, primary, FailingSink())

    def test_original_entries_refuse_success_shaped_case_interruptions(self):
        for name in MODULES:
            for scenario in ("zero", "none", "false", "wrapped", "negative_wrapped"):
                with self.subTest(name=name, scenario=scenario):
                    done, witness = self.child(name, scenario)
                    self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
                    self.assert_partial(name, witness)
                    self.assertFalse(witness["escaped_primary_identity"])
                    self.assertIn("incomplete", done.stderr.lower())

    def test_original_entries_keep_nonzero_float_huge_and_keyboard_interruptions(self):
        for name in MODULES:
            for scenario, status in (("nonzero", 47), ("floating_zero", 1), ("huge", 255), ("keyboard", -2)):
                with self.subTest(name=name, scenario=scenario):
                    done, witness = self.child(name, scenario)
                    self.assertEqual(done.returncode, status, done.stdout + done.stderr)
                    self.assert_partial(name, witness)
                    self.assertTrue(witness["escaped_primary_identity"])

    def test_help_and_argument_errors_do_not_begin_observations(self):
        for name in MODULES:
            for scenario, status in (("help", 0), ("arguments", 2)):
                with self.subTest(name=name, scenario=scenario):
                    done, witness = self.child(name, scenario)
                    self.assertEqual(done.returncode, status, done.stdout + done.stderr)
                    self.assertEqual(witness["called"], [])
                    self.assertFalse(witness["output_exists"])
                    self.assertIsNone(witness["report"])

    def test_normal_success_and_case_refusals_keep_their_status(self):
        for name in MODULES:
            for scenario, status in (("healthy", 0), ("ordinary", 1), ("returned_refusal", 1)):
                with self.subTest(name=name, scenario=scenario):
                    done, witness = self.child(name, scenario)
                    self.assertEqual(done.returncode, status, done.stdout + done.stderr)
                    self.assertEqual(witness["report"]["passed"], scenario == "healthy")
                    self.assertNotIn("interrupted", witness["report"])

    def test_success_shaped_reporting_failures_cannot_restore_success(self):
        for name in MODULES:
            for scenario in ("report_zero", "report_none", "report_wrapped", "report_negative_wrapped",
                             "output_zero", "output_wrapped"):
                with self.subTest(name=name, scenario=scenario):
                    done, witness = self.child(name, scenario)
                    self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
                    self.assert_partial(name, witness)

    def test_nonzero_reporting_interruptions_keep_the_new_exception_identity(self):
        for name in MODULES:
            for scenario, status in (("report_nonzero", 47), ("report_keyboard", -2)):
                with self.subTest(name=name, scenario=scenario):
                    done, witness = self.child(name, scenario)
                    self.assertEqual(done.returncode, status, done.stdout + done.stderr)
                    self.assert_partial(name, witness)
                    self.assertTrue(witness["escaped_report_fault_identity"])

    def test_exec_wrappers_keep_their_startup_path_and_eof_input(self):
        for name in MODULES:
            for status in (0, 7):
                with self.subTest(name=name, status=status):
                    root = self.root / (name + "-" + str(status))
                    root.mkdir()
                    pid = root / "native.pid"
                    program = ("import json,os,sys; print(json.dumps({'pid':os.getpid(),"
                               "'bytes':len(sys.stdin.buffer.read())})); sys.exit(" + str(status) + ")")
                    command = [sys.executable, "-B"]
                    if name == "weekly_reset":
                        command.append("-I")
                    command += [str(SMOKE / (name + ".py")), "--exec-wrapper", str(pid),
                                sys.executable, "-I", "-B", "-c", program]
                    options = {"stdin": subprocess.DEVNULL} if name == "weekly_reset" else {"input": b"ignored wrapper input"}
                    done = subprocess.run(command, cwd=root, capture_output=True, timeout=10, **options)
                    self.assertEqual(done.returncode, status, done.stderr)
                    observed = json.loads(done.stdout)
                    self.assertEqual(observed["pid"], int(pid.read_text()))
                    self.assertEqual(observed["bytes"], 0)


if __name__ == "__main__":
    unittest.main()
