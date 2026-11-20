# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""The selected worker's embedded owner, outside the protected compiler group."""

import os
import select
import signal
import subprocess
import sys
import time


def pending(fd):
    watched = select.poll()
    watched.register(fd, select.POLLIN)
    return bool(watched.poll(0))


def supervise(command, environment, fd, guard, cooperative):
    if pending(fd):
        return 128 + signal.SIGTERM
    # The parent supplies only its opt-in inherited job descriptors. The worker
    # cancellation endpoint and the keeper's authenticated socket are CLOEXEC.
    child = subprocess.Popen(command, start_new_session=True, close_fds=False, env=environment)
    try:
        guard["observe_lifeline"](child.pid, fd)
    finally:
        # An exiting leader may still have writers. Hold its original wait until
        # the signal reached its exact private group. An admitted native front
        # owns its compiler (including separate groups) and settles cooperatively.
        # Ordinary identity queries and target probes own no compiler authority.
        try:
            if cooperative:
                os.killpg(child.pid, signal.SIGTERM)
                os.killpg(child.pid, signal.SIGCONT)
            else:
                os.killpg(child.pid, signal.SIGKILL)
                # A concurrent spawn may not have joined at the first signal.
                # The original unreaped leader still pins the group number.
                guard["observe_exit"](child.pid)
                os.killpg(child.pid, signal.SIGKILL)
        except (ProcessLookupError, PermissionError):
            # XNU can report EPERM for a group with only a zombie. The owning
            # wait and the following kernel absence check still have to pass.
            pass
        code = child.wait()
        while guard["group_exists"](child.pid):
            time.sleep(0.05)
    return 128 - code if code < 0 else code


def main(arguments):
    guard_source, memory_source, descriptor, mode, *command = arguments
    if mode not in ("compiler", "disposable") or memory_source and mode != "compiler":
        raise ValueError("invalid native worker command mode")
    fd = int(descriptor)
    os.set_inheritable(fd, False)
    # A launcher's ignored SIGCHLD can discard the owning wait on some systems.
    signal.signal(signal.SIGCHLD, signal.SIG_DFL)
    signals = {signal.SIGINT, signal.SIGTERM, signal.SIGHUP, signal.SIGQUIT}
    # The caller owns this helper and signals its lifeline. Its actual front
    # must be able to receive TERM even after a background/nohup launch.
    for signum in signals:
        signal.signal(signum, signal.SIG_DFL)
    signal.pthread_sigmask(signal.SIG_UNBLOCK, signals)
    guard = {"__name__": "native_command_guard"}
    exec(guard_source, guard)
    if pending(fd):
        return 128 + signal.SIGTERM

    def run(argv, env):
        return supervise(argv, env, fd, guard, mode == "compiler")

    if memory_source:
        memory = {"__name__": "native_trust_memory"}
        exec(memory_source, memory)
        return memory["run"](command, supervise=run)
    return run(command, os.environ)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
