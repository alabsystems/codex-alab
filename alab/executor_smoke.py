#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Check the packaged standalone executor's protocol identity without a provider call."""

import argparse
import asyncio
import json
from pathlib import Path


async def check(package, home, client_version):
    package, home = Path(package).resolve(strict=True), Path(home).resolve(strict=True)
    metadata = json.loads((package / "codex-package.json").read_text())
    if metadata.get("execServer") != "bin/codex-exec-server":
        raise ValueError("package metadata does not name its standalone executor")
    environment = {"PATH": "/usr/bin:/bin", "HOME": str(home), "TMPDIR": str(home),
                   "CODEX_HOME": str(home / "codex"), "USER": "alab-smoke",
                   "LOGNAME": "alab-smoke"}
    for name, directory in (("XDG_CONFIG_HOME", "config"), ("XDG_CACHE_HOME", "cache"),
                            ("XDG_DATA_HOME", "data"), ("XDG_STATE_HOME", "state"),
                            ("XDG_RUNTIME_DIR", "runtime")):
        environment[name] = str(home / directory)
    stderr_path = home / "executor-smoke.stderr"
    with stderr_path.open("wb") as stderr:
        child = await asyncio.create_subprocess_exec(
            str(package / "bin/codex-exec-server"), "--listen", "stdio",
            cwd=home, env=environment, stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE, stderr=stderr)
        try:
            # One deadline detects a stuck child, including shutdown after stdin EOF.
            async with asyncio.timeout(60):
                async def request(message, expected_id):
                    child.stdin.write(json.dumps(message).encode() + b"\n")
                    await child.stdin.drain()
                    reply = json.loads(await child.stdout.readline())
                    if reply.get("id") != expected_id or "error" in reply:
                        raise ValueError(f"executor rejected request {expected_id}: {reply}")
                    return reply["result"]

                initialized = await request(
                    {"id": 1, "method": "initialize", "params": {"clientName": "alab-package-smoke"}}, 1)
                if not initialized.get("sessionId"):
                    raise ValueError("executor initialization returned no session identity")
                reported = initialized["environmentInfo"]
                if reported.get("executorVersion") != client_version:
                    raise ValueError(f"executor protocol version {reported.get('executorVersion')!r}, "
                                     f"expected client {client_version!r}")
                child.stdin.write(b'{"method":"initialized","params":{}}\n')
                refreshed = await request({"id": 2, "method": "environment/info", "params": {}}, 2)
                if refreshed.get("executorVersion") != client_version:
                    raise ValueError("executor identity changed after initialization")
                child.stdin.close()
                await child.stdin.wait_closed()
                if await child.wait() != 0:
                    raise ValueError("executor failed after its stdio smoke")
        except TimeoutError as error:
            raise RuntimeError("standalone executor exceeded its one-minute hang detector") from error
        except Exception as error:
            raise RuntimeError(f"standalone executor smoke: {error}") from error
        finally:
            if child.returncode is None:
                try:
                    child.kill()
                except ProcessLookupError:
                    pass
                await child.wait()
            child.stdin.close()
    print(f"smoke: bin/codex-exec-server protocol client version: {client_version}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--home", type=Path, required=True)
    parser.add_argument("--client-version", required=True)
    args = parser.parse_args()
    try:
        asyncio.run(check(args.package, args.home, args.client_version))
    except (OSError, ValueError, RuntimeError, KeyError, TypeError) as error:
        diagnostics = args.home / "executor-smoke.stderr"
        if diagnostics.is_file():
            print(diagnostics.read_text(errors="replace")[-16384:])
        parser.exit(1, f"{error}\n")


if __name__ == "__main__":
    main()
