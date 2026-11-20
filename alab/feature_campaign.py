#!/usr/bin/env python3
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
"""Collect declared feature evidence for an actual missing-bundle gate."""
import argparse
from contextlib import contextmanager
import hashlib
import json
import os
import re
import secrets
from pathlib import Path
import subprocess
import sys
import tempfile

if __name__ == '__main__':
    sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import command_guard
from cli_status import run_operation
from feature_evidence import FILE_HASH, check, combine, progress, read_record, require_valid, snapshot, stage_bundle_logs
from feature_gate import admission, bundle_path
from feature_recipes.manifest import (NATIVE_PYTHON, PYTHON, RUST, STAGE_FORMATS, WIRE,
                                      decode as decode_recipes, selected_test_names)
from feature_recipes.request import NoFeatureRequest, common_dir, context, request
from qualification import worktree_clean
from upstream_intake import git, git_environment

RECIPES = HERE / 'feature_recipes'
INNER_PYTHON_FIXTURES = {'wire': 'wire_controls.py', 'model-wire': 'model_wire_controls.py',
                         'product-wire': 'product_wire_controls.py', 'skill-wire': 'skill_wire_controls.py',
                         'native-ui-wire': 'native_ui_controls.py', 'execution-wire': 'execution_wire_controls.py',
                         'unit-comparison': 'unit_comparison_controls.py'}
STAGES = tuple(STAGE_FORMATS)


class ExecutionFailure(Exception):
    def __init__(self, status, message):
        super().__init__(message)
        self.status = status if status > 0 else 128 - status


def digest(path):
    return FILE_HASH['file_sha256'](path)


def exclusive_json(path, data):
    with path.open('x') as stream:
        json.dump(data, stream, indent=2)
        stream.write('\n')


def read_recipe_snapshot(path):
    _metadata, raw = FILE_HASH['file_bytes_snapshot'](path)
    recipes = decode_recipes(raw, path.name)
    return recipes, hashlib.sha256(raw).hexdigest()


def recipe_digest(manifest_sha, recipes, tooling, *, tracked=None):
    def input_path(path):
        if tracked is not None and path not in tracked:
            raise ValueError('recipe input is absent from the original producer commit: ' + str(path))
        return path

    manifest = input_path(recipes / 'scenarios.json')
    programs = [tooling / 'feature_campaign.py', tooling / 'tests/test_proof_build.py', tooling / 'lib.sh',
                tooling / 'package.sh', tooling / 'python_test_results.py', tooling / 'cli_status.py',
                *sorted(p for p in recipes.iterdir() if p != manifest and p.is_file())]
    identities = {}
    for program in programs:
        input_path(program)
        if program.suffix == '.inputs':
            _, inputs = FILE_HASH['file_bytes_snapshot'](program)
            identities[program.name] = hashlib.sha256(inputs).hexdigest()
            for helper in inputs.decode('utf-8').splitlines():
                identities[helper] = digest(input_path(recipes.parent / helper))
        else:
            identities[program.name] = digest(program)
    identities[manifest.name] = manifest_sha
    return hashlib.sha256(json.dumps(identities, sort_keys=True).encode()).hexdigest()

def row_selection(groups, row, scenario, feature_id, number):
    if not isinstance(row['groups'], list) or any(not isinstance(n, str) for n in row['groups']):
        raise ValueError('invalid recipe groups for ' + row['scenario'])
    if not row['groups']:
        return None
    selected = {}
    for name in row['groups']:
        group = groups[name]
        stage = group['stage']
        if stage not in STAGES:
            raise ValueError('invalid test selection for ' + name)
        names = selected_test_names(group, name, scenario)
        if names:
            selected.setdefault(stage, set()).update(names)
    coverage = row.get('coverage')
    if not isinstance(coverage, str) or not coverage.strip():
        raise ValueError(f"{feature_id} scenario {number} needs a coverage explanation: {row['scenario']}")
    return coverage, selected


