# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Bounded, metadata-only log-row timeline; never read formatted log bodies."""
import argparse
import json
from pathlib import Path
import re
import sqlite3
import stat
import sys
import time
import uuid

sys.path.insert(0, str(Path(__file__).resolve().parent))
from cli_status import run_operation


TARGETS = (
    "codex_api::endpoint::alab_request_diagnostics",
    "codex_http_client::response::alab_error_body_capture",
    "codex_core::responses_retry::alab_capacity_retry",
    "codex_core::client::alab_transport",
    "codex_core::responses_retry",
    "codex_api::endpoint::alab_request_diagnostics::http_operations",
)
LEVELS = ("TRACE", "DEBUG", "INFO", "WARN", "ERROR")
MAX_LIMIT = 1000
VM_STEPS = 200_000
QUERY_SECONDS = 0.25
LOCK_SECONDS = 0.05
_COLUMNS = (
    ("id", "INTEGER", 0, 1), ("ts", "INTEGER", 1, 0),
    ("ts_nanos", "INTEGER", 1, 0), ("level", "TEXT", 1, 0),
    ("target", "TEXT", 1, 0), ("feedback_log_body", "TEXT", 0, 0),
    ("module_path", "TEXT", 0, 0), ("file", "TEXT", 0, 0),
    ("line", "INTEGER", 0, 0), ("thread_id", "TEXT", 0, 0),
    ("process_uuid", "TEXT", 0, 0), ("estimated_bytes", "INTEGER", 1, 0),
)
_READ_COLUMNS = {"id", "ts", "ts_nanos", "level", "target",
                 "process_uuid", "thread_id"}


class TimelineError(Exception):
    """Fixed diagnostics: SQLite exception text can contain private schema data."""
    def __init__(self, category, sqlite_code=None):
        self.category = category
        self.sqlite_code = sqlite_code
        super().__init__(category if sqlite_code is None else
                         f"{category} (sqlite_code={sqlite_code})")


def _canonical_uuid(value):
    if not isinstance(value, str) or len(value) != 36:
        return None
    try:
        return value if str(uuid.UUID(value)) == value else None
    except ValueError:
        return None


def _process(value):
    if not isinstance(value, str) or len(value) > 51:
        return None
    match = re.fullmatch(r"pid:([1-9][0-9]{0,9}):(.{36})", value)
    if not match or int(match[1]) > 0xffffffff or not _canonical_uuid(match[2]):
        return None
    return value


def _validate_scope(pid, process_uuid, thread_id, limit, since):
    if pid is None and process_uuid is None and thread_id is None:
        raise TimelineError("scope_required")
    if pid is not None and (type(pid) is not int or not 1 <= pid <= 0xffffffff):
        raise TimelineError("invalid_scope")
    if process_uuid is not None and not _process(process_uuid):
        raise TimelineError("invalid_scope")
    if thread_id is not None and not _canonical_uuid(thread_id):
        raise TimelineError("invalid_scope")
    if type(limit) is not int or not 1 <= limit <= MAX_LIMIT:
        raise TimelineError("invalid_limit")
    if since is not None and (type(since) is not int or not 0 <= since <= 2**63 - 1):
        raise TimelineError("invalid_since")


def _schema(connection):
    if connection.execute("PRAGMA encoding").fetchone() != ("UTF-8",):
        raise TimelineError("unsupported_database_encoding")
    tables = connection.execute("PRAGMA table_list('logs')").fetchmany(2)
    if (len(tables) != 1 or tables[0][0:3] != ("main", "logs", "table")
            or tables[0][3:] != (12, 0, 0)):
        raise TimelineError("unsupported_schema")
    columns = connection.execute("PRAGMA table_xinfo('logs')").fetchmany(13)
    if len(columns) != 12 or any(
        (row[1], row[2], row[3], row[5]) != expected or row[6] != 0
        for row, expected in zip(columns, _COLUMNS)
    ):
        raise TimelineError("unsupported_schema")


def _authorize(action, table, column, database, _source):
    if action == sqlite3.SQLITE_READ:
        if database != "main" or table != "logs" or column not in _READ_COLUMNS:
            return sqlite3.SQLITE_DENY
    elif action not in (sqlite3.SQLITE_SELECT, sqlite3.SQLITE_FUNCTION):
        return sqlite3.SQLITE_DENY
    return sqlite3.SQLITE_OK


