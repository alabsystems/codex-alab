# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Closed exit classifiers never dispatch foreign exception or payload callbacks."""
import ast
import copy
import os
from pathlib import Path
import runpy
import sys
from types import SimpleNamespace
import unittest


ALAB = Path(__file__).resolve().parents[1]
CLASSIFIERS = (('cli_status.py', 'successful_exit'),
               ('file_hash.py', '_successful_exit'),
               ('smoke/capacity_exec.py', '_successful_exit'),
               ('smoke/history_pagination.py', 'successful_primary_exit'))


def closed_classifier(relative, name):
    """Execute the actual function AST with only its documented boot dependencies."""
    source = ALAB / relative
    tree = ast.parse(source.read_bytes(), filename=str(source))
    functions = [node for node in ast.walk(tree)
                 if type(node) is ast.FunctionDef and node.name == name]
    assert len(functions) == 1
    function = copy.deepcopy(functions[0])
    namespace = {'os': os, 'sys': sys}
    exec(compile(ast.Module(body=[function], type_ignores=[]), str(source), 'exec'), namespace)
    if name != 'successful_primary_exit':
        return namespace[name]

    def classify(error):
        namespace['primary'] = error
        return namespace[name]()
    return classify


class CliStatusCallbackTests(unittest.TestCase):
    def test_all_closed_copies_preserve_plain_exit_and_integer_controls(self):
        for relative, name in CLASSIFIERS:
            classify = closed_classifier(relative, name)
            for code, expected in ((None, True), (False, True), (0, True), (256, os.name == 'posix'),
                                   (-256, os.name == 'posix'), (47, False), (0.0, False),
                                   (sys.maxsize + 1, False)):
                with self.subTest(source=relative, code=code):
                    self.assertIs(classify(SystemExit(code)), expected)

    def test_all_closed_copies_refuse_even_transparent_exit_subclasses_before_callbacks(self):
        callbacks = []

        class Transparent(SystemExit):
            pass

        class Hostile(SystemExit):
            def __getattribute__(self, name):
                callbacks.append(name)
                raise SystemExit(0)

            def __str__(self):
                callbacks.append('str')
                raise SystemExit(0)

        for relative, name in CLASSIFIERS:
            classify = closed_classifier(relative, name)
            for error in (Transparent(47), Hostile(47)):
                with self.subTest(source=relative, subtype=type(error).__name__):
                    try:
                        classify(error)
                    except BaseException as observed:
                        self.assertIs(type(observed), RuntimeError)
                        self.assertEqual(str(observed), 'unsupported SystemExit type')
                        self.assertTrue(observed.__suppress_context__)
                        self.assertIsNone(observed.__cause__)
                    else:
                        self.fail('unsupported SystemExit subclass was accepted')
                    self.assertEqual(callbacks, [])

    def test_payload_class_and_integer_operators_are_not_callback_boundaries(self):
        callbacks = []

        class Payload:
            def __getattribute__(self, name):
                callbacks.append(name)
                raise SystemExit(0)

        class Integer(int):
            def __getattribute__(self, name):
                callbacks.append(name)
                raise SystemExit(0)

            def __index__(self):
                callbacks.append('index')
                raise SystemExit(0)

            def __eq__(self, other):
                callbacks.append('eq')
                raise SystemExit(0)

            def __and__(self, other):
                callbacks.append('and')
                raise SystemExit(0)

        for relative, name in CLASSIFIERS:
            classify = closed_classifier(relative, name)
            for code, expected in ((Payload(), False), (Integer(256), os.name == 'posix'),
                                   (Integer(47), False)):
                with self.subTest(source=relative, payload=type(code).__name__):
                    self.assertIs(classify(SystemExit(code)), expected)
                    self.assertEqual(callbacks, [])

    def test_shared_run_operation_preserves_plain_failure_object_identity(self):
        policy = runpy.run_path(str(ALAB / 'cli_status.py'))
        for error in (SystemExit(47), KeyboardInterrupt()):
            with self.subTest(error=type(error).__name__):
                def operation():
                    raise error
                try:
                    policy['run_operation'](operation, 'owned: ')
                except BaseException as observed:
                    self.assertIs(observed, error)
                else:
                    self.fail('interrupted operation returned')

    def test_output_close_selector_reaches_closed_classifier_without_class_callbacks(self):
        original_path = sys.path[:]
        try:
            sys.path.insert(0, str(ALAB / 'smoke'))
            policy = runpy.run_path(str(ALAB / 'smoke/capacity_exec.py'))
        finally:
            sys.path[:] = original_path
        callbacks = []

        class ForeignExit(SystemExit):
            def __getattribute__(self, name):
                if name in ('__class__', 'code'):
                    callbacks.append(name)
                    raise SystemExit(0)
                return super().__getattribute__(name)

        primary = ForeignExit(47)

        def close():
            raise primary

        outputs = policy['OutputFiles']((('owned', Path('never-opened')),))
        outputs.streams = [SimpleNamespace(close=close)]
        try:
            outputs._close(None)
        except BaseException as observed:
            self.assertIs(type(observed), RuntimeError)
            self.assertEqual(str(observed), 'unsupported SystemExit type')
        else:
            self.fail('failed output close returned')
        self.assertEqual(callbacks, [])


if __name__ == '__main__':
    unittest.main()
