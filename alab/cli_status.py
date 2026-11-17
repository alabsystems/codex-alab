# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""CLI exit classification and failure diagnostics, independent of execution tools."""
import os
import sys


def successful_exit(error):
    """Recognize success only for exact SystemExit with a native integer code."""
    if type(error) is not SystemExit:
        raise RuntimeError("unsupported SystemExit type") from None
    code = error.code
    if code is None:
        return True
    if not issubclass(type(code), int):
        return False
    code = int.__index__(code)
    if os.name == "posix" and -sys.maxsize - 1 <= code <= sys.maxsize:
        return code & 0xff == 0
    return code == 0


def _failure_notes(error):
    try:
        notes = getattr(error, "__notes__", ())
        if isinstance(notes, (list, tuple)):
            return tuple(note for note in notes if isinstance(note, str))
    except Exception:
        pass  # Advisory notes must not replace an available primary failure.
    return ()


def failure_detail(error):
    return "\n".join((str(error), *_failure_notes(error)))


def report_failure(prefix, error, *, incomplete=False, stdout_line=None):
    """Keep a failed CLI operation nonzero even when diagnostics cannot finish."""
    try:
        if stdout_line is not None:
            print(stdout_line)
        message = ("operation incomplete: requested a successful exit"
                   if incomplete else str(error))
        print(prefix + message, file=sys.stderr)
        for note in _failure_notes(error):
            print(note, file=sys.stderr)
    except SystemExit as report_error:
        if not successful_exit(report_error):
            raise
    except Exception:
        pass
    return 1


def run_operation(operation, prefix):
    """Guard an already-parsed CLI operation without changing API exceptions."""
    try:
        return operation()
    except SystemExit as error:
        if not successful_exit(error):
            raise
        return report_failure(prefix, error, incomplete=True)
    except Exception as error:
        return report_failure(prefix, error)