def plan(inventory, recipes, scenario_numbers=(), *, skip_unimplemented=False):
    groups = recipes['groups']
    results = {}
    required = {}
    for feature in inventory['features']:
        identity = feature['id']
        if identity not in recipes['features']:
            if skip_unimplemented:
                continue
            raise ValueError('unsupported feature scope: ' + identity)
        rows = recipes['features'][identity]
        if [row['scenario'] for row in rows] != [s['description'] for s in feature['scenarios']]:
            raise ValueError('scenario contract changed for ' + identity)
        if any(type(n) is not int or not 1 <= n <= len(rows) for n in scenario_numbers):
            raise ValueError(f'scenario number outside 1..{len(rows)} for {identity}')
        for number, row in enumerate(rows, 1):
            included = not scenario_numbers or number in scenario_numbers
            selection = row_selection(groups, row, feature['scenarios'][number - 1], identity, number)
            if selection is None:
                if not included or skip_unimplemented:
                    continue
                raise ValueError('scenario has no recipe: ' + row['scenario'])
            coverage, selected = selection
            if included:
                if not selected:
                    raise ValueError('scenario has no applicable recipe: ' + row['scenario'])
                results[identity, row['scenario']] = (coverage, selected)
                for stage, names in selected.items():
                    required.setdefault(stage, set()).update(names)
    return results, {stage: sorted(required[stage]) for stage in STAGES if stage in required}


def remaining(observed, rows, stages):
    # A name emitted by two stages cannot establish which execution an imported
    # observation witnessed. Such a stage must execute its own current recipe.
    sources = {}
    for stage, names in stages.items():
        fmt = STAGE_FORMATS[stage]
        for name in names:
            sources.setdefault((fmt, name), set()).add(stage)
    needed = {}
    selected = set()
    for feature in observed['features']:
        for row in feature['scenarios']:
            key = feature['id'], row['description']
            _, by_stage = rows[key]
            witnessed = {(c.get('format', 'libtest'), n) for c in row['checks'] for n in c['tests']}
            needed[key] = {}
            for stage, names in by_stage.items():
                fmt = STAGE_FORMATS[stage]
                missing = {n for n in names if (fmt, n) not in witnessed
                           or len(sources[fmt, n]) != 1}
                if missing:
                    needed[key][stage] = missing
                    selected.add(stage)
    return needed, {stage: names for stage, names in stages.items() if stage in selected}


@contextmanager
def lock(path):
    try:
        import fcntl
    except ImportError as error:
        raise ValueError('native feature execution requires a kernel worktree lock') from error
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open('a+b') as stream:
        try:
            fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            raise ExecutionFailure(75, 'another feature campaign owns ' + str(path)) from error
        yield


def execution_workspace(repo, source, selected, common):
    workspace = selected.resolve(strict=True) if selected is not None else common / 'alab-feature-workspace'
    if workspace.exists():
        current = git(workspace, 'rev-parse', 'HEAD')
        if (common_dir(workspace) != common or git(workspace, 'rev-parse', '--abbrev-ref', 'HEAD') != 'HEAD'
                or not worktree_clean(workspace, current)):
            raise ValueError('execution worktree must be clean, detached and in this repository')
        if current == source:
            return workspace
        if selected is not None and git(workspace, 'merge-base', current, source) != current:
            raise ValueError('supplied worktree can only fast-forward to the recorded source')
        arguments = ['-C', str(workspace), 'switch', '--detach', source]
    else:
        arguments = ['-C', str(repo), 'worktree', 'add', '--detach', str(workspace), source]
    # Worktree progress accompanies other diagnostics. The CLI emits one JSON result on stdout.
    subprocess.run(['git', *arguments], env=git_environment(), stdout=sys.stderr, check=True)
    return workspace


def run_stage(argv, workspace, log):
    # The custodian holds the workspace lock on collector loss. The outer recipe
    # execs atpkg-publish, which owns the separate native controller cohort.
    redirect = 'cd -- "$1" || exit 1; log=$2; shift 2; set -C; exec "$@" >"$log" 2>&1'
    return command_guard.run(['/bin/bash', '-c', redirect, 'feature-stage',
                              str(workspace), str(log), *argv], cooperative_controller=True)


def guard_path(stage, folder):
    # Native fixture commands write their guard receipt inside the protected profile.
    return folder / ('artifacts/guard.json' if stage in NATIVE_PYTHON else 'guard.json')


