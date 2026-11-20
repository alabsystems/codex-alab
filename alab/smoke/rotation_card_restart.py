# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Observe the destination address card losing its process claim in a private Darwin host."""

from contextlib import closing
import ctypes
import select
import struct

from capacity_exec import require
import renewal_follow as fixture
from rotation_account_card import CLAIM_WINDOW_SECONDS


def live_birth(pid):
    # Darwin's sys/proc_info.h, struct proc_bsdinfo. The fourth word is pbi_pid,
    # and the last two fields are the process start in seconds and microseconds.
    layout = struct.Struct('=12I16s32s6IQQ')
    info = ctypes.create_string_buffer(layout.size)
    proc_pidinfo = ctypes.CDLL('/usr/lib/libproc.dylib').proc_pidinfo
    proc_pidinfo.argtypes = (ctypes.c_int, ctypes.c_int, ctypes.c_uint64, ctypes.c_void_p, ctypes.c_int)
    proc_pidinfo.restype = ctypes.c_int
    if proc_pidinfo(pid, 3, 0, info, layout.size) == layout.size:  # PROC_PIDTBSDINFO
        fields = layout.unpack(info.raw)
        if fields[3] == pid and fields[-1] < 1_000_000:
            return fields[-2] * 1_000_000 + fields[-1]
    return None


def require_destination(codex_home, pid, birth, thread, image_hash):
    records = [r for r in fixture.records(codex_home) if r.get('pid') == pid]
    require(len(records) == 1 and records[0].get('sessionId') == thread,
            'the verified destination no longer has its unique conversation record')
    try:
        evidence = fixture.process_evidence(records[0])
    except (OSError, RuntimeError, ValueError, KeyError, TypeError):
        raise RuntimeError('the destination process record could not be verified') from None
    require(evidence['kernelBirth'] == birth and live_birth(pid) == birth
            and evidence['image']['sha256'] == image_hash,
            'the destination process no longer matches the verified handoff')


def verify_claim_loss(card, ctl, *, pid, birth, thread, codex_home, report):
    require(report.get('original_input_and_completed_plan_preserved') is True
            and type(pid) is int and 0 < pid < 2**31 and type(birth) is int and birth > 0
            and report.get('successor_pid') == pid and thread and report.get('thread') == thread
            and report.get('codex_sha256'), 'claim-loss check requires a verified quota handoff')
    require_destination(codex_home, pid, birth, thread, report['codex_sha256'])
    with closing(select.kqueue()) as watcher:
        note = select.kevent(pid, filter=select.KQ_FILTER_PROC, flags=select.KQ_EV_ADD | select.KQ_EV_ONESHOT,
                             fflags=select.KQ_NOTE_EXIT)
        watcher.control([note], 0, 0)
        opened_at = card.open()
        require_destination(codex_home, pid, birth, thread, report['codex_sha256'])
        card.require_open()
        deadline = opened_at + CLAIM_WINDOW_SECONDS
        require(card.clock() < deadline and not watcher.control(None, 1, 0)
                and live_birth(pid) == birth, 'the destination claim or its card expired before restart')
        ack = f'OK restarting pid {pid} cause=asked'
        require(ctl('restart', f'pid={pid}') in (ack, f'{ack} discarded=0'),
                'the host did not acknowledge the plain guarded destination restart')
        while True:
            card.front()
            remaining = deadline - card.clock()
            require(remaining > 0, 'the verified destination did not exit within the card observation period')
            events = watcher.control(None, 1, min(remaining, .1))
            if events:
                event = events[0]
                require(event.ident == pid and event.filter == select.KQ_FILTER_PROC
                        and not (event.flags & select.KQ_EV_ERROR) and event.fflags & select.KQ_NOTE_EXIT,
                        'the kernel did not confirm the guarded destination exit')
                break
        card.await_withdrawal(opened_at)
    report['destination_card_claim_loss'] = {
        'pid': pid, 'guarded_restart': True, 'kernel_exit': True,
        'withdrawn_before_expiry': True, 'elapsed_ms': round((card.clock() - opened_at) * 1000),
    }
