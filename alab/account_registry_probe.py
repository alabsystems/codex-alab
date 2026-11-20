#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Read aterm's registry associations; never enroll or authenticate an account."""
import argparse
import atexit
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile


QUERIES = {
    # The default is the materialized display table. Only the registry view
    # carries typed seat bindings and the seats without a machine assignment.
    'machines': ['aterm', 'db', 'computers', '--registry'],
    # This independent typed view resolves disputed machine names by stable key.
    'canonical': ['aterm', 'db', 'alab', 'machines'],
    'accounts': ['aterm', 'db', 'table', 'scan', 'alab/registry', 'account', '--json'],
    'preferences': ['aterm', 'db', 'alab', 'preferences'],
    'observations': ['aterm', 'db', 'table', 'scan', 'alab/registry', 'account-obs', '--json'],
}
PUBLISHED_MACHINES = ['aterm', 'file', 'cat', 'lab/machines', '--published', '--json']
FILE_STATES = ('live', 'held', 'never', 'off')
FILE_REASONS = (None, 'succession', 'membership', 'busy', 'policy', 'keys', 'catalog', 'custody')
HERE_UNCONFIRMED = 'current-machine-binding-unconfirmed'
HERE_CHANGED = 'current-machine-binding-changed'
KEY = re.compile(r'(?:mc|ac|ad|mb)-[0-9a-f]{16}\Z')
ASCII_LOWER = str.maketrans('ABCDEFGHIJKLMNOPQRSTUVWXYZ', 'abcdefghijklmnopqrstuvwxyz')
GRID_CONFLICT = '⚠ '
LIFECYCLE = ('live', 'retired', 'ended', 'merged', 'locked')
ACCOUNT_STATUSES = (None, 'pending', 'active', 'paused', 'broken', 'banned', 'suspended', 'cancelled')
# aterm-db alab::observe::CLIS. Other observation strings are private source text.
OBSERVATION_CLIS = ('claude', 'codex', 'grok', 'kimi', 'agy', 'muse', 'codex-alab')
SEAT = re.compile(r'[0-9a-f]{32}\Z')
LIMITS = [
    'These are local replicated registry observations, not fresh provider authentication.',
    'Preferences, retained observations and placements establish associations, not login usability.',
    'Commands are separate reads, not an atomic registry snapshot.',
]


def unique_json_object(pairs):
    result = dict(pairs)
    if len(result) != len(pairs):
        raise ValueError('duplicate JSON member')
    return result


def query(command):
    receipt = {'command': command}
    try:
        completed = subprocess.run(command, capture_output=True, text=True, timeout=20)
        receipt['exit_code'] = completed.returncode
        if completed.returncode:
            receipt['error'] = 'command-failed'
            not_ready = ' '.join(command[:3]) + ': Commons membership is not ready for table access'
            if (command[:2] == ['aterm', 'db'] and len(command) >= 3
                    and completed.stderr.strip() == not_ready):
                receipt['reason'] = 'commons-membership-not-ready'
            return None, receipt
        value = json.loads(completed.stdout, object_pairs_hook=unique_json_object)
        if not isinstance(value, dict):
            raise ValueError('object required')
        return value, receipt
    except subprocess.TimeoutExpired:
        receipt['error'] = 'timeout'
    except OSError:
        receipt['error'] = 'unavailable'
    except (ValueError, UnicodeError):
        receipt['error'] = 'invalid-json'
    return None, receipt


def text(value):
    return isinstance(value, str) and bool(value) and not any(ord(c) < 32 for c in value)


def key(value, prefix):
    return text(value) and value.startswith(prefix + '-') and KEY.fullmatch(value) is not None


def address_spelling(value):
    # Native AddressIndex uses Rust's trim_ascii(), which leaves vertical tabs in place.
    return value.strip(' \t\n\f\r').translate(ASCII_LOWER)


def address_spellings(field):
    # AddressIndex records each row once under every spelling its candidates claim.
    candidates = field.get('conflict') if isinstance(field, dict) else [field]
    if not isinstance(candidates, list):
        raise ValueError('invalid-address-collection')
    return {address_spelling(candidate) for candidate in candidates if isinstance(candidate, str)}


def login_address(value):
    value = address_spelling(value)
    local, _, domain = value.partition('@')
    if (not local or '@' in domain or '.' not in domain or domain.startswith('.')
            or domain.endswith('.') or len(value.encode('utf-8')) > 254
            or any(c.isspace() or ord(c) < 32 or 127 <= ord(c) <= 159 for c in value)):
        raise ValueError('expected login must be a registered email address')
    return value


def read_addresses(reader, receipts):
    # A native --where comparison can omit an older mixed-case address or an
    # ASCII-padded spelling. Either can make a requested login ambiguous.
    value, receipt = reader(['aterm', 'db', 'table', 'scan', 'alab/registry', 'address', '--json'])
    receipts.append(receipt)
    if receipt.get('error'):
        raise ValueError('address-query-failed')
    entries = rows(value, 'rows', complete=False)
    if value.get('collection') != 'address' or type(value.get('malformed')) is not int or value['malformed']:
        raise ValueError('invalid-address-collection')
    return entries


def conflict_candidates(cell):
    candidates = cell.get('conflict') if isinstance(cell, dict) and set(cell) == {'conflict'} else None
    if (isinstance(candidates, list) and len(candidates) >= 2
            and all(v is None or isinstance(v, str) for v in candidates)
            and len(set(candidates)) == len(candidates)):
        return candidates
    return None


def grid_conflict(candidates):
    return GRID_CONFLICT + ' ‖ '.join(v or '' for v in candidates)


def identity_grid_cell(field, cell):
    # A raw scalar shaped like a grid dispute cannot corroborate its claimants.
    if isinstance(cell, str) and cell.startswith(GRID_CONFLICT):
        raise ValueError('unconfirmed-login-fields')
    if not isinstance(cell, dict) or field not in ('addr', 'login'):
        return cell
    candidates = conflict_candidates(cell)
    if candidates is None:
        raise ValueError('unconfirmed-login-fields')
    for candidate in candidates:
        if candidate is None:
            continue
        if field == 'addr':
            try:
                login_address(candidate)
            except ValueError:
                raise ValueError('unconfirmed-login-fields') from None
        elif not key(candidate, 'ad'):
            raise ValueError('unconfirmed-login-fields')
    # The join uses the typed candidates. The complete grid must independently
    # agree on their exact stored spellings and order, including any clear.
    return grid_conflict(candidates)


def corroborated_status_disputes(accounts, grid):
    # Only address reports may disregard a dispute. Both account views must
    # identify the same candidates, and no candidate becomes the account's status.
    disputes = {}
    for account in accounts:
        status = account.get('status')
        if status in ACCOUNT_STATUSES:
            continue
        candidates = conflict_candidates(status)
        if candidates is None or any(value not in ACCOUNT_STATUSES for value in candidates):
            raise ValueError('invalid-account')
        disputes[account['pk']] = grid_conflict(candidates)
    displayed = {row['ac']: row['status'] for row in grid
                 if isinstance(row.get('status'), str) and row['status'].startswith(GRID_CONFLICT)}
    if displayed != disputes:
        raise ValueError('unconfirmed-account-status')
    return set(disputes)