def stage_observation(stage, tests, folder, argv, sha, command_status, *, recipes=None):
    tooling = HERE if recipes is None else recipes.parent
    recipes = RECIPES if recipes is None else recipes
    path = folder / ('combined.log' if stage in RUST else 'artifacts/tests.json')
    if stage not in RUST:
        guard = read_record(guard_path(stage, folder))
        if (guard.get('kind') != 'guardian-command-completion' or guard.get('completed') is not True
                or type(guard.get('normalized_exit_status')) is not int
                or guard['normalized_exit_status'] != command_status
                or not isinstance(guard.get('argv'), list)):
            raise ValueError('Python stage disagrees with its native guard: ' + str(folder))
        if stage in NATIVE_PYTHON:
            selectors = argv[6:] if stage in WIRE else []
            runner = tooling / 'python_test_results.py' if selectors else Path(argv[2]) / 'alab/python_test_results.py'
            expected = ['-I', '-B', str(runner),
                        '--start-directory', str(recipes), '--pattern', INNER_PYTHON_FIXTURES[stage],
                        '--record', str(Path(argv[4]) / 'tests.json')]
            if selectors:
                if selectors != tests:
                    raise ValueError('wire command differs from its scenario selection')
                expected.extend(['--tests', *tests])
            if (guard['argv'][1:] != expected or not isinstance(guard['argv'][0], str)
                    or not Path(guard['argv'][0]).is_absolute()):
                raise ValueError('Python guard did not run the selected fixture for ' + stage)
        else:
            layer = 'package-profile' if stage == 'package-inputs' else 'python-profile'
            expected = ['/bin/bash', argv[0], layer, *argv[2:6]]
            if guard['argv'][:len(expected)] != expected:
                raise ValueError('Python guard did not run this source and stage under the protected profile')
        argv = guard['argv']
    observation = dict(source=sha, status=command_status, command=argv,
                       format=STAGE_FORMATS[stage],
                       tests=tests, log=str(path), log_sha256=digest(path),
                       reason='Candidate test executions for ' + stage + '.')
    check(observation, sha)
    return observation


def execute(stage, tests, run_dir, workspace, state, stable, *, scoped_wire=False, recorded_recipes=None):
    source = state['source']
    stage_dir = run_dir / stage
    if recorded_recipes is None:
        stage_dir.mkdir(exist_ok=True)
    recipes = RECIPES if recorded_recipes is None else recorded_recipes
    # The candidate already guards suite count and test non-emptiness. Exact required
    # identities are checked against the raw libtest/Python result before any reuse.
    select_rust = stage in RUST and stage not in ('transport', 'websocket', 'autopilot-integration')
    filters = tests if select_rust or (scoped_wire and stage in WIRE) else []
    for attempt in (1, 2, 3):
        folder = stage_dir / str(attempt)
        argv = [str(recipes / 'run.sh'), 'outer', str(workspace), source, str(folder / 'artifacts'), stage, *filters]
        before = dict(source=source, command=argv)
        status_file = folder / 'result.json'
        if recorded_recipes is not None:
            if folder.resolve(strict=True) != folder or (folder / 'artifacts').resolve(strict=True) != folder / 'artifacts':
                raise ValueError('indirect recorded stage directory: ' + str(folder))
            if read_record(folder / 'request.json') != before:
                raise ValueError('recorded stage request changed: ' + str(folder))
        if recorded_recipes is not None or folder.exists():
            if not status_file.exists():
                raise ValueError('interrupted stage has no completion status: ' + str(folder))
            result = read_record(status_file)
            if (result['source'] != source or result['command'] != argv
                    or result['output_sha256'] != digest(folder / 'combined.log')):
                raise ValueError('stage receipt or output changed: ' + str(folder))
        else:
            folder.mkdir()
            (folder / 'artifacts').mkdir()
            exclusive_json(folder / 'request.json', before)
            stable()
            status = run_stage(argv, workspace, folder / 'combined.log')
            result = dict(**before, status=status, output_sha256=digest(folder / 'combined.log'))
            try:
                stable()
            except (ValueError, OSError, subprocess.CalledProcessError, ExecutionFailure) as error:
                exclusive_json(status_file, {**result, 'invalidated': str(error)})
                raise
            if result['status'] == 0:
                test_log = folder / ('combined.log' if stage in RUST else 'artifacts/tests.json')
                result['test_log_sha256'] = digest(test_log) if test_log.is_file() else None
                if stage not in RUST:
                    guard = guard_path(stage, folder)
                    result['guard_sha256'] = digest(guard) if guard.is_file() else None
            exclusive_json(status_file, result)
        if 'invalidated' in result:
            raise ValueError('stage source changed during execution: ' + str(folder))
        status = result['status']
        if type(status) is not int:
            raise ValueError('invalid stage status: ' + str(folder))
        if status == 0:
            if stage not in RUST and result['guard_sha256'] != digest(guard_path(stage, folder)):
                raise ValueError('native completion guard changed: ' + str(folder))
            obs = stage_observation(stage, tests, folder, argv, source, status, recipes=recorded_recipes)
            if result['test_log_sha256'] != obs['log_sha256']:
                raise ValueError('named test log changed: ' + str(folder))
            return obs
        refusal = folder / 'cache-refused.status'
        if not (status == 75 and refusal.is_file() and refusal.read_text() == '75\n'):
            raise ExecutionFailure(status, f'{stage} exited {status}: {folder}')
    raise ExecutionFailure(75, f'{stage} could not reserve its cache after three recorded attempts: {stage_dir}')


