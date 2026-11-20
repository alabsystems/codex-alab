#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Capture named unittest results for declared feature evidence, without proof claims."""
import argparse
from contextlib import redirect_stderr, redirect_stdout
import io
import json
from pathlib import Path
import runpy
import sys
import unittest


CLI = runpy.run_path(str(Path(__file__).with_name('cli_status.py')))


class NamedResult(unittest.TextTestResult):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.rows = []
        self.active = {}

    def startTest(self, test):
        super().startTest(test)
        row = {'id': test.id(), 'status': 'running', 'finished': False}
        self.rows.append(row)
        self.active[id(test)] = row

    def stopTest(self, test):
        self.active.pop(id(test))['finished'] = True
        super().stopTest(test)

    def mark(self, test, status):
        row = self.active.get(id(test))
        if row is None:
            # Class/module fixture errors have no startTest/stopTest pair.
            self.rows.append({'id': test.id(), 'status': status, 'finished': True})
        else:
            row['status'] = status

    def addSuccess(self, test):
        if self.active[id(test)]['status'] == 'running':
            self.mark(test, 'passed')
        super().addSuccess(test)

    def addFailure(self, test, err):
        self.mark(test, 'failed')
        super().addFailure(test, err)

    def addError(self, test, err):
        self.mark(test, 'error')
        super().addError(test, err)

    def addSkip(self, test, reason):
        # A skipped subtest must also prevent its parent qualifying as passed.
        parent = getattr(test, 'test_case', test)
        self.mark(parent, 'skipped')
        super().addSkip(test, reason)

    def addExpectedFailure(self, test, err):
        self.mark(test, 'expected-failure')
        super().addExpectedFailure(test, err)

    def addUnexpectedSuccess(self, test):
        self.mark(test, 'unexpected-success')
        super().addUnexpectedSuccess(test)

    def addSubTest(self, test, subtest, err):
        if err is not None:
            self.mark(test, 'failed-subtest')
        super().addSubTest(test, subtest, err)


def passed_tests(record):
    if (not isinstance(record, dict) or type(record.get('schema')) is not int or record['schema'] != 1
            or record.get('kind') != 'python-unittest-results'
            or record.get('completed') is not True
            or type(record.get('exit_status')) is not int or record['exit_status'] != 0
            or not isinstance(record.get('output'), str)):
        raise ValueError('incomplete or unsuccessful Python test record')
    rows = record.get('tests')
    count = record.get('tests_run')
    if (type(count) is not int or count < 1 or not isinstance(rows, list)
            or len(rows) != count):
        raise ValueError('Python selection is empty or has inconsistent test counts')
    names = []
    for row in rows:
        if (not isinstance(row, dict) or row.get('status') != 'passed'
                or row.get('finished') is not True
                or not isinstance(row.get('id'), str) or not row['id'].strip()):
            raise ValueError('Python selection includes unfinished, failed or skipped tests')
        names.append(row['id'])
    if len(set(names)) != len(names):
        raise ValueError('Python selection repeats a test identity')
    return names


def execute(suite):
    output = io.StringIO()
    with redirect_stdout(output), redirect_stderr(output):
        result = unittest.TextTestRunner(
            stream=output, verbosity=2, resultclass=NamedResult).run(suite)
    record = {'schema': 1, 'kind': 'python-unittest-results', 'completed': not result.shouldStop,
              'tests_run': result.testsRun, 'tests': result.rows,
              'exit_status': 0 if result.wasSuccessful() else 1, 'output': output.getvalue()}
    try:
        passed_tests(record)
    except ValueError:
        record['exit_status'] = 1
    return record


def test_ids(suite):
    for child in suite:
        if isinstance(child, unittest.TestSuite):
            yield from test_ids(child)
        else:
            yield child.id()


def discover(directory, pattern, names):
    loader = unittest.TestLoader()
    if names:
        if (len(names) != len(set(names))
                or any('.' not in name or not all(part.isidentifier() for part in name.split('.'))
                       for name in names)):
            raise ValueError('expected distinct, fully qualified unittest names')
        loader.testNamePatterns = names
    suite = loader.discover(str(directory.resolve()), pattern)
    if names and sorted(test_ids(suite)) != sorted(names):
        raise ValueError('discovered tests differ from the exact requested names')
    return suite


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--start-directory', type=Path, required=True)
    parser.add_argument('--pattern', default='test*.py')
    parser.add_argument('--record', type=Path, required=True)
    parser.add_argument('--tests', nargs='+', help='exact test identities from the selected fixture')
    args = parser.parse_args()
    try:
        # Refuse existing output before discovery or test side effects begin.
        with args.record.open('x') as stream:
            suite = discover(args.start_directory, args.pattern, args.tests)
            record = execute(suite)
            if args.tests and sorted(row['id'] for row in record['tests']) != sorted(args.tests):
                record['exit_status'] = 1
                record['output'] += '\nExecuted tests differ from the exact requested names.\n'
            json.dump(record, stream, indent=2)
            stream.write('\n')
        sys.stdout.write(record['output'])
        return record['exit_status']
    except SystemExit as error:
        if not CLI['successful_exit'](error):
            raise
        return CLI['report_failure']('Python test results: ', error, incomplete=True)
    except (OSError, ImportError, ValueError) as error:
        return CLI['report_failure']('Python test results: ', error)


if __name__ == '__main__':
    sys.exit(main())
