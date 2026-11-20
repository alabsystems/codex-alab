# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'smoke'))
from rotation_account_card import CardCheck


SID = 'tq4n7w2v8rh'
CLOSED = 'fact-menu open=false'
OPEN = f'fact-menu open=true session={SID} box=account-addresses'
PRIVATE = 'private-account@example.invalid'


class PrivateHost:
    def __init__(self):
        self.now = 100.0
        self.sessions = f'0 {SID} - alive Shell meta=0 window=0 active=1 wfocus=1 path=live'
        self.focus = 'OK available=1 window=89 app_active=1 window_key=1 key=1 posted_keys=0'
        self.menu = CLOSED
        self.menu_on_left = OPEN
        self.text = ('Accounts\n› refresh\n↓ scroll · pgdn page · ← show addresses · '
                     '→ switch account · enter refresh · esc back')
        self.calls = []
        self.left_at = None
        self.withdraw_at = None
        self.menu_latency = 0.0
        self.during_control = {}

    def sleep(self, seconds):
        self.now += seconds

    def ctl(self, *args):
        self.calls.append(args)
        if change := self.during_control.get(args):
            change()
        if args == ('sessions',):
            return self.sessions
        if args == ('native-focus',):
            return self.focus
        if args == ('controls', 'fact-menu'):
            self.sleep(self.menu_latency)
            if self.withdraw_at is not None and self.now >= self.withdraw_at:
                self.menu = CLOSED
            return self.menu
        if args == ('text', 'trim'):
            return self.text
        if args == ('key', 'left'):
            self.left_at = self.now
            self.sleep(.5)
            self.menu = self.menu_on_left
            return 'OK'
        if args == ('open', 'fact-menu', 'close'):
            self.menu = CLOSED
            return 'OK'
        if args == ('key', 'esc'):
            self.text = 'Codex'
            return 'OK'
        if args in (('send', '/accounts'), ('key', 'enter')):
            return 'OK'
        raise AssertionError(f'unexpected private control command: {args!r}')

    def check(self):
        return CardCheck(self.ctl, SID, clock=lambda: self.now, sleep=self.sleep)


