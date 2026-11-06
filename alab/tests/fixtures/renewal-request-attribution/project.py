#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Project actual retained HTTP bodies onto request-classification fields only."""
import hashlib
import json
from pathlib import Path
import sys

PREFIX = "Generate a concise, single-line task title"


def project(body):
    result = {"model": body["model"], "tools": [
        {key: tool[key] for key in ("type", "name") if key in tool}
        for tool in body["tools"]], "input": []}
    for item in body["input"]:
        if item.get("role") == "user":
            result["input"].append({"role": "user", "content": [
                {"type": part["type"], "text": part["text"][:len(PREFIX)]}
                for part in item.get("content", []) if part.get("type") == "input_text"
                and not part["text"].startswith("<environment_context>")]})
    result["input"] = [item for item in result["input"] if item["content"]]
    if "text" in body:
        result["text"] = body["text"]
    return result


if __name__ == "__main__":
    source = Path(sys.argv[1])
    packets = json.loads(source.read_text())
    root = Path(__file__).resolve().parent
    indexes = {"foreground": 0, "thread-title": 1}
    for name, index in indexes.items():
        (root / (name + ".json")).write_text(json.dumps(project(packets[index]["body"]), indent=2) + "\n")
    (root / "provenance.json").write_text(json.dumps({
        "kind": "actual-packet-classification-projection",
        "source": source.parent.name + "/" + source.name,
        "sha256": hashlib.sha256(source.read_bytes()).hexdigest(), "indexes": indexes,
        "projection": "model; tool type/name; user input_text prefixes limited to title classifier prefix length; text output format. Developer/environment/tool-parameter prose omitted."
    }, indent=2) + "\n")