def append_checks(scenario, checks, by_stage, reason):
    scenario['checks'].extend({**checks[stage], 'tests': sorted(names), 'reason': reason}
                              for stage, names in by_stage.items())


def part_name(index, scenario_numbers):
    name = f'part-{index}'
    if scenario_numbers:
        selection = hashlib.sha256(json.dumps(scenario_numbers).encode()).hexdigest()
        name += '-scenarios-' + selection
    return name


def link_artifact(source, target):
    try:
        os.link(source, target)
    except FileExistsError:
        if target.is_symlink() or digest(target) != digest(source):
            raise ValueError('an existing feature artifact differs: ' + str(target))


def publication_logs(destination):
    logs = destination / 'logs'
    if destination.is_symlink() or logs.is_symlink():
        raise ValueError('feature publication directory must not be a symlink')
    return logs


def publish(repo, state, record, expected, stable=lambda: None):
    destination = bundle_path(repo, state['source'])
    destination.parent.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(dir=destination.parent) as tmp:
        staged = Path(tmp) / 'bundle'
        exported = stage_bundle_logs(record, expected, staged, retained_logs=publication_logs(destination))
        logs = publication_logs(destination)
        destination.mkdir(exist_ok=True)
        logs.mkdir(exist_ok=True)
        for source_file in sorted((staged / 'logs').iterdir()):
            if digest(source_file) != source_file.stem:
                raise ValueError('staged feature log hash changed: ' + str(source_file))
            link_artifact(source_file, logs / source_file.name)
        # Check both retained and newly linked captures in place before admitting a record.
        require_valid(exported, expected, destination)
        record_file = staged / 'record.json'
        exclusive_json(record_file, exported)
        stable()
        link_artifact(record_file, destination / 'record.json')
    result = admission(repo, state['base'], state['source'], state['upstream'])
    if result['errors']:
        raise ValueError('; '.join(result['errors']))
    stable()
    return result


