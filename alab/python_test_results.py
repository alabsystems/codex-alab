#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Capture named unittest results for declared feature evidence, without proof claims."""
import argparse
from contextlib import redirect_stderr, redirect_stdout
import io
import json
from pathlib import Path
import sys
import unittest


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
    record = {'schema': 1, 'kind': 'python-unittest-results', 'completed': True,
              'tests_run': result.testsRun, 'tests': result.rows,
              'exit_status': 0 if result.wasSuccessful() else 1, 'output': output.getvalue()}
    try:
        passed_tests(record)
    except ValueError:
        record['exit_status'] = 1
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--start-directory', type=Path, required=True)
    parser.add_argument('--pattern', default='test*.py')
    parser.add_argument('--record', type=Path, required=True)
    args = parser.parse_args()
    # Refuse existing output before discovery or test side effects begin.
    with args.record.open('x') as stream:
        suite = unittest.TestLoader().discover(str(args.start_directory.resolve()), args.pattern)
        record = execute(suite)
        json.dump(record, stream, indent=2)
        stream.write('\n')
    sys.stdout.write(record['output'])
    return record['exit_status']


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (OSError, ImportError) as error:
        print('Python test results: ' + str(error), file=sys.stderr)
        sys.exit(1)