def confirm_login_tables(accounts, addresses, reader, receipts, setup_accounts=(), *,
                         include_aliases=False, checking_mail=False):
    """The raw scans can omit completeness. Confirm their identity fields against complete live views."""
    alias_view = None
    status_disputes = set()
    for noun, options, typed, prefix, fields in (
            ('ai-accounts', ['--registry'], accounts, 'ac', ('row', 'provider', 'login')),
            ('emails', [], addresses, 'ad', ('row', 'addr'))):
        value, receipt = reader(['aterm', 'db', noun, *options])
        receipts.append(receipt)
        if receipt.get('error'):
            raise ValueError('login-view-unavailable')
        shown = rows(value, noun)
        if value.get('table') != noun or value.get('state') != 'live' or value.get('why', '') is not None:
            raise ValueError('login-view-unconfirmed')

        def indexed(entries, pk):
            result = {}
            for row in entries:
                identity = row.get(pk)
                checked = fields + ('status',) if noun == 'ai-accounts' and identity in setup_accounts else fields
                cells = tuple(row.get(field) for field in checked)
                # Retired rows cannot own a login. Their keys and lifecycle still
                # have to agree, even when the grid renders an old disputed cell.
                if row.get('row') in LIFECYCLE[1:]:
                    cells = (row['row'],)
                elif pk == 'pk' and typed_lifecycle(row) == 'live':
                    cells = tuple(identity_grid_cell(field, cell) for field, cell in zip(checked, cells))
                if (not key(identity, prefix) or identity in result
                        or (pk == prefix and not all(field in row for field in checked))
                        or any(cell is not None and not isinstance(cell, str) for cell in cells)):
                    raise ValueError('unconfirmed-login-fields')
                result[identity] = cells
            return result

        # The native nouns render scalar references and addresses in their exact spelling.
        # Require every key and lifecycle to agree, plus the identity fields of possible owners.
        if indexed(typed, 'pk') != indexed(shown, prefix):
            raise ValueError('unconfirmed-login-fields')
        if checking_mail and noun == 'ai-accounts':
            status_disputes = corroborated_status_disputes(typed, shown)
        if noun == 'emails' and include_aliases:
            alias_view = {row['ad']: row['alias_of'] for row in shown if 'alias_of' in row}
    return alias_view, status_disputes


def typed_cell(row, field):
    return field in row, row.get(field)


def has_null_lifecycle(row):
    return 'row' in row and row['row'] is None


def typed_lifecycle(row):
    # Row::live treats the typed null left by restoring absence as unwritten.
    return 'live' if row.get('row') is None else row['row']


def resolve_logins(logins, accounts, entries, *, discover=None):
    """Join account.login to address.addr and require its reverse lookup to be unique."""
    registered = {}
    owners = {}
    disputed = set()
    for row in accounts:
        if typed_lifecycle(row) != 'live' or row['provider'] != 'openai':
            continue
        address = row.get('login')
        if isinstance(address, dict):
            candidates = address.get('conflict')
            if not isinstance(candidates, list) or not candidates or any(
                    candidate is not None and not key(candidate, 'ad') for candidate in candidates):
                raise ValueError('invalid-login-reference')
            disputed.update(candidates)
            continue
        if address is not None:
            if not key(address, 'ad'):
                raise ValueError('invalid-login-reference')
            owners.setdefault(address, []).append(row['pk'])
            registered[row['pk']] = address

    by_key = {}
    for row in entries:
        address = row.get('pk')
        if not key(address, 'ad') or address in by_key:
            raise ValueError('invalid-address-collection')
        by_key[address] = row
    if discover is not None:
        logins = []
        for account in discover:
            address = registered.get(account)
            row = by_key.get(address)
            if row is None or typed_lifecycle(row) != 'live' or not isinstance(row.get('addr'), str):
                raise ValueError('unconfirmed-registered-login')
            logins.append(login_address(row['addr']))

    holders = {login: [] for login in logins}
    for address, row in by_key.items():
        field = row.get('addr')
        for spelling in address_spellings(field) & holders.keys():
            lifecycle = typed_lifecycle(row)
            if lifecycle not in LIFECYCLE:
                raise ValueError('invalid-address-lifecycle')
            if lifecycle == 'live':
                if isinstance(field, dict):
                    raise ValueError('ambiguous-login-address')
                holders[spelling].append(address)
    result = []
    for login in logins:
        addresses = holders[login]
        if len(addresses) != 1:
            raise ValueError('ambiguous-login-address' if addresses else 'unresolved-login-address')
        if addresses[0] in disputed:
            raise ValueError('ambiguous-login-account')
        matches = owners.get(addresses[0], [])
        if len(matches) != 1:
            raise ValueError('ambiguous-login-account' if matches else 'unresolved-login-account')
        result.append((login, addresses[0], matches[0]))
    resolved = [account for _, _, account in result]
    if len(set(resolved)) != len(resolved):
        raise ValueError('duplicate-login-account')
    if discover is not None and resolved != discover:
        raise ValueError('unconfirmed-registered-login')
    return result


def confirm_registry_source(value):
    # Working views can be complete while an unpublished local edit changes an association.
    if isinstance(value, dict) and ('registry_source' in value or 'pending_cells' in value):
        pending = value.get('pending_cells')
        if value.get('registry_source') != 'commons' or type(pending) is not int or pending != 0:
            raise ValueError('registry-view-unconfirmed')


def rows(value, field, complete=True):
    confirm_registry_source(value)
    if not isinstance(value, dict) or ((complete or 'complete' in value) and value.get('complete') is not True):
        raise ValueError('incomplete-view')
    result = value.get(field)
    if not isinstance(result, list) or not all(isinstance(row, dict) for row in result):
        raise ValueError('invalid-rows')
    return result


def read_mail_metadata(reader, receipts, collection=None, pk=None):
    command = (['aterm', 'db', 'alab', 'mail'] if collection is None else
               ['aterm', 'db', 'table', 'get', 'alab/registry', collection, pk, '--json'])
    view, receipt = reader(command)
    if collection is not None:
        # A candidate might be withdrawn by the next read.
        receipt = dict(receipt, command=[*command[:-2], f'<selected-{collection}>', '--json'])
    receipts.append(receipt)
    source = 'mail-view' if collection is None else f'{collection}-detail'
    if receipt.get('error'):
        raise ValueError(f'{source}-unavailable')
    confirm_registry_source(view)
    if (not isinstance(view, dict) or view.get('state', 'live') != 'live'
            or view.get('complete', True) is not True
            or any(view.get(k) is not None for k in ('why', 'kept', 'refused', 'problem'))):
        raise ValueError(f'{source}-unconfirmed')
    return view