def campaign(repo, gate_record, workspace_option, run, *, new_run=False, parts=(), feature_id=None,
             scenario_numbers=(), for_sync=False, collect_ready=False):
    state, actual, path = request(repo, gate_record)
    if for_sync and git(repo, 'merge-base', state['base'], state['source']) != state['base']:
        raise ExecutionFailure(1, 'the sync candidate does not contain its recorded private main')

    def check_current_main():
        if for_sync and git(repo, 'rev-parse', 'refs/remotes/origin/main') != state['base']:
            raise ExecutionFailure(1, 'private main advanced during sync feature collection')

    check_current_main()
    expected = snapshot(repo, state['source'], state['upstream'], state['base'])
    manifest = RECIPES / 'scenarios.json'
    recipes, manifest_sha = read_recipe_snapshot(manifest)

    recipe_sha = recipe_digest(manifest_sha, RECIPES, HERE)
    request_info = dict(gate_record=str(path), gate_record_sha256=digest(path), context=state,
                        affected_features=actual['affected_features'],
                        recipe_sha256=recipe_sha, admitted=not actual['errors'])
    # A concurrent collector can finish this exact request before the sync takes its lock.
    if for_sync and request_info['admitted']:
        return request_info
    feature_recipes = recipes['features']
    unsupported = sorted(f for f in actual['affected_features']
                         if f not in feature_recipes or any(row['groups'] == [] for row in feature_recipes[f]))
    if scenario_numbers and feature_id is None:
        raise ValueError('scenario selection requires a single feature')
    if len(set(scenario_numbers)) != len(scenario_numbers):
        raise ValueError('repeated scenario number')
    scenario_numbers = sorted(scenario_numbers)
    if feature_id is not None:
        if feature_id not in actual['affected_features']:
            raise ValueError('feature is outside the recorded gate scope: ' + feature_id)
        if parts:
            raise ValueError('partial collection requires its own executions')
    elif unsupported:
        if run and not (for_sync or collect_ready):
            raise ValueError('unsupported feature scope: ' + ', '.join(unsupported))
        collect_ready = True
    partial = feature_id is not None or collect_ready
    records, part_inputs = [], {}
    for part in parts:
        part = part.resolve(strict=True)
        if str(part) in part_inputs:
            raise ValueError('repeated feature part: ' + str(part))
        _metadata, raw = FILE_HASH['file_bytes_snapshot'](part)
        records.append(json.loads(raw))
        part_inputs[str(part)] = hashlib.sha256(raw).hexdigest()
    observed = combine(records, expected)
    selected = observed
    if feature_id is not None:
        # Keep every reviewed scenario in the transferable part. Only the selected
        # feature receives checks, and the ordinary gate still requires the full scope.
        selected = {**observed, 'features': [f for f in observed['features'] if f['id'] == feature_id]}
        request_info['selected_feature'] = feature_id
    # Check the complete ordered contract before narrowing the execution view.
    rows, stages = plan(selected, recipes, scenario_numbers, skip_unimplemented=collect_ready)
    if collect_ready:
        pending = {}
        for feature in selected['features']:
            missing = [n for n, row in enumerate(feature['scenarios'], 1)
                       if (feature['id'], row['description']) not in rows]
            if missing:
                pending[feature['id']] = missing
        request_info['scenarios_without_recipes'] = pending
    if scenario_numbers:
        request_info['selected_scenarios'] = scenario_numbers
    if scenario_numbers or collect_ready:
        selected = {'features': [{**feature, 'scenarios': [
            row for row in feature['scenarios'] if (feature['id'], row['description']) in rows
        ]} for feature in selected['features']]}
    needed, stages = remaining(selected, rows, stages)
    request_info.update(unsupported_features=unsupported, stages=list(stages), parts=part_inputs)
    if not run or (request_info['admitted'] and not partial) or (collect_ready and not rows):
        return request_info
    common = common_dir(repo)
    root = common / 'alab-feature-campaign'
    with lock(root / 'workspace.lock'):
        workspace = execution_workspace(repo, state['source'], workspace_option, common) if stages else repo

        def stable():
            check_current_main()
            if (context(repo, state['base']) != state or recipe_digest(digest(manifest), RECIPES, HERE) != recipe_sha
                    or not all(worktree_clean(tree, state['source']) for tree in {workspace, repo})):
                raise ValueError('source, frontier, recipe or worktree changed during the campaign')

        stable()
        if not stages and not partial:
            return {**request_info, 'admitted': True,
                    'admission': publish(repo, state, observed, expected, stable)}
        out = root / state['source'] / state['base'] / recipe_sha
        if feature_id is not None:
            # The inventory permits arbitrary nonblank IDs. Use the index in the
            # immutable full snapshot so their spelling never becomes a path.
            index = next(i for i, f in enumerate(expected['features']) if f['id'] == feature_id)
            out = out / part_name(index, scenario_numbers)
        elif collect_ready:
            out = out / 'part-ready'
        out.mkdir(parents=True, exist_ok=True)
        if new_run:
            out = Path(tempfile.mkdtemp(prefix='fresh-', dir=out))
        inputs = dict(snapshot=expected, recipe_sha256=recipe_sha, workspace=str(workspace))
        if feature_id is not None:
            inputs['selected_feature'] = feature_id
        if scenario_numbers:
            inputs['selected_scenarios'] = scenario_numbers
        if (out / 'inputs.json').exists():
            if read_record(out / 'inputs.json') != inputs:
                raise ValueError('existing campaign belongs to a different workspace or scope')
        else:
            exclusive_json(out / 'inputs.json', inputs)
        checks = {}
        for stage, tests in stages.items():
            stable()
            print('Executing or validating', stage, 'in', out, flush=True, file=sys.stderr)
            checks[stage] = execute(stage, tests, out, workspace, state, stable, scoped_wire=bool(scenario_numbers))
        for feature in selected['features']:
            for scenario in feature['scenarios']:
                key = feature['id'], scenario['description']
                reason, _ = rows[key]
                append_checks(scenario, checks, needed[key], reason)
        stable()
        if partial:
            # Independently validate the logs and full frozen scope before publishing
            # the part alongside its receipts. This path is outside gate admission.
            combine([observed], expected)
            target = out / 'part.json'
            with tempfile.TemporaryDirectory(dir=out) as tmp:
                staged = Path(tmp) / 'part.json'
                exclusive_json(staged, observed)
                stable()
                link_artifact(staged, target)
            return {**request_info, 'part': str(target), 'run': str(out)}
        result = publish(repo, state, observed, expected, stable)
        return {**request_info, 'admitted': True, 'admission': result, 'run': str(out)}


