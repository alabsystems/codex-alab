# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Bounded hashes or bytes from one regular, stable opened descriptor.

This checks the descriptor snapshot while reading. It does not establish custody
of a pathname or prove which image an application executed.
Reading refuses when native O_NOFOLLOW or O_NONBLOCK flags are unavailable.
"""
import hashlib
import os
import stat
import sys


def _successful_exit(error):
    # Keep this standalone classification aligned with alab/cli_status.py.
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


def _identity(metadata):
    return (metadata.st_dev, metadata.st_ino, metadata.st_mode, metadata.st_size,
            metadata.st_mtime_ns, metadata.st_ctime_ns)


def _file_snapshot(path, *, cache=None, error_type=ValueError, keep_bytes=False):
    descriptor = None
    primary = None
    label = "snapshot" if keep_bytes else "hash"
    operation = "reading" if keep_bytes else "hashing"
    try:
        try:
            flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK
        except AttributeError as error:
            raise error_type("stable file snapshots require O_NOFOLLOW and O_NONBLOCK") from error
        descriptor = os.open(path, flags)
        before = os.fstat(descriptor)
        if not stat.S_ISREG(before.st_mode):
            raise error_type(f"{label} input is not a regular file: {path}")
        identity = _identity(before)
        value = cache.get(identity) if cache is not None else None
        if value is None:
            chunks = [] if keep_bytes else None
            hasher = None if keep_bytes else hashlib.sha256()
            length = 0
            while length <= before.st_size:
                chunk = os.read(descriptor, min(1024 * 1024, before.st_size + 1 - length))
                if not chunk:
                    break
                if chunks is None:
                    hasher.update(chunk)
                else:
                    chunks.append(chunk)
                length += len(chunk)
            if length != before.st_size:
                raise error_type(f"file changed while {operation}: {path}")
            value = hasher.hexdigest() if chunks is None else b"".join(chunks)
        after = os.fstat(descriptor)
        if _identity(before) != _identity(after):
            raise error_type(f"file changed while {operation}: {path}")
    except BaseException as error:
        primary = error
        raise
    finally:
        if descriptor is not None:
            try:
                os.close(descriptor)
            except BaseException as error:
                if primary is None:
                    if issubclass(type(error), SystemExit) and _successful_exit(error):
                        raise RuntimeError(f"{label} descriptor close incomplete: requested a successful exit") from error
                    raise
                try:
                    detail = f"{type(error).__name__}: {error}"
                except BaseException:
                    detail = "unprintable close failure"
                try:
                    primary.add_note(label + " descriptor close failed: " + detail)
                except BaseException:
                    pass  # Close diagnostics cannot replace the selected failure.
    if cache is not None:
        cache[identity] = value
    return before, value


def file_sha256_snapshot(path, *, cache=None, error_type=ValueError):
    """Return same-descriptor metadata/digest, optionally reusing a private cache.

    Cache keys use the metadata fields checked around every observation. Entries
    are trusted snapshots from this reader; the cache is not independent evidence
    of current content and must remain private to its caller.
    """
    return _file_snapshot(path, cache=cache, error_type=error_type)


def file_bytes_snapshot(path, *, error_type=ValueError):
    """Return same-descriptor metadata and bytes without caching or decoding."""
    return _file_snapshot(path, error_type=error_type, keep_bytes=True)


def file_sha256(path, *, error_type=ValueError):
    return file_sha256_snapshot(path, error_type=error_type)[1]