def mail_row_detail(collection, pk, reader, receipts):
    detail = read_mail_metadata(reader, receipts, collection, pk)
    row = detail.get('row')
    if (detail.get('collection') != collection or not isinstance(row, dict) or row.get('pk') != pk
            or any(not isinstance(detail.get(k), list) or not all(isinstance(name, str) for name in detail[k])
                   for k in ('conflicts', 'raw'))):
        raise ValueError(f'invalid-{collection}-detail')
    return detail


def login_lifecycle_evidence(accounts, addresses, associated, logins, setup_accounts, reader, receipts):
    # The complete nouns show null both for absence and for a restored cell. For
    # the latter, get also reports the native live interpretation and cell flags.
    # Retain the scan's exact form privately, even when both joins resolve alike.
    cells, address_details = {}, {}
    account_keys = set(associated) | {account for _, _, account in logins}
    address_keys = {address for _, address, _ in logins}
    for collection, entries, selected, fields in (
            ('account', accounts, account_keys, ('row', 'provider', 'login')),
            ('address', addresses, address_keys, ('row', 'addr'))):
        for row in entries:
            pk = row['pk']
            if pk not in selected:
                continue
            cell = typed_cell(row, 'row')
            cells[collection, pk] = cell
            setup = collection == 'account' and pk in setup_accounts
            restored = cell == (True, None)
            # A raw status can look absent or active in both the scan and the
            # complete noun. Only the selected account's get exposes that flag.
            if not (restored or setup):
                continue
            detail = mail_row_detail(collection, pk, reader, receipts)
            checked = fields + ('status',) if setup else fields
            if (detail.get('lifecycle') != 'live'
                    or any(field in detail['raw'] or field in detail['conflicts']
                           or typed_cell(row, field) != typed_cell(detail['row'], field) for field in checked)):
                raise ValueError('unconfirmed-restored-lifecycle' if restored else 'setup-accounts-unconfirmed')
            if collection == 'address':
                address_details[pk] = detail
    return cells, address_details


def live_mailbox(mailbox, shown_lifecycle, reader, receipts):
    """The native mail table can show a provisional lifecycle for a conflict."""
    if shown_lifecycle not in (None, 'live'):
        return False
    detail = mail_row_detail('mailbox', mailbox, reader, receipts)
    if 'row' in detail['conflicts'] or 'row' in detail['raw']:
        return False
    # get renders the unwritten lifecycle as live, while the mail view renders null.
    if detail.get('lifecycle') != 'live' or detail['row'].get('row') != shown_lifecycle:
        raise ValueError('mailbox-lifecycle-changed')
    return True


def alias_declaration(value, flags):
    if flags is None or 'raw' in flags:
        return 'unconfirmed'
    if 'conflicts' in flags:
        candidates = conflict_candidates(value)
        valid = candidates is not None and all(v is None or key(v, 'ad') for v in candidates)
        return 'disputed' if valid else 'unconfirmed'
    if value is None:
        return 'absent'
    return 'declared' if key(value, 'ad') else 'unconfirmed'


def declared_mail_path(address, typed, flags):
    """Return the visited addresses and their first owner, or None for an invalid or provisional cell."""
    declared, seen = [], set()
    current = address
    while current in typed and current not in seen:
        row = typed[current]
        lifecycle, protected = typed_lifecycle(row), row.get('protected')
        if flags(current, 'row'):
            return None
        if lifecycle in LIFECYCLE[1:]:
            break
        if lifecycle != 'live' or flags(current, 'protected'):
            return None
        if protected is True:
            break
        if protected is not None and protected is not False:
            return None
        # The typed scan can show competing address identities even when an
        # optional detail is unavailable or omits its conflict marker.
        if flags(current, 'addr') or isinstance(row.get('addr'), dict) or flags(current, 'mailbox'):
            return None
        declared.append(current)
        seen.add(current)
        owner = row.get('mailbox')
        if owner is not None:
            # A settled owner decides the route before alias_of is read.
            return (declared, owner) if key(owner, 'mb') else None
        alias = row.get('alias_of')
        alias_flags = flags(current, 'alias_of')
        if alias_flags or (alias is not None and alias_declaration(alias, alias_flags) != 'declared'):
            return None
        current = alias
    return declared, None


def settled_address_route(address, mailbox, evidence, via, typed, flags, tag_holders, summary_excludes_alias):
    walked = declared_mail_path(address, typed, flags)
    if walked is None:
        return False
    declared, owner = walked
    path = [address, *via]
    if owner is not None:
        return owner == mailbox and evidence == 'owner' and declared == path
    if evidence != 'observed':
        return False
    if declared[:len(path)] == path:
        first_failed = len(path)
    else:
        # A final provider-observed +tag edge can be inferred from the base address
        # without following its own mailbox or alias. Native still checks protection.
        if not via or declared[:len(path) - 1] != path[:-1]:
            return False
        parent, base = typed[path[-2]], typed.get(path[-1], {})
        if (typed_lifecycle(base) != 'live' or not isinstance(base.get('addr'), str)
                or (base.get('protected') is not None and base['protected'] is not False)
                or not isinstance(parent.get('addr'), str)
                or any(flags(path[-1], field) for field in ('row', 'addr', 'protected'))):
            return False
        local, _, domain = address_spelling(parent['addr']).partition('@')
        untagged, plus, _ = local.partition('+')
        if not plus:
            return False
        try:
            spelling = login_address(f'{untagged}@{domain}')
        except ValueError:
            return False
        if address_spelling(base['addr']) != spelling:
            return False
        # listed_base asks AddressIndex for one live holder before checking that
        # row's protection. Even a protected second holder makes the name ambiguous.
        if spelling not in tag_holders:
            tag_holders[spelling] = {
                pk for pk, row in typed.items()
                if typed_lifecycle(row) == 'live' and spelling in address_spellings(row.get('addr'))
            }
        if tag_holders[spelling] != {path[-1]}:
            return False
        if flags(path[-2], 'alias_of') is None and not summary_excludes_alias(path[-2], path[-1]):
            return False
        first_failed = len(path) - 1
    # Native tries each declared alias before using this address's observation.
    # A discarded alias needed a settled empty hint to return no destination.
    return all(not flags(failed, 'mailbox_hint') and typed[failed].get('mailbox_hint') is None
               for failed in declared[first_failed:])