def recover_part(repo, record, from_run, producer, output):
    """Export only complete scenario rows from a durably stopped single-feature run."""
    common = common_dir(repo)
    root = common / 'alab-feature-campaign'
    out = from_run.absolute()
    if out != out.resolve(strict=True) or not out.is_relative_to(root):
        raise ValueError('expected an original native campaign directory')
    producer = producer.resolve(strict=True)
    tooling = producer / 'alab'
    recipes = tooling / 'feature_recipes'
    target = output.parent.resolve(strict=True) / output.name
    if target.is_relative_to(common) or any(target.is_relative_to(p) for p in (repo, producer)):
        raise ValueError('write the recovered part outside the campaign and source worktrees')

    with lock(root / 'workspace.lock'):
        state, _, gate = request(repo, record)
        source = state['source']
        expected = snapshot(repo, source, state['upstream'], state['base'])
        original = read_record(out / 'inputs.json')
        feature = original.get('selected_feature')
        numbers = original.get('selected_scenarios', [])
        workspace = Path(original['workspace'])
        producer_head = git(producer, 'rev-parse', 'HEAD^{commit}')
        if not worktree_clean(producer, producer_head):
            raise ValueError('original producer worktree is not clean')
        if producer_head != source:
            try:
                published = git(repo, 'merge-base', producer_head, 'refs/remotes/origin/main')
            except subprocess.CalledProcessError:
                published = None
            if published != producer_head:
                raise ValueError("original producer is outside the candidate repository's published private history")
        tracked = {producer / name for name in git(producer, 'ls-tree', '-r', '--name-only', '-z',
                                                   producer_head, '--', 'alab').split('\0') if name}
        recipe, manifest_sha = read_recipe_snapshot(recipes / 'scenarios.json')
        fingerprint = recipe_digest(manifest_sha, recipes, tooling, tracked=tracked)
        if (not isinstance(numbers, list) or not all(type(n) is int for n in numbers)
                or numbers != sorted(set(numbers))):
            raise ValueError('invalid original scenario selection')
        features = [f['id'] for f in expected['features']]
        if feature not in features:
            raise ValueError('expected one affected feature from the original campaign')
        index = features.index(feature)
        inputs = dict(snapshot=expected, recipe_sha256=fingerprint, workspace=str(workspace), selected_feature=feature)
        if numbers:
            inputs['selected_scenarios'] = numbers
        scope = root / source / state['base'] / fingerprint / part_name(index, numbers)
        if original != inputs or not (out == scope or (out.parent == scope and re.fullmatch(r'fresh-[a-z0-9_]{8}', out.name))):
            raise ValueError('stopped run differs from the native source, base, recipe or scope')
        if not workspace.is_absolute() or workspace != workspace.resolve(strict=True) or common_dir(workspace) != common:
            raise ValueError('invalid original candidate workspace')
        if target.is_relative_to(workspace):
            raise ValueError('write the recovered part outside the original candidate workspace')
        if (out / 'part.json').exists():
            raise ValueError('completed part already exists; use the original part')

        def stable():
            if (request(repo, gate)[0] != state or not worktree_clean(producer, producer_head)
                    or not worktree_clean(workspace, source)
                    or recipe_digest(digest(recipes / 'scenarios.json'), recipes, tooling, tracked=tracked) != fingerprint
                    or read_record(out / 'inputs.json') != inputs):
                raise ValueError('source, producer or recorded inputs changed during recovery')

        stable()
        observed = combine([], expected)
        selected = observed['features'][index]
        view = {'features': [selected]}
        rows, stages = plan(view, recipe, numbers)
        candidate_recipe, _ = read_recipe_snapshot(repo / 'alab/feature_recipes/scenarios.json')
        candidate_rows = candidate_recipe['features'].get(feature, [])
        checks = {}
        for stage, tests in stages.items():
            try:
                checks[stage] = execute(stage, tests, out, workspace, state, stable,
                                        scoped_wire=bool(numbers), recorded_recipes=recipes)
            except ExecutionFailure as error:
                if not 0 < error.status < 128:
                    raise ValueError('stage stopped by signal without usable normal completion') from error
                stopped_status = error.status
                break
        else:
            raise ValueError('no durably stopped stage in this run')
        stopped = stage
        if any((out / later).exists() for later in stages if later not in checks and later != stopped):
            raise ValueError('found stage output beyond the first failed stage')

        recovered = []
        unresolved = []
        for number, scenario in enumerate(selected['scenarios'], 1):
            key = feature, scenario['description']
            if key not in rows:
                continue
            reason, needed = rows[key]
            if (needed.keys() <= checks.keys() and number <= len(candidate_rows)
                    and candidate_rows[number - 1]['scenario'] == scenario['description']):
                candidate = row_selection(candidate_recipe['groups'], candidate_rows[number - 1],
                                          scenario, feature, number)
                if candidate is not None and candidate[1] == needed:
                    append_checks(scenario, checks, needed, reason)
                    recovered.append(number)
                    continue
            unresolved.append(number)
        report = progress([observed], expected)
        if report['covered'] != len(recovered):
            raise ValueError('recovered rows did not satisfy the source-declared checks')
        stable()
        if recovered:
            with tempfile.TemporaryDirectory(dir=target.parent) as tmp:
                staged = Path(tmp) / 'part.json'
                exclusive_json(staged, observed)
                link_artifact(staged, target)
        return dict(context=state, gate_record=str(gate), gate_record_sha256=digest(gate),
                    recipe_sha256=fingerprint, checked_producer=producer_head, run=str(out), selected_feature=feature,
                    stopped_stage=stopped, stopped_status=stopped_status, recovered_scenarios=recovered,
                    unresolved_scenarios=unresolved, part=str(target) if recovered else None)


