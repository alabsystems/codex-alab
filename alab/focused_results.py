#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Reject empty or incomplete focused libtest selections; not a qualification gate.

The caller supplies the actual command status and combined stdout/stderr. This checks
completion counts only, not source provenance, scenario semantics or entitlements.
Use unit_results.py for the full gate's artifact-based library comparisons.
"""
import argparse
import json
from pathlib import Path
import re
import sys


START = re.compile(r'^running ([0-9]+) tests?$', re.MULTILINE)
RESULT = re.compile(
    r'^test result: (ok|FAILED)\. ([0-9]+) passed; ([0-9]+) failed; '
    r'([0-9]+) ignored; ([0-9]+) measured; ([0-9]+) filtered out;', re.MULTILINE)
ANSI = re.compile(r'\x1b\[[0-?]*[ -/]*[@-~]')
PASSED = re.compile(r'^test (\S+) \.\.\. ok$', re.MULTILINE)


def libtest_output(raw):
    output = raw.decode('utf-8')
    # A cursor command can end a TUI frame immediately before libtest's line.
    # Preserve that boundary without treating ordinary visible text as a record.
    return ANSI.sub(lambda control: '\n' if output.startswith('test ', control.end()) else '', output)


def completed_names(output, suites):
    passed = []
    for start, result, suite in zip(START.finditer(output), RESULT.finditer(output), suites):
        names = PASSED.findall(output[start.end():result.start()])
        if len(names) != suite['passed']:
            raise ValueError('executed test names disagree with suite counts')
        passed.extend(names)
    return passed


def confirm(output, status, minimum_suites, *, required_filters=(), required_tests=()):
    if status != 0:
        raise ValueError(f'test command failed with status {status}')
    if minimum_suites < 1:
        raise ValueError('minimum suites must be positive')
    if any(marker in output.casefold() for marker in (
            'skipping test', 'skipping cloud-managed subprocess')):
        raise ValueError('test output reports a skipped scenario')
    starts = list(START.finditer(output))
    results = list(RESULT.finditer(output))
    if len(starts) < minimum_suites or len(results) != len(starts):
        raise ValueError('missing selected suites or completion summaries')
    suites = []
    previous_end = -1
    for start, result in zip(starts, results):
        if not previous_end < start.start() < result.start():
            raise ValueError('overlapping or out-of-order suite output')
        count = int(start[1])
        passed, failed, ignored, measured, filtered = map(int, result.groups()[1:])
        if (count == 0 or result[1] != 'ok' or failed or ignored or measured
                or count != passed):
            raise ValueError('selection is empty, failed, ignored or incomplete')
        suites.append({'passed': passed, 'failed': failed, 'ignored': ignored,
                       'measured': measured, 'filtered_out': filtered})
        previous_end = result.end()
    if required_filters or required_tests:
        names = completed_names(output, suites)
        if len(names) != len(set(names)):
            raise ValueError('selected suites repeat a test identity')
        for selector in required_filters:
            if not selector:
                raise ValueError('required filters must be nonempty')
            if not any(selector in name for name in names):
                raise ValueError('required filter matched no passed test: ' + selector)
        for name in required_tests:
            if not name or names.count(name) != 1:
                raise ValueError('required exact test did not pass once: ' + name)
    return suites


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--log', type=Path, required=True)
    parser.add_argument('--status', type=int, required=True,
                        help='actual test command exit status, including pipeline failures')
    parser.add_argument('--minimum-suites', type=int, default=1)
    parser.add_argument('--require-filter', action='append', default=[],
                        help='require a passed name containing this libtest filter; repeatable')
    parser.add_argument('--require-test', action='append', default=[],
                        help='require this exact emitted test name once; repeatable')
    args = parser.parse_args()
    try:
        suites = confirm(libtest_output(args.log.read_bytes()), args.status, args.minimum_suites,
                         required_filters=args.require_filter, required_tests=args.require_test)
    except (OSError, UnicodeError, ValueError) as error:
        print(f'focused results: {error}', file=sys.stderr)
        return 1
    print(json.dumps({'schema': 1, 'suites': suites,
                      'scope': 'nonempty completed selections only; qualification is separate'}))
    return 0


if __name__ == '__main__':
    sys.exit(main())