class AddressEvidence:
    """Keep exact consulted cells private to one Registry read."""

    def __init__(self, typed, details, reader, receipts, changed):
        self.typed = typed
        self.details = details
        self.reader = reader
        self.receipts = receipts
        self.changed = changed
        self.consulted = {}

    def flags(self, address, field):
        token = (address, field)
        if token in self.consulted:
            return self.consulted[token][1]
        if address not in self.details:
            try:
                self.details[address] = mail_row_detail('address', address, self.reader, self.receipts)
            except ValueError:
                self.details[address] = None
        detail = self.details[address]
        scanned = self.typed[address]

        def cell(row):
            return json.dumps(typed_cell(row, field), sort_keys=True)

        value = cell(scanned)
        markers = None
        if detail is not None:
            if cell(detail['row']) != value:
                raise ValueError(self.changed)
            markers = tuple(kind for kind in ('raw', 'conflicts') if field in detail[kind])
            if field == 'row' and not markers and detail.get('lifecycle') != typed_lifecycle(scanned):
                raise ValueError(self.changed)
        elif field == 'row' and has_null_lifecycle(scanned):
            markers = ('unconfirmed',)
        self.consulted[token] = value, markers
        return markers

    def alias_flags(self, address, shown):
        markers = self.flags(address, 'alias_of')
        alias = self.typed[address].get('alias_of')
        # Both the emails view and native mail show a settled target as its key
        # and an unset link as null. Native mail shows a dispute's provisional value.
        if markers == () and (alias is None or key(alias, 'ad')) and shown != (True, alias):
            raise ValueError(self.changed)
        return markers

    def declaration(self, address, shown):
        alias = alias_declaration(self.typed[address].get('alias_of'), self.alias_flags(address, shown))
        identity_confirmed = not (self.flags(address, 'row') or self.flags(address, 'addr'))
        return alias, identity_confirmed


def read_login_mail_fields(logins, address_rows, reader, receipts, details, alias_view):
    typed = {row['pk']: row for row in address_rows}
    if alias_view is not None:
        cells = AddressEvidence(typed, details, reader, receipts, 'alias-declarations-changed')
        fields = {}
        for _, address, account in logins:
            shown = typed_cell(alias_view, address)
            alias, confirmed = cells.declaration(address, shown)
            if alias == 'disputed':
                # Native emails renders the same ordered candidates as table get,
                # with an empty string for a clear. The mail summary does not.
                candidates = typed[address]['alias_of']['conflict']
                if shown != (True, grid_conflict(candidates)):
                    raise ValueError(cells.changed)
            fields[account] = {'alias_declaration': alias if confirmed else 'unconfirmed'}
        return fields, cells.consulted

    view = read_mail_metadata(reader, receipts)

    def index(table, prefix):
        found = {}
        for row in rows(view, table):
            pk = row.get(prefix)
            if not key(pk, prefix) or pk in found:
                raise ValueError('invalid-mail-view')
            found[pk] = row
        return found

    addresses, boxes = index('addresses', 'ad'), index('mailboxes', 'mb')
    cells = AddressEvidence(typed, details, reader, receipts, 'mail-routes-changed')
    routes, live = {}, {}
    tag_holders, tag_aliases = {}, {}

    def flags(address, field):
        if field == 'alias_of':
            return cells.alias_flags(address, typed_cell(addresses.get(address, {}), field))
        return cells.flags(address, field)

    def summary_excludes_alias(parent, target):
        # Native prepends a successful saved alias to via before trying a +tag.
        # Without typed detail, that same hop cannot prove which route supplied it.
        present, alias = typed_cell(addresses.get(parent, {}), 'alias_of')
        tag_aliases[parent] = json.dumps((present, alias), sort_keys=True)
        return present and (alias is None or (key(alias, 'ad') and alias != target))

    def confirmed_mailbox(mailbox):
        if mailbox not in live:
            live[mailbox] = live_mailbox(mailbox, boxes.get(mailbox, {}).get('row', ''), reader, receipts)
        return live[mailbox]

    def live_addresses(path):
        for link in path:
            shown = addresses.get(link, {}).get('row', '')
            if shown not in (None, 'live') or (has_null_lifecycle(typed.get(link, {})) and shown is not None):
                return False
        return True

    for _, address, account in logins:
        row = addresses.get(address, {})
        signing = row.get('accounts')
        # A live registered address can predate an explicit lifecycle word. The
        # native reducer itself follows only live addresses through an alias.
        if (not live_addresses([address]) or 'mailbox' not in row or not isinstance(signing, list)
                or not all(isinstance(a, dict) and key(a.get('ac'), 'ac') for a in signing)
                or sum(a['ac'] == account for a in signing) != 1):
            raise ValueError('mail-login-unconfirmed')
        alias, identity_confirmed = cells.declaration(address, typed_cell(row, 'alias_of'))
        observation = {'alias_declaration': alias if identity_confirmed else 'unconfirmed', 'status': 'unconfirmed'}
        route = row['mailbox']
        if route is None:
            # The native summary must return a live owner reached directly or
            # through settled explicit aliases. The contradiction stays private.
            walked = None
            if key(typed[address].get('mailbox'), 'mb') or alias == 'declared':
                walked = declared_mail_path(address, typed, flags)
            declared, owner = walked or ([], None)
            via = tuple(declared[1:])
            if (owner and live_addresses(via)
                    and (not via or flags(via[-1], 'mailbox') is not None)
                    and confirmed_mailbox(owner)):
                routes[account] = (observation, owner, via)
            else:
                if identity_confirmed:
                    observation['status'] = 'no-effective-route'
                routes[account] = (observation, None, ())
            continue
        if not isinstance(route, dict):
            raise ValueError('invalid-mail-route')
        mailbox, evidence, via = route.get('mb'), route.get('how'), route.get('via')
        if (not key(mailbox, 'mb') or evidence not in ('owner', 'observed', 'routed', 'hint')
                or not isinstance(via, list) or not all(key(link, 'ad') for link in via)
                or len({address, *via}) != len(via) + 1):
            raise ValueError('invalid-mail-route')
        observation['evidence'] = evidence
        if (evidence in ('owner', 'observed')
                and settled_address_route(address, mailbox, evidence, via, typed, flags, tag_holders,
                                          summary_excludes_alias)
                and live_addresses(via)
                and confirmed_mailbox(mailbox)):
            observation.update(status='reported', mailbox_key=mailbox)
        # Even when two candidates project to identical public output, their exact
        # mailbox and the native alias chain must agree across the reads.
        routes[account] = (observation, mailbox, tuple(via))
    fields = {account: {'mail_route': route[0]} for account, route in routes.items()}
    return fields, (routes, cells.consulted, tag_holders, tag_aliases)


def machine_index(entries):
    by_key = {}
    for row in entries:
        mc = row.get('mc')
        if (not key(mc, 'mc') or mc in by_key or not text(row.get('label'))
                or row.get('row') not in LIFECYCLE
                or (row.get('merged_into') is not None and not key(row['merged_into'], 'mc'))):
            raise ValueError('invalid-machine')
        by_key[mc] = row
    return by_key


def matches_machine(selector, machine_key, label):
    # A different machine can have this key as its display label.
    if key(selector, 'mc'):
        return selector == machine_key
    return text(label) and selector == label


def selected_machines(machine, data, *, require_live_binding=False):
    if require_live_binding:
        binding = data.get('machines')
        if (not isinstance(binding, dict) or binding.get('state') != 'live'
                or binding.get('why', '') is not None):
            raise ValueError('registry-view-unconfirmed')
    canonical = machine_index(rows(data.get('canonical'), 'machines'))
    seen = set()
    for row in canonical.values():
        add_seats(seen, row.get('uuids'))
    return [row for row in canonical.values() if row['row'] == 'live'
            and not row.get('merged_into') and matches_machine(machine, row['mc'], row['label'])]