def sync_check(repo):
    """Run the native gate, satisfy its own missing-bundle request, and recheck once."""
    source = git(repo, 'rev-parse', 'HEAD^{commit}')
    parent = common_dir(repo) / 'alab-qualification' / source
    parent.mkdir(parents=True, exist_ok=True)
    # The recorder exclusively creates this directory. Other runs on the same
    # source may finish concurrently, so never infer our record from timestamps.
    directory = parent / ('run-' + secrets.token_hex(16))
    command = [sys.executable, '-I', '-B', str(repo / 'alab/qualification.py')]
    status = subprocess.run([*command, '--directory', str(directory)], cwd=repo).returncode
    if status != 1:
        return status if status >= 0 else 128 - status
    record = directory / 'record.json'
    if not record.is_file():
        return 1
    try:
        result = campaign(repo, record, None, True, for_sync=True)
    except NoFeatureRequest:
        return 1
    except ExecutionFailure as error:
        print('feature campaign:', error, file=sys.stderr)
        # A feature test has no certified candidate/pristine attribution.
        return error.status if error.status == 75 or error.status > 128 else 1
    if pending := result.get('scenarios_without_recipes'):
        print('feature campaign: scenarios awaiting recipes:', json.dumps(pending, sort_keys=True),
              file=sys.stderr)
        if 'part' in result:
            print('feature campaign: verified runnable scenarios:', result['part'], file=sys.stderr)
        return 1
    if not result['admitted'] or context(repo, result['context']['base']) != result['context']:
        return 1
    # Only the complete native gate can qualify the source, including after a
    # concurrent collector has finished the same request.
    status = subprocess.run(command, cwd=repo).returncode
    if status == 0 and git(repo, 'rev-parse', 'HEAD^{commit}') != source:
        raise ValueError('candidate changed before feature qualification finished')
    return status if status >= 0 else 128 - status


