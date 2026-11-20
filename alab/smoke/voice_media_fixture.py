#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Synthetic voice-host IPC peer. No native media library or audio device is opened."""

import json
import sys


def main():
    responses = {
        "hello": {"type": "ready"},
        "initializeRuntime": {"type": "runtimeReady"},
        "startTransport": {"type": "offer", "sdp": "v=0\r\ns=fixture-offer\r\n"},
        "applyAnswer": {"type": "transportReady"},
        "openDevices": {"type": "devicesOpened"},
        "setAudioControls": {"type": "audioControlsApplied"},
        "inspectAudio": {"type": "audioState", "state": {"microphonePeak": 0, "speakerPeak": 0}},
        "close": {"type": "closed"},
    }
    source, sink = sys.stdin.buffer, sys.stdout.buffer
    while True:
        header = source.read(4)
        if not header:
            return
        length = int.from_bytes(header, "big")
        if len(header) != 4 or not 0 < length <= 128 * 1024:
            raise ValueError("unexpected fixture command framing")
        request = json.loads(source.read(length))
        name = request["type"]
        if name == "hello" and request["protocol"] != 1:
            raise ValueError("unsupported fixture protocol")
        if name == "applyAnswer" and request["sdp"] != "v=0\r\ns=fixture-answer\r\n":
            raise ValueError("unexpected answer")
        if name == "listDevices":
            reply = {"type": "deviceList", "devices": [{"name": "Fixture", "channels": 1, "isDefault": True}]}
        else:
            reply = responses[name]
        payload = json.dumps(reply).encode()
        sink.write(len(payload).to_bytes(4, "big") + payload)
        sink.flush()
        if name == "close":
            return


if __name__ == "__main__":
    main()