def seat_set(value):
    if (not isinstance(value, list) or not all(text(s) and SEAT.fullmatch(s) for s in value)
            or len(set(value)) != len(value)):
        raise ValueError('invalid-machine-seats')
    return set(value)


def add_seats(seen, value):
    seats = seat_set(value)
    if seen.intersection(seats):
        raise ValueError('invalid-machine-seats')
    seen.update(seats)


def machine_bindings(data):
    bindings = rows(data.get('machines'), 'computers')
    unbound_seat_count = 0
    seen_seats = set()
    bound_rows = []
    for row in bindings:
        add_seats(seen_seats, row.get('seats'))
        if row.get('mc') is None and set(row) == {'mc', 'seats'}:
            unbound_seat_count += len(row['seats'])
        else:
            bound_rows.append(row)
    return machine_index(bound_rows), unbound_seat_count


def unmatched_machine_status(machine, selected, bindings):
    if selected:
        return 'ambiguous-machine'
    # Canonical names can differ from authored labels. Only an exact machine
    # key can contradict its own absence from the canonical list of live rows.
    row = bindings.get(machine)
    if row and row['row'] == 'live' and not row.get('merged_into'):
        raise ValueError('machine-association-changed')
    return 'machine-not-found'


def analyze(machine, expected, data, details, *, current_only=False, setup_accounts=(), checking_logins=False,
            status_disputes=()):
    """Only selected metadata is retained; source notes and address fields never escape."""
    by_machine, unbound_seat_count = machine_bindings(data)
    selected = selected_machines(machine, data, require_live_binding=current_only)
    accounts = rows(data.get('accounts'), 'rows', complete=False)
    if (data['accounts'].get('collection') != 'account' or type(data['accounts'].get('malformed')) is not int
            or data['accounts'].get('malformed') != 0):
        raise ValueError('invalid-account-collection')
    preferences = rows(data.get('preferences'), 'preferences')
    observations = []
    if not current_only:
        observed = data.get('observations')
        observations = rows(observed, 'rows', complete=False)
        if observed.get('collection') != 'account-obs':
            raise ValueError('invalid-observation-collection')
        if observed.get('malformed', 0) != 0:
            raise ValueError('malformed-observations')
    by_account = {}
    for row in accounts:
        account = row.get('pk')
        status = row.get('status')
        if setup_accounts:
            # Native account_status::usable admits a live row with no status, pending or active.
            if account in setup_accounts and status not in (None, 'pending', 'active'):
                raise ValueError('setup-accounts-unconfirmed')
        elif status not in ACCOUNT_STATUSES and account not in status_disputes:
            raise ValueError('invalid-account')
        if (not key(account, 'ac') or account in by_account
                or not text(row.get('provider')) or typed_lifecycle(row) not in LIFECYCLE
                or (not checking_logins and has_null_lifecycle(row))
                or (row.get('label') is not None and not text(row['label']))):
            raise ValueError('invalid-account')
        by_account[account] = row
    for row in preferences:
        if (not key(row.get('mc'), 'mc') or not key(row.get('ac'), 'ac')
                or row.get('row') not in LIFECYCLE):
            raise ValueError('invalid-preference')
    for observation in observations:
        row = observation.get('row')
        seat = observation.get('machine')
        if (not text(seat) or not SEAT.fullmatch(seat)
                or not isinstance(row, dict) or not key(row.get('account'), 'ac')
                or observation.get('pk') != row['account'] + '.' + seat
                or row.get('state') not in ('running', 'idle', 'signed-out')
                or not text(row.get('cli')) or type(row.get('at')) is not int
                or not 0 <= row['at'] <= 253402300799000):
            raise ValueError('invalid-observation')
    reports = []
    for host in selected:
        binding = by_machine.get(host['mc'])
        if binding is None or binding['row'] != 'live' or binding.get('merged_into'):
            raise ValueError('invalid-machine-details')
        # Authored and canonical labels can differ following native name resolution.
        # Only their stable key can join these views.
        seats = seat_set(binding['seats'])
        if seats != seat_set(host.get('uuids')):
            raise ValueError('invalid-machine-seats')
        detail = details.get(host['mc'])
        # These are separate reads; a rename or retirement invalidates the
        # selected live/name binding even when the stable key still resolves.
        if (not isinstance(detail, dict) or detail.get('complete') is not True
                or detail.get('mc') != host['mc']
                or detail.get('label') != host['label'] or detail.get('row') != 'live'):
            raise ValueError('invalid-machine-details')
        # Both native views derive these UUIDs from the same machine-facts binding.
        # A changed binding cannot join an earlier seat to a later account observation.
        detail_seats = [row.get('uuid') for row in rows(detail, 'facts')]
        if seat_set(detail_seats) != seats:
            raise ValueError('invalid-machine-seats')
        links = {}
        retained = {}

        def add(account, source):
            row = by_account.get(account)
            if row is None:
                raise ValueError('dangling-account-reference')
            if typed_lifecycle(row) == 'live' and row['provider'] == 'openai':
                links.setdefault(account, set()).add(source)

        earlier_preferences = {row['ac'] for row in preferences
                               if row['mc'] == host['mc'] and row['row'] == 'live'}
        if not earlier_preferences.issubset(by_account):
            raise ValueError('dangling-account-reference')
        for observation in observations:
            if observation.get('machine') in seats:
                row = observation.get('row')
                account = row['account']
                add(account, 'retained-observation')
                if account in links:
                    retained.setdefault(account, set()).add((
                        observation['machine'], row['at'], row['cli'], row['state']))
        for row in rows(detail, 'places'):
            add(row.get('ac'), 'placement')
        # The later machine detail contains all its current preferences. The earlier
        # list corroborates a link only while that same account is still preferred.
        for row in rows(detail, 'prefers'):
            account = row.get('ac')
            add(account, 'machine-preference')
            if account in earlier_preferences:
                add(account, 'preference')
        reports.append({
            'machine_key': host['mc'], 'label': host['label'],
            'machine_facts_seat_count': len(seats),
            'openai_account_count': len(links),
            'accounts': [{'account_key': account, 'label': by_account[account].get('label'),
                          **({} if account in status_disputes else {'status': by_account[account].get('status')}),
                          'sources': sorted(sources),
                          'observations': [
                              {'seat': seat, 'cli': cli, 'state': state, 'observed_at_ms': at}
                              for seat, at, cli, state in sorted(retained.get(account, []))
                          ]} for account, sources in sorted(links.items())],
        })
    if len(reports) != 1:
        status = unmatched_machine_status(machine, selected, by_machine)
    elif reports[0]['openai_account_count'] < expected:
        status = 'insufficient-accounts'
    else:
        status = 'matched'
    return {'status': status, 'proven': status == 'matched', 'machines': reports,
            'unbound_seat_count': unbound_seat_count}


