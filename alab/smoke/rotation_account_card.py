# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Drive the account address card in the rotation fixture's private, focused Mac window."""

import re
import time

from capacity_exec import require


# A 30-second native card expiry cannot serve as evidence of authority loss.
CLAIM_WINDOW_SECONDS = 28


def focused_session(ctl):
    lines = ctl('sessions').splitlines()
    require(len(lines) == 1, 'the account-card fixture requires its own single session')
    row = lines[0].split()
    require(len(row) >= 5 and row[0].isdigit() and re.fullmatch(r'[\w-]+', row[1], re.ASCII)
            and row[3] == 'alive' and {'active=1', 'wfocus=1'} <= set(row[5:])
            and any(re.fullmatch(r'window=\d+', field) for field in row[5:]),
            'the private account-card session is unavailable or unfocused')
    return row[1]


class CardCheck:
    def __init__(self, ctl, sid, *, clock=time.monotonic, sleep=time.sleep):
        self.ctl = ctl
        self.sid = sid
        self.clock = clock
        self.sleep = sleep
        self.window = None
        self.front()

    def front(self):
        require(focused_session(self.ctl) == self.sid, 'the account-card session changed')
        focus = self.ctl('native-focus').split()
        windows = [field for field in focus if re.fullmatch(r'window=\d+', field)]
        require(len(windows) == 1 and {'available=1', 'app_active=1', 'window_key=1', 'key=1'} <= set(focus),
                'the private account-card window lost native focus')
        if self.window is None:
            self.window = windows[0]
        require(windows[0] == self.window, 'the native account-card window changed')

    def is_open(self):
        self.front()
        menu = self.ctl('controls', 'fact-menu')
        self.front()
        if menu == 'fact-menu open=false':
            return False
        require(menu == f'fact-menu open=true session={self.sid} box=account-addresses',
                'unexpected native menu in the private account-card window')
        return True

    def require_closed(self):
        require(not self.is_open(), 'the native account card is still open')

    def require_open(self):
        require(self.is_open(), 'the native account card is no longer open')

    def wait(self, predicate, deadline, failure):
        while self.clock() < deadline:
            matched = predicate()
            if matched and self.clock() < deadline:
                return
            self.sleep(.1)
        raise RuntimeError(failure)

    def details_visible(self, *, addresses=False):
        lines = [line.strip() for line in self.ctl('text', 'trim').splitlines()]
        titles = [i for i, line in enumerate(lines) if re.fullmatch(r'Accounts(?: [↑↓]+)?', line)]
        if not titles:
            return False
        accepts = [i for i in range(titles[-1] + 1, len(lines)) if lines[i] == '› refresh']
        if not accepts:
            return False
        footer = ' '.join(' '.join(lines[accepts[-1] + 1:]).split())
        return not addresses or '← show addresses' in footer

    def open(self):
        self.require_closed()
        self.ctl('send', '/accounts')
        self.front()
        self.ctl('key', 'enter')
        self.wait(lambda: self.details_visible(addresses=True), self.clock() + 10,
                  'the accounts pane did not offer its address action')
        self.front()
        opened = self.clock()
        self.ctl('key', 'left')
        self.wait(self.is_open, opened + 10, 'the private host did not open the native account card')
        return opened

    def dismiss_source(self):
        state = 'open' if self.is_open() else 'withdrawn'
        # This native action cannot send Escape into Codex if a concurrent roster change
        # has just withdrawn the address card. Only then dismiss Codex's own details.
        if state == 'open':
            self.ctl('open', 'fact-menu', 'close')
        self.require_closed()
        self.front()
        require(self.details_visible(), 'the source accounts pane disappeared during inspection')
        self.front()
        self.ctl('key', 'esc')
        self.wait(lambda: not self.details_visible(), self.clock() + 10, 'the source accounts pane stayed open')
        self.front()
        return state

    def await_withdrawal(self, opened_at):
        self.wait(lambda: not self.is_open(), opened_at + CLAIM_WINDOW_SECONDS,
                  'the destination card did not withdraw before its ordinary expiry')