def arguments():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=['request', 'run', 'collect-part', 'collect-ready', 'recover-part', 'sync'])
    parser.add_argument('--feature', help='one affected feature to execute for collect-part')
    parser.add_argument('--scenario', type=int, action='append', default=[],
                        help='one-based scenario number within that feature (repeatable)')
    parser.add_argument('--repo', type=Path, default=HERE.parent, help='worktree where the native check ran')
    parser.add_argument('--record', type=Path, help='native record.json (default: newest for HEAD)')
    parser.add_argument('--workspace', type=Path, help='dedicated clean detached worktree to fast-forward, or use the managed checkout')
    parser.add_argument('--part', action='append', type=Path, default=[],
                        help='independently verified partial for the identical frozen scope (repeatable)')
    parser.add_argument('--new-run', action='store_true', help='start fresh, retaining all prior attempts')
    parser.add_argument('--from-run', type=Path, help='original stopped single-feature campaign directory')
    parser.add_argument('--producer', type=Path, help='clean original collector checkout for recover-part')
    parser.add_argument('--output', type=Path, help='separate recovered part file')
    args = parser.parse_args()
    recovery = (args.from_run, args.producer, args.output)
    if args.action == 'recover-part':
        if not all(recovery) or args.feature or args.scenario or args.part or args.new_run or args.workspace:
            parser.error('recover-part requires --from-run, --producer and --output, and takes its scope from the run')
    elif any(recovery):
        parser.error('--from-run, --producer and --output apply only to recover-part')
    if args.action == 'sync' and (args.record or args.workspace or args.part):
        parser.error('sync uses its own native record and managed workspace')
    if (args.action == 'collect-part') != (args.feature is not None):
        parser.error('collect-part requires --feature, which applies only to collect-part')
    if args.scenario and args.action != 'collect-part':
        parser.error('--scenario requires collect-part --feature')
    if args.action in ('collect-part', 'collect-ready') and args.part:
        parser.error('--part imports observations into a complete campaign')
    if args.new_run and args.action not in ('run', 'collect-part', 'collect-ready'):
        parser.error('--new-run requires run, collect-part or collect-ready')
    return args


def run(args):
    os.umask(0o077)
    if args.action == 'sync':
        return sync_check(args.repo.resolve(strict=True))
    try:
        if args.action == 'recover-part':
            result = recover_part(args.repo.resolve(strict=True), args.record, args.from_run, args.producer, args.output)
            print(json.dumps(result, indent=2))
            return result['stopped_status']
        result = campaign(args.repo.resolve(strict=True), args.record, args.workspace,
                          args.action in ('run', 'collect-part', 'collect-ready'), new_run=args.new_run,
                          collect_ready=args.action == 'collect-ready',
                          parts=args.part, feature_id=args.feature, scenario_numbers=args.scenario)
        print(json.dumps(result, indent=2))
    except ExecutionFailure as error:
        print('feature campaign:', error, file=sys.stderr)
        return error.status
    return 0


if __name__ == '__main__':
    args = arguments()
    sys.exit(run_operation(lambda: run(args), 'feature campaign: '))