def _published_binding(reader, receipts):
    """Read only the published local marker and its signed seats, never machine names."""
    view, receipt = reader(PUBLISHED_MACHINES)
    receipts.append(receipt)
    if receipt.get('error') or not isinstance(view, dict) or view.get('file') != 'lab/machines':
        return None
    if view.get('state') in FILE_STATES and isinstance(view.get('complete'), bool):
        status = {'state': view['state'], 'complete': view['complete']}
        if 'why' in view and view['why'] in FILE_REASONS:
            status['why'] = view['why']
        receipt['published_file'] = status
    if (view.get('state') != 'live' or view.get('why', '') is not None or view.get('complete') is not True
            or any(view.get(k) is not None for k in ('kept', 'refused', 'problem'))):
        return None
    columns = view.get('columns')
    if (not isinstance(columns, list) or columns.count('here') != 1 or columns.count('seats') != 1
            or not isinstance(view.get('rows'), list)):
        return None
    binding = None
    seen = set()
    for row in view['rows']:
        if not isinstance(row, dict):
            return None
        mc, cells, values = row.get('pk'), row.get('cells'), row.get('values')
        if (not key(mc, 'mc') or mc in seen or type(row.get('live')) is not bool
                or not isinstance(cells, dict) or not isinstance(values, dict)):
            return None
        seen.add(mc)
        # Native omits an unset computed cell from both maps. A refused cell has no typed value.
        if 'here' not in cells and 'here' not in values:
            continue
        if (binding is not None or not row['live'] or cells.get('here') != 'yes' or values.get('here') != 'yes'
                or cells.get('row', 'live') != 'live' or values.get('row', 'live') != 'live'
                or cells.get('merged_into') or values.get('merged_into')):
            return None
        seats = cells.get('seats')
        if not isinstance(seats, str) or seats != values.get('seats'):
            return None
        try:
            binding = (mc, seat_set(seats.split(', ')))
        except ValueError:
            return None
    return binding


def probe_here(expected, reader=query, **selectors):
    """Bracket the typed association search with this Mac's published machine binding."""
    return probe(None, expected, reader, **selectors)


class CurrentMachineProbe:
    """Keep the verified machine key and signed seats private to one operation."""

    def __init__(self):
        self._binding = None

    def read(self, expected, reader=query, **selectors):
        return probe(None, expected, reader, current_machine=self, **selectors)


def machine_detail(host, reader, receipts):
    value, receipt = reader(['aterm', 'db', 'alab', 'machines', host['mc']])
    receipts.append(receipt)
    if receipt.get('error'):
        raise ValueError('machine-query-failed')
    return value


def redact_named_login_metadata(report):
    selector = report['requested_machine']
    if isinstance(selector, str) and '@' in selector:
        report['requested_machine'] = None
    for host in report['machines']:
        # Only repeat a label the caller supplied. A stable key suffices otherwise.
        if host['label'] != report['requested_machine']:
            del host['label']
            report['requested_machine'] = host['machine_key']
        for account in host['accounts']:
            account.pop('label', None)
            for observation in account.get('observations', ()):
                if observation['cli'] not in OBSERVATION_CLIS:
                    del observation['cli']


def probe(machine, expected, reader=query, *, expected_accounts=(), expected_logins=(), show_logins=False,
          show_mail_routes=False, show_alias_declarations=False, current_machine=None, setup_accounts=()):
    report = {'schema': 1, 'checked_at_utc': datetime.now(timezone.utc).isoformat(),
              'requested_machine': machine, 'expect_at_least': expected,
              'status': 'unavailable', 'proven': False, 'machines': [],
              'commands': [], 'errors': [], 'limits': LIMITS}
    if show_alias_declarations and show_mail_routes:
        raise ValueError('select alias declarations or mail routes')
    show_logins = show_logins or show_mail_routes or show_alias_declarations
    if show_logins and (expected_logins or expected_accounts):
        raise ValueError('discover logins or select expected accounts')
    if setup_accounts:
        setup_accounts = tuple(setup_accounts)
        if (not show_logins or not all(key(account, 'ac') for account in setup_accounts)
                or len(set(setup_accounts)) != len(setup_accounts)):
            raise ValueError('setup accounts must be distinct stable ac- keys in a registered login lookup')
        report['setup_confirmed'] = False
    if expected_logins:
        if expected_accounts:
            raise ValueError('select accounts by stable key or by login')
        expected_logins = [login_address(value) for value in expected_logins]
        if len(set(expected_logins)) != len(expected_logins):
            raise ValueError('expected logins must be distinct')
        report['requested_login_count'] = len(expected_logins)
    if expected_accounts:
        if (not all(key(account, 'ac') for account in expected_accounts)
                or len(set(expected_accounts)) != len(expected_accounts)):
            raise ValueError('expected accounts must be distinct stable ac- keys')
        report['expected_account_keys'] = list(expected_accounts)
    if machine is not None:
        result = _probe_associations(report, machine, expected, reader, expected_accounts, expected_logins, show_logins,
                                     show_mail_routes=show_mail_routes, show_alias_declarations=show_alias_declarations,
                                     setup_accounts=setup_accounts)
        if expected_logins or show_logins:
            redact_named_login_metadata(result)
        return result

    report['requested_here'] = True
    receipts = []
    unconfirmed = dict(report, requested_machine=None, errors=[HERE_UNCONFIRMED], commands=receipts)
    binding = _published_binding(reader, receipts)
    if binding is None:
        return unconfirmed
    if current_machine is not None and current_machine._binding not in (None, binding):
        return dict(unconfirmed, errors=[HERE_CHANGED])
    machine, seats = binding
    report['requested_machine'] = machine
    report['commands'].extend(receipts)
    result = _probe_associations(report, machine, expected, reader, expected_accounts, expected_logins, show_logins,
                                here_seats=seats, show_mail_routes=show_mail_routes,
                                show_alias_declarations=show_alias_declarations, setup_accounts=setup_accounts)
    if _published_binding(reader, receipts) != binding or HERE_UNCONFIRMED in result['errors']:
        return unconfirmed
    if current_machine is not None and result['status'] in ('matched', 'insufficient-accounts'):
        current_machine._binding = binding
    result['commands'].append(receipts[-1])
    # Native labels and observation details can contain addresses. Here the stable
    # machine key identifies the report, and only explicit discovery includes logins.
    for entry in result['machines']:
        del entry['label']
        fields = ('account_key', 'sources', 'registered_login', 'mail_route', 'alias_declaration')
        entry['accounts'] = [{k: account[k] for k in fields if k in account} for account in entry['accounts']]
    return result