class AccountCardTests(unittest.TestCase):
    def test_open_observes_the_requested_card_and_dates_it_before_the_left_key(self):
        host = PrivateHost()
        check = host.check()
        opened = check.open()
        self.assertEqual(opened, host.left_at)
        self.assertLess(opened, host.now)
        self.assertEqual([call for call in host.calls if call[0] in ('send', 'key')],
                         [('send', '/accounts'), ('key', 'enter'), ('key', 'left')])
        left = host.calls.index(('key', 'left'))
        self.assertIn(('controls', 'fact-menu'), host.calls[:host.calls.index(('send', '/accounts'))])
        self.assertIn(('text', 'trim'), host.calls[host.calls.index(('key', 'enter')) + 1:left])
        self.assertIn(('controls', 'fact-menu'), host.calls[left + 1:])
        host.withdraw_at = host.now + .3
        check.await_withdrawal(opened)
        check.require_closed()

    def test_open_requires_the_previous_card_to_be_closed(self):
        host = PrivateHost()
        host.menu = OPEN
        with self.assertRaises(RuntimeError):
            host.check().open()
        self.assertNotIn(('send', '/accounts'), host.calls)

    def test_missing_address_action_keeps_the_left_key_out_of_the_private_session(self):
        host = PrivateHost()
        host.text = f'Accounts\n{PRIVATE}\n› refresh\n↓ scroll · enter refresh · esc back'
        with self.assertRaises(RuntimeError) as error:
            host.check().open()
        self.assertNotIn(PRIVATE, str(error.exception))
        self.assertNotIn(('key', 'left'), host.calls)

    def test_transcript_mention_of_show_addresses_does_not_authorize_the_left_key(self):
        host = PrivateHost()
        host.text = f'Last reply from {PRIVATE}\nUse the show addresses action to see the account.\n› '
        with self.assertRaises(RuntimeError) as error:
            host.check().open()
        self.assertNotIn(PRIVATE, str(error.exception))
        self.assertNotIn(('key', 'left'), host.calls)

    def test_old_address_hint_above_the_current_accounts_footer_cannot_authorize_left(self):
        host = PrivateHost()
        host.text = ('You asked to show addresses\nAccounts\n› refresh\n'
                     '↓ scroll · pgdn page · enter refresh · esc back')
        with self.assertRaises(RuntimeError):
            host.check().open()
        self.assertNotIn(('key', 'left'), host.calls)

    def test_wrong_card_or_additional_control_rows_cannot_confirm_open(self):
        for observation in (OPEN.replace(SID, 'tother12345'), OPEN.replace('account-addresses', 'quota'),
                            f'{OPEN}\nitem={PRIVATE}', f'{OPEN} email={PRIVATE}'):
            with self.subTest(observation=observation):
                host = PrivateHost()
                host.menu_on_left = observation
                with self.assertRaises(RuntimeError) as error:
                    host.check().open()
                self.assertNotIn(PRIVATE, str(error.exception))

    def test_require_closed_accepts_only_a_single_unambiguous_closed_header(self):
        for observation in (OPEN, '', f'{CLOSED}\nitem={PRIVATE}', f'{CLOSED} email={PRIVATE}'):
            with self.subTest(observation=observation):
                host = PrivateHost()
                host.menu = observation
                with self.assertRaises(RuntimeError) as error:
                    host.check().require_closed()
                self.assertNotIn(PRIVATE, str(error.exception))

    def test_source_expiry_or_a_slow_reply_cannot_count_as_withdrawal(self):
        for natural_expiry, read_delay in ((30, 0), (28.1, .4)):
            with self.subTest(natural_expiry=natural_expiry, read_delay=read_delay):
                host = PrivateHost()
                check = host.check()
                opened = check.open()
                host.withdraw_at = opened + natural_expiry
                host.now = opened + 27.8
                host.menu_latency = read_delay
                with self.assertRaises(RuntimeError):
                    check.await_withdrawal(opened)

    def test_another_menu_cannot_stand_in_for_withdrawal(self):
        host = PrivateHost()
        check = host.check()
        opened = check.open()
        host.menu = f'fact-menu open=true session={SID} box={PRIVATE}'
        with self.assertRaises(RuntimeError) as error:
            check.await_withdrawal(opened)
        self.assertNotIn(PRIVATE, str(error.exception))

    def test_dismissing_the_source_closes_a_present_card_before_leaving_codex_details(self):
        for observed, expected in ((OPEN, 'open'), (CLOSED, 'withdrawn')):
            with self.subTest(observed=observed):
                host = PrivateHost()
                check = host.check()
                check.open()
                check.require_open()
                host.menu = observed
                host.calls.clear()
                self.assertEqual(check.dismiss_source(), expected)
                actions = [call for call in host.calls if call[0] in ('open', 'key')]
                close = [('open', 'fact-menu', 'close')] if observed == OPEN else []
                self.assertEqual(actions, close + [('key', 'esc')])
                self.assertEqual(host.menu, CLOSED)

    def test_missing_source_details_never_sends_escape_to_a_replacement_pane(self):
        host = PrivateHost()
        check = host.check()
        check.open()
        host.menu = CLOSED
        host.text = f'Another pane\n{PRIVATE} asked to show addresses'
        with self.assertRaises(RuntimeError) as error:
            check.dismiss_source()
        self.assertNotIn(PRIVATE, str(error.exception))
        self.assertNotIn(('key', 'esc'), host.calls)

    def test_dismisses_source_details_after_the_host_withdraws_the_address_action(self):
        host = PrivateHost()
        check = host.check()
        check.open()
        host.menu = CLOSED
        host.text = 'Accounts ↓\n› refresh\n↓ scroll · pgdn page · enter refresh · esc back'
        self.assertEqual(check.dismiss_source(), 'withdrawn')
        self.assertEqual(host.text, 'Codex')

    def test_losing_the_private_session_or_native_focus_invalidates_a_closed_sample(self):
        for field, marker in (('sessions', SID), ('sessions', 'active=1'), ('focus', 'window_key=1')):
            with self.subTest(field=field, marker=marker):
                host = PrivateHost()
                check = host.check()
                opened = check.open()
                host.menu = CLOSED
                setattr(host, field, getattr(host, field).replace(marker, 'unknown'))
                with self.assertRaises(RuntimeError):
                    check.await_withdrawal(opened)

    def test_focus_replacement_during_the_native_menu_read_invalidates_the_sample(self):
        for field in ('window', 'second session'):
            with self.subTest(field=field):
                host = PrivateHost()
                check = host.check()
                opened = check.open()
                host.menu = CLOSED
                def replace_front():
                    if field == 'window':
                        host.focus = host.focus.replace('window=89', 'window=90')
                    else:
                        host.sessions += '\n' + host.sessions

                host.during_control['controls', 'fact-menu'] = replace_front
                with self.assertRaises(RuntimeError):
                    check.await_withdrawal(opened)

    def test_losing_focus_during_input_or_pane_inspection_withholds_the_followup_key(self):
        for action, key in (('send', 'enter'), ('read', 'esc')):
            with self.subTest(action=action):
                host = PrivateHost()
                check = host.check()

                def lose_focus():
                    host.focus = host.focus.replace('window_key=1', 'window_key=0')

                if action == 'send':
                    host.during_control['send', '/accounts'] = lose_focus
                    operation = check.open
                else:
                    check.open()
                    host.during_control['text', 'trim'] = lose_focus
                    operation = check.dismiss_source
                with self.assertRaises(RuntimeError):
                    operation()
                self.assertNotIn(('key', key), host.calls)

    def test_source_pane_disappearance_is_witnessed_in_the_original_window(self):
        host = PrivateHost()
        check = host.check()
        check.open()

        def replace_window():
            host.focus = host.focus.replace('window=89', 'window=90')

        host.during_control['key', 'esc'] = replace_window
        with self.assertRaises(RuntimeError):
            check.dismiss_source()