def _sqlite_failure(error):
    code = getattr(error, "sqlite_errorcode", None)
    code = code if type(code) is int else None
    primary = None if code is None else code & 0xff
    category = {
        sqlite3.SQLITE_BUSY: "database_busy",
        sqlite3.SQLITE_LOCKED: "database_busy",
        sqlite3.SQLITE_INTERRUPT: "query_budget_exceeded",
        sqlite3.SQLITE_NOTADB: "invalid_database",
        sqlite3.SQLITE_CORRUPT: "invalid_database",
        sqlite3.SQLITE_TOOBIG: "metadata_too_large",
    }.get(primary, "database_error")
    return TimelineError(category, code)


def read_timeline(db, *, pid=None, process_uuid=None, thread_id=None,
                  limit=100, since=None, target=None):
    """Return latest selected rows in row-ID order, without attempt pairing."""
    _validate_scope(pid, process_uuid, thread_id, limit, since)
    if target is not None and (type(target) is not str or target not in TARGETS):
        raise TimelineError("invalid_target")
    targets = TARGETS if target is None else (target,)
    try:
        path = Path(db).resolve(strict=True)
        before = path.stat()
        if not stat.S_ISREG(before.st_mode):
            raise TimelineError("database_not_regular")
    except (OSError, RuntimeError):
        raise TimelineError("database_unavailable") from None
    deadline = time.monotonic() + QUERY_SECONDS
    steps_left = VM_STEPS

    def progress():
        nonlocal steps_left
        steps_left -= 100
        return int(steps_left < 0 or time.monotonic() >= deadline)

    connection = None
    primary_failed = True
    try:
        connection = sqlite3.connect(path.as_uri() + "?mode=ro", uri=True,
                                     timeout=LOCK_SECONDS)
        connection.set_progress_handler(progress, 100)
        connection.setlimit(sqlite3.SQLITE_LIMIT_LENGTH, 65536)
        connection.setlimit(sqlite3.SQLITE_LIMIT_SQL_LENGTH, 16384)
        connection.execute("PRAGMA query_only=ON")
        connection.execute("PRAGMA trusted_schema=OFF")
        connection.execute("PRAGMA cache_size=-1024")
        connection.execute("BEGIN")
        _schema(connection)
        connection.set_authorizer(_authorize)
        target_placeholders = ",".join("?" for _ in targets)
        clauses = [f"target COLLATE BINARY IN ({target_placeholders})",
                   "level COLLATE BINARY IN (?,?,?,?,?)"]
        arguments = [*targets, *LEVELS]
        if pid is not None:
            clauses.append("process_uuid LIKE ?")
            arguments.append(f"pid:{pid}:%")
        if process_uuid is not None:
            clauses.append("process_uuid COLLATE BINARY = ?")
            arguments.append(process_uuid)
        if thread_id is not None:
            clauses.append("thread_id COLLATE BINARY = ?")
            arguments.append(thread_id)
        if since is not None:
            clauses.append("typeof(ts)='integer' AND ts >= ?")
            arguments.append(since)
        # Byte substrings bound returned identities even for hostile SQLite types.
        # Neither the projection nor authorizer grants access to formatted bodies.
        query = ("SELECT CASE WHEN typeof(id)='integer' THEN id END, "
                 "CASE WHEN typeof(ts)='integer' THEN ts END, "
                 "CASE WHEN typeof(ts_nanos)='integer' THEN ts_nanos END, level, target, "
                 "CASE WHEN typeof(process_uuid)='text' THEN "
                 "substr(CAST(process_uuid AS BLOB),1,52) END, "
                 "CASE WHEN typeof(thread_id)='text' THEN "
                 "substr(CAST(thread_id AS BLOB),1,37) END FROM logs WHERE "
                 + " AND ".join(clauses) + " ORDER BY id DESC LIMIT ?")
        raw_rows = connection.execute(query, [*arguments, limit + 1]).fetchall()
        if time.monotonic() >= deadline:
            raise TimelineError("query_budget_exceeded")
        after = path.stat()
        if (before.st_dev, before.st_ino) != (after.st_dev, after.st_ino):
            raise TimelineError("database_changed")
    except sqlite3.Error as error:
        raise _sqlite_failure(error) from None
    except OSError:
        raise TimelineError("database_unavailable") from None
    else:
        primary_failed = False
    finally:
        if connection is not None:
            try:
                connection.close()
            except BaseException:
                # Cleanup cannot replace an available primary error or turn a
                # successful query into successful-exit cancellation.
                if not primary_failed:
                    raise TimelineError("database_close_failed") from None

    rows = []
    previous = {}
    invalid_scope_rows = 0
    for identity, seconds, nanos, level, row_target, process, thread in reversed(raw_rows[:limit]):
        if (type(identity) is not int or not 1 <= identity <= 2**63 - 1
                or type(level) is not str or level not in LEVELS
                or type(row_target) is not str or row_target not in targets):
            raise TimelineError("invalid_metadata")
        process = _process(_ascii(process))
        thread = _canonical_uuid(_ascii(thread))
        if ((pid is not None and (process is None or int(process.split(":")[1]) != pid))
                or (process_uuid is not None and process != process_uuid)
                or (thread_id is not None and thread != thread_id)):
            invalid_scope_rows += 1
            continue
        timestamp = (seconds * 1_000_000_000 + nanos if
                     type(seconds) is int and 0 <= seconds <= 2**63 - 1
                     and type(nanos) is int and 0 <= nanos < 1_000_000_000 else None)
        key = (process, thread, row_target, level) if process and thread else None
        gap = None
        gap_status = "identity_unavailable" if key is None else "first_selected_row"
        if timestamp is None:
            gap_status = "timestamp_unavailable"
            if key is not None:
                previous.pop(key, None)
        elif key is not None:
            earlier = previous.get(key)
            if earlier is not None:
                if timestamp < earlier:
                    gap_status = "backward_timestamp"
                else:
                    gap_status, gap = "measured", timestamp - earlier
            previous[key] = timestamp
        rows.append({"id": identity, "ts": seconds if timestamp is not None else None,
                     "ts_nanos": nanos if timestamp is not None else None,
                     "level": level, "target": row_target, "process_uuid": process,
                     "thread_id": thread, "selected_row_gap_ns": gap,
                     "row_gap_status": gap_status})
    scope = {"pid": pid, "process_uuid": process_uuid, "thread_id": thread_id, "since_epoch_seconds": since}
    if target is not None:
        scope["target"] = target
    return {"schema": "logs_2/feedback_log_body", "rows": rows,
            "limit": limit, "more_rows_available": len(raw_rows) > limit,
            "omitted_invalid_scope_rows": invalid_scope_rows,
            "scope": scope,
            "gap_meaning": "Stored wall-clock gaps between selected rows with the same validated process, thread, target and level; not retry, deadline or outage durations."}


def _ascii(value):
    try:
        return value.decode("ascii") if isinstance(value, bytes) else None
    except UnicodeDecodeError:
        return None


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--db", required=True, help="existing logs_2 SQLite file")
    parser.add_argument("--pid", type=int)
    parser.add_argument("--process-uuid", help="exact canonical pid:N:UUID identity")
    parser.add_argument("--thread-id", help="canonical thread UUID")
    parser.add_argument("--target", help="select one exact diagnostic target: " + ", ".join(TARGETS))
    parser.add_argument("--limit", type=int, default=100)
    parser.add_argument("--since", type=int, help="inclusive epoch seconds")
    arguments = parser.parse_args(argv)

    def operation():
        try:
            result = read_timeline(arguments.db, pid=arguments.pid,
                                   process_uuid=arguments.process_uuid,
                                   thread_id=arguments.thread_id, limit=arguments.limit,
                                   since=arguments.since, target=arguments.target)
            print(json.dumps(result, sort_keys=True, separators=(",", ":")))
            return 0
        except TimelineError:
            raise
        except Exception:
            raise TimelineError("operation_failed") from None

    return run_operation(operation, "request timeline: ")


if __name__ == "__main__":
    sys.exit(main())