def _probe_associations(report, machine, expected, reader, expected_accounts, expected_logins, show_logins,
                       *, here_seats=None, show_mail_routes=False, show_alias_declarations=False, setup_accounts=()):
    queries = QUERIES.copy()
    if show_logins:
        # The native machine detail supplies current preferences and placements.
        del queries['observations']
    with ThreadPoolExecutor(max_workers=len(queries)) as executor:
        results = list(executor.map(reader, queries.values()))
    data = {}
    for name, (value, receipt) in zip(queries, results):
        report['commands'].append(receipt)
        data[name] = value
    if any(receipt.get('error') for _, receipt in results):
        report['errors'].append('metadata-query-failed')
        if data['accounts'] is None:
            # The local file can explain what this Mac can read. Machine associations
            # still require the typed collection, even if the file is current.
            value, receipt = reader(['aterm', 'file', 'status', 'ai/accounts', '--json'])
            report['commands'].append(receipt)
            if (not receipt.get('error') and isinstance(value, dict)
                    and value.get('file') == 'ai/accounts'
                    and value.get('state') in FILE_STATES
                    and 'why' in value and value['why'] in FILE_REASONS):
                status = {'state': value['state'], 'why': value['why']}
                if isinstance(value.get('complete'), bool):
                    status['complete'] = value['complete']
                report['local_account_file'] = status
        return report
    try:
        details = {}
        checking_logins = bool(expected_logins) or show_logins
        checking_mail = show_mail_routes or show_alias_declarations
        status_disputes = ()
        selected = selected_machines(machine, data, require_live_binding=checking_logins or here_seats is not None)
        if here_seats is not None and (len(selected) != 1 or selected[0]['mc'] != machine
                                       or seat_set(selected[0]['uuids']) != here_seats):
            raise ValueError(HERE_UNCONFIRMED)
        if checking_logins and len(selected) != 1:
            bindings, _ = machine_bindings(data)
            report['status'] = unmatched_machine_status(machine, selected, bindings)
            return report
        if checking_logins:
            initial_addresses = read_addresses(reader, report['commands'])
            alias_view, status_disputes = confirm_login_tables(
                rows(data['accounts'], 'rows', complete=False), initial_addresses, reader, report['commands'],
                setup_accounts, include_aliases=show_alias_declarations, checking_mail=checking_mail)
        for host in selected:
            details[host['mc']] = machine_detail(host, reader, report['commands'])
        result = analyze(machine, expected, data, details, current_only=show_logins, setup_accounts=setup_accounts,
                         checking_logins=checking_logins, status_disputes=status_disputes)
        if len(result['machines']) == 1:
            initial_rows = result['machines'][0]['accounts']
            initial_accounts = [row['account_key'] for row in initial_rows]
            if checking_logins:
                initial_logins = resolve_logins(expected_logins, data['accounts']['rows'], initial_addresses,
                                                discover=initial_accounts if show_logins else None)
                initial_cells, address_details = login_lifecycle_evidence(
                    data['accounts']['rows'], initial_addresses, initial_accounts, initial_logins,
                    setup_accounts, reader, report['commands'])
                if checking_mail:
                    mail_fields, witnesses = read_login_mail_fields(initial_logins, initial_addresses, reader,
                                                                    report['commands'], address_details, alias_view)
            # A retained place can outlive its account. Bracket the machine detail
            # with typed account reads before asserting membership or absence by key.
            later, receipt = reader(QUERIES['accounts'])
            report['commands'].append(receipt)
            if receipt.get('error'):
                raise ValueError('account-query-failed')
            if checking_logins:
                final_addresses = read_addresses(reader, report['commands'])
                alias_view, status_disputes = confirm_login_tables(
                    rows(later, 'rows', complete=False), final_addresses, reader, report['commands'],
                    setup_accounts, include_aliases=show_alias_declarations, checking_mail=checking_mail)
                final_cells, address_details = login_lifecycle_evidence(
                    later['rows'], final_addresses, initial_accounts, initial_logins,
                    setup_accounts, reader, report['commands'])
                if checking_mail:
                    final_mail = read_login_mail_fields(initial_logins, final_addresses, reader,
                                                        report['commands'], address_details, alias_view)
                    if final_mail != (mail_fields, witnesses):
                        raise ValueError('alias-declarations-changed' if show_alias_declarations else 'mail-routes-changed')
                if initial_cells != final_cells:
                    raise ValueError('login-bindings-changed')
                # Recheck the selected machine and its current assignments after the
                # final login reads. Explicit lookups also count retained observations.
                checks = ['machines', 'preferences', 'canonical']
                if expected_logins:
                    checks.insert(0, 'observations')
                for name in checks:
                    data[name], receipt = reader(QUERIES[name])
                    report['commands'].append(receipt)
                    if receipt.get('error'):
                        raise ValueError('machine-query-failed')
                current = selected_machines(machine, data, require_live_binding=True)
                if (len(current) != 1 or current[0]['mc'] != selected[0]['mc']
                        or current[0]['label'] != selected[0]['label']
                        or seat_set(current[0]['uuids']) != seat_set(selected[0]['uuids'])):
                    raise ValueError('machine-association-changed')
                details[current[0]['mc']] = machine_detail(current[0], reader, report['commands'])
            result = analyze(machine, expected, dict(data, accounts=later), details,
                             current_only=show_logins, setup_accounts=setup_accounts, checking_logins=checking_logins,
                             status_disputes=status_disputes)
            final_rows = result['machines'][0]['accounts']
            associated = [row['account_key'] for row in final_rows]
            if (initial_accounts != associated
                    or (checking_logins and [row['sources'] for row in initial_rows]
                        != [row['sources'] for row in final_rows])):
                raise ValueError('account-associations-changed')
            if setup_accounts:
                before, after = ({row['pk']: typed_cell(row, 'status') for row in scan['rows']
                                  if row['pk'] in setup_accounts} for scan in (data['accounts'], later))
                if not set(setup_accounts).issubset(associated) or before != after:
                    raise ValueError('setup-accounts-unconfirmed')
                result['setup_confirmed'] = result['proven']
            if checking_logins:
                final_logins = resolve_logins(expected_logins, later['rows'], final_addresses,
                                              discover=associated if show_logins else None)
                if initial_logins != final_logins:
                    raise ValueError('login-bindings-changed')
                if show_logins:
                    # Account labels, machine detail names, canonical identities, and the
                    # mailbox can all contain other addresses. Export only the verified join.
                    by_account = {account: login for login, _, account in final_logins}
                    result['machines'][0]['accounts'] = [
                        {'account_key': row['account_key'], 'registered_login': by_account[row['account_key']],
                         'sources': row['sources']} for row in final_rows]
                    if checking_mail:
                        for row in result['machines'][0]['accounts']:
                            row.update(mail_fields[row['account_key']])
                else:
                    expected_accounts = [account for _, _, account in final_logins]
                    result['resolved_login_account_keys'] = expected_accounts
            if expected_accounts:
                missing = [account for account in expected_accounts if account not in associated]
                result['missing_account_keys'] = missing
                if missing:
                    result.update(status='missing-accounts', proven=False)
        report.update(result)
    except (ValueError, TypeError, KeyError) as error:
        # Errors are constant classifications, never provider output or raw rows.
        report['errors'].append(str(error) if isinstance(error, ValueError) else 'invalid-metadata')
    return report


