# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Read file-qualified definitions, properties and enum values from experimental schema exports."""
from functools import lru_cache
import json
from pathlib import Path
import shutil
import subprocess


EXPORTS = "codex-rs/app-server-protocol/schema/precomputed/app-server-exports-experimental.json.zst"


def decompress(data):
    try:
        from compression import zstd
    except ImportError:
        if shutil.which("zstd"):
            command = ["zstd", "-d", "-c"]
        elif shutil.which("python3.14"):
            command = ["python3.14", "-I", "-c",
                       "from compression import zstd; import sys; "
                       "sys.stdout.buffer.write(zstd.decompress(sys.stdin.buffer.read()))"]
        else:
            return None
        result = subprocess.run(command, input=data, capture_output=True)
        return result.stdout if result.returncode == 0 else None
    try:
        return zstd.decompress(data)
    except zstd.ZstdError:
        return None


@lru_cache(maxsize=2)
def experimental_schemas(repo, revision):
    """The same json_schema map pristine generate-json-schema --experimental exports."""
    result = subprocess.run(["git", "-C", repo, "show", f"{revision}:{EXPORTS}"], capture_output=True)
    if result.returncode:
        return {}
    data = decompress(result.stdout)
    exports = json.loads(data) if data else None
    schemas = exports.get("json_schema") if isinstance(exports, dict) else None
    return schemas if isinstance(schemas, dict) else {}


def collect_items(schemas):
    """Use the same structural traversal for the binary's files and the committed export."""
    out = set()

    def walk(rel, node, where):
        if isinstance(node, dict):
            for key, inner in node.items():
                if key in ("definitions", "$defs", "properties") and isinstance(inner, dict):
                    for name, sub in inner.items():
                        out.add("%s %s" % (rel, ".".join(where + [name])))
                        # The combined bundle groups definitions under a v2 namespace.
                        if (rel == "codex_app_server_protocol.schemas.json" and key == "definitions"
                                and not where and name == "v2" and isinstance(sub, dict)):
                            for owner, schema in sub.items():
                                out.add(f"{rel} v2.{owner}")
                                walk(rel, schema, [name, owner])
                        else:
                            walk(rel, sub, where + [name])
                elif key == "enum" and isinstance(inner, list):
                    for value in inner:
                        if isinstance(value, str):
                            out.add("%s %s=%s" % (rel, ".".join(where), value))
                elif isinstance(inner, (dict, list)):
                    walk(rel, inner, where)
        elif isinstance(node, list):
            for inner in node:
                walk(rel, inner, where)

    for file, schema in schemas:
        if not isinstance(schema, dict) or not schema:
            raise ValueError(f"invalid JSON schema in {file}")
        walk(file, schema, [])
    return out


def schema_items(directory):
    root = Path(directory)
    return collect_items((file.relative_to(root).as_posix(), json.loads(file.read_bytes()))
                         for file in root.rglob("*.json"))


def archive_items(repo, revision):
    if not revision:
        raise ValueError("missing experimental export revision")
    schemas = experimental_schemas(repo, revision)
    if not schemas or "codex_app_server_protocol.schemas.json" not in schemas:
        raise ValueError(f"missing experimental export at {revision}")
    try:
        items = collect_items((file, json.loads(data)) for file, data in schemas.items())
    except (TypeError, ValueError) as error:
        raise ValueError(f"invalid experimental export at {revision}: {error}") from error
    if not items:
        raise ValueError(f"empty experimental export at {revision}")
    return items