def association_for(report, account_key):
    """Project one native slot key from a completed probe(machine, 1) for display."""
    unavailable = {'status': 'unverified', 'reason': 'registry-unavailable'}
    if not key(account_key, 'ac'):
        return {'status': 'unverified', 'reason': 'unconfirmed-account-key'}
    if not isinstance(report, dict):
        return unavailable
    status = report.get('status')
    if status in ('machine-not-found', 'ambiguous-machine'):
        return {'status': 'unverified', 'reason': status}
    if status == 'unavailable':
        if report.get('errors') in (['registry-view-unconfirmed'], [HERE_UNCONFIRMED]):
            return {'status': 'unverified', 'reason': report['errors'][0]}
        commands = report.get('commands')
        if isinstance(commands, list) and any(
            isinstance(receipt, dict) and receipt.get('error') == 'command-failed'
            and receipt.get('reason') == 'commons-membership-not-ready' for receipt in commands
        ):
            return {'status': 'unverified', 'reason': 'commons-membership-not-ready'}
        return unavailable
    machines = report.get('machines')
    if (status not in ('matched', 'insufficient-accounts') or report.get('errors') != []
            or report.get('proven') is not (status == 'matched')
            or not isinstance(machines, list) or len(machines) != 1 or not isinstance(machines[0], dict)):
        return unavailable
    machine = machines[0]
    accounts = machine.get('accounts')
    count = machine.get('openai_account_count')
    label = None if report.get('requested_here') is True else machine.get('label')
    selector_matches = matches_machine(report.get('requested_machine'), machine.get('machine_key'), label)
    if (not key(machine.get('machine_key'), 'mc') or not selector_matches
            or not isinstance(accounts, list) or type(count) is not int or count != len(accounts)
            or (status == 'matched') != bool(accounts)):
        return unavailable
    kinds = ('machine-preference', 'placement', 'preference', 'retained-observation')
    associations = {}
    for row in accounts:
        if not isinstance(row, dict):
            return unavailable
        account, sources = row.get('account_key'), row.get('sources')
        if (not key(account, 'ac') or account in associations or not isinstance(sources, list)
                or not sources or any(source not in kinds for source in sources)):
            return unavailable
        associations[account] = sources
    if account_key not in associations:
        return {'status': 'missing'}
    return {'status': 'associated', 'sources': sorted(set(associations[account_key]))}


def write_new_report(path, encoded):
    staged = tempfile.NamedTemporaryFile(mode='w', encoding='utf-8', prefix='.registry-report-',
                                         dir=path.parent, delete=False)
    try:
        with staged:
            staged.write(encoded)
            staged.flush()
            os.fsync(staged.fileno())
        os.link(staged.name, path)
    finally:
        try:
            os.unlink(staged.name)
        except OSError:
            print('Cannot remove the private temporary evidence file.', file=sys.stderr)


def finish_failed_stdout(stream):
    # Python retries buffered stdout at shutdown and can replace exit 2 with
    # 120. Keep the caller's descriptor usable until then.
    try:
        stream.flush()
    except (OSError, ValueError):
        try:
            stream.close()
        except (OSError, ValueError):
            pass


def association_exit_code(report):
    if report['proven']:
        return 0
    return 2 if report['status'] in ('ambiguous-machine', 'unavailable') else 1


def address_check(report, *, declarations):
    name = 'alias-declarations' if declarations else 'mail-routes'
    if not report['proven']:
        return {'name': name, 'status': report['status'], 'exit_code': association_exit_code(report)}

    accounts = report['machines'][0]['accounts']
    if declarations:
        complete = all(row['alias_declaration'] in ('declared', 'absent') for row in accounts)
    else:
        complete = all(row['mail_route']['status'] == 'reported' for row in accounts)
    return {'name': name, 'status': 'complete' if complete else 'unresolved', 'exit_code': 0 if complete else 1}


def main(argv=None):
    parser = argparse.ArgumentParser(
        description=__doc__,
        epilog="For alias and mail-route lookups, check.status and check.exit_code give the requested "
               "lookup's result. The top-level proven flag confirms the machine's account associations.")
    location = parser.add_mutually_exclusive_group(required=True)
    location.add_argument('--machine', help='exact registry label or stable mc- key')
    location.add_argument('--here', action='store_true', help="use this Mac's published Registry machine binding")
    parser.add_argument('--expect-at-least', type=int,
                        help='minimum association count (defaults to the number requested, 1 for --show-logins, '
                             '--show-alias-declarations or --show-mail-routes, otherwise 2)')
    selectors = parser.add_mutually_exclusive_group()
    selectors.add_argument('--expect-account', action='append', default=[], metavar='AC_KEY',
                           help='require this stable OpenAI account key on the selected machine; repeat for each account')
    selectors.add_argument('--expect-login', action='append', default=[], metavar='EMAIL',
                           help='resolve this registered OpenAI login address and require its account on the machine')
    selectors.add_argument('--show-logins', action='store_true',
                           help='show registered sign-in addresses currently preferred or placed on this machine')
    selectors.add_argument('--show-alias-declarations', action='store_true',
                           help='show whether each assigned login has a saved alias link in the Registry')
    selectors.add_argument('--show-mail-routes', action='store_true',
                           help='show each assigned login, its alias declaration status, and the receiving mailbox key '
                                'when confirmed')
    parser.add_argument('--output', type=Path, help='create a new private JSON file on a volume supporting hard links')
    args = parser.parse_args(argv)
    discovery = args.show_logins or args.show_mail_routes or args.show_alias_declarations
    expected = args.expect_at_least if args.expect_at_least is not None else (
        len(args.expect_account or args.expect_login) or (1 if discovery else 2))
    if expected < 1 or (not args.here and (not text(args.machine) or len(args.machine) > 128)):
        parser.error('a nonempty machine label/key and positive count are required')
    try:
        options = dict(expected_accounts=args.expect_account, expected_logins=args.expect_login,
                       show_logins=args.show_logins)
        if args.show_mail_routes:
            options['show_mail_routes'] = True
        if args.show_alias_declarations:
            options['show_alias_declarations'] = True
        report = probe_here(expected, **options) if args.here else probe(args.machine, expected, **options)
    except ValueError as error:
        parser.error(str(error))
    check = None
    if args.show_alias_declarations or args.show_mail_routes:
        check = address_check(report, declarations=args.show_alias_declarations)
        report = {'check': check, **report}
    try:
        encoded = json.dumps(report, indent=2) + '\n'
    except (TypeError, ValueError):
        print('Cannot encode the evidence report as JSON.', file=sys.stderr)
        return 2
    if args.output:
        try:
            write_new_report(args.output, encoded)
        except OSError:
            print('Cannot create the requested new evidence file.', file=sys.stderr)
            return 2
    try:
        if sys.stdout is None:
            raise OSError('stdout unavailable')
        print(encoded, end='', flush=True)
    except (OSError, ValueError):
        if sys.stdout is not None:
            atexit.register(finish_failed_stdout, sys.stdout)
        message = 'Cannot write the evidence report to stdout.'
        if args.output:
            message += ' The requested private file was saved.'
        print(message, file=sys.stderr)
        return 2
    return check['exit_code'] if check is not None else association_exit_code(report)


if __name__ == '__main__':
    sys.exit(main())
