#!/usr/bin/env python3
"""Unified dependency-graph solver and CI lane router.

This module is the single source of truth that maps repository file paths to
affected Cargo crates, pnpm packages, local check.py targets, and CI lanes.
Both GitHub Actions workflows and `scripts/check.py --affected` read from it so
path ownership and dependency edges are never duplicated.

Everything that cannot be routed safely falls back to the whole matrix: an
unrecognised path, a workflow or classifier edit, a manual or scheduled run, a
release tag, a diff that cannot be read, and an empty change set.
"""
import argparse
from collections import defaultdict
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tomllib

REPO = Path(__file__).resolve().parents[1]
CLASSIFIER = 'scripts/ci-changes.py'
CLASSIFIER_TESTS = 'scripts/test_ci_changes.py'

# Build-input documents whose bytes are embedded or bundled by the plugin.
HOST_DOCS = ('docs/companion-rules.md',)
PACKAGE_DOCS = ('README.md', 'docs/plugin-development.md')

# Direct-touch Presubmit triggers for Large toolchain contract tests.
MUTATOR_CONTRACT_PATHS = (
    'crates/mutator/src/golang.rs',
    'crates/mutator/src/java.rs',
    'crates/mutator/src/clojure.rs',
    'crates/mutator/src/python.rs',
    'crates/mutator/src/typescript.rs',
    'crates/mutator/src/lua.rs',
    'crates/mutator/src/engine.rs',
    'crates/mutator/src/rust_suite.rs',
    'crates/quality-core/src/process.rs',
    'crates/quality-core/src/process_windows.rs',
)
CRAPPER_CONTRACT_PATHS = (
    'crates/quality-core/src/runner.rs',
    'crates/quality-core/src/runner_go.rs',
    'crates/quality-core/src/runners/',
    'crates/crapper/src/',
    'crates/uml-viewer/tests/crap_languages.rs',
    'crates/uml-viewer/tests/crap_runner_smoke.rs',
    'crates/uml-viewer/tests/mutate_lua.rs',
    'crates/uml-viewer/tests/toolchain_contract.rs',
    'crates/crapper/tests/process_tree.rs',
)
UPGRADE_E2E_PATHS = (
    'scripts/package.mjs',
    'scripts/ci-upgrade-package.mjs',
    'scripts/release.mjs',
    'dsh-plugin.json',
    'packages/e2e/upgrade.spec.mjs',
    'packages/e2e/package.spec.mjs',
)
TOOLCHAIN_SETUP_SCRIPTS = (
    'scripts/setup-windows-lua.ps1',
    'scripts/setup-windows-maven-clojure.ps1',
)

OUTPUT_KEYS = (
    'docs_only',
    'full',
    'fast',
    'typescript',
    'rust_hermetic',
    'panel_e2e',
    'upgrade_e2e',
    'mutate_refresh',
    'toolchain_crapper',
    'toolchain_mutator',
    'release',
)

FULL_EVENTS = ('workflow_dispatch', 'schedule')
RELEASE_TAG = re.compile(r'^refs/tags/v')
COMMIT = re.compile(r'^[0-9a-f]{7,64}$')
NODE_SCRIPT = re.compile(r'^scripts/[^/]+[.]mjs$')
GUARD_SCRIPT = re.compile(r'^scripts/[^/]+[.](py|ps1)$')


class Unresolved(Exception):
    """The changed files cannot be read safely, so the whole matrix runs."""


def load_workspace_graph(repository=REPO):
    """Read live Cargo + pnpm workspace manifests and build the reverse-dep DAG.

    Returns (ws_crates, pnpm_pkgs, rdeps_edges, dev_rdeps_edges) where:
    - rdeps_edges[src] contains nodes invalidated whenever src changes.
    - dev_rdeps_edges[src] contains nodes whose tests are invalidated by src
      (non-transitive to library consumers of dst).
    """
    root = Path(repository)
    ws_crates = []
    raw_deps = {}
    crates_dir = root / 'crates'
    if crates_dir.is_dir():
        for manifest in sorted(crates_dir.glob('*/Cargo.toml')):
            data = tomllib.loads(manifest.read_text(encoding='utf-8'))
            name = data.get('package', {}).get('name') or manifest.parent.name
            ws_crates.append(name)
            normal = set(data.get('dependencies', {}).keys()) | set(
                data.get('build-dependencies', {}).keys()
            )
            dev = set(data.get('dev-dependencies', {}).keys())
            for target_cfg in data.get('target', {}).values():
                if isinstance(target_cfg, dict):
                    normal |= set(target_cfg.get('dependencies', {}).keys())
                    normal |= set(target_cfg.get('build-dependencies', {}).keys())
                    dev |= set(target_cfg.get('dev-dependencies', {}).keys())
            raw_deps[name] = (normal, dev)

    ws_set = set(ws_crates)
    rdeps_edges = defaultdict(set)
    dev_rdeps_edges = defaultdict(set)
    for dst_crate, (normal, dev) in raw_deps.items():
        dst = f'cargo:{dst_crate}'
        for dep in normal:
            if dep in ws_set:
                rdeps_edges[f'cargo:{dep}'].add(dst)
        for dep in dev:
            if dep in ws_set:
                dev_rdeps_edges[f'cargo:{dep}'].add(dst)

    pnpm_pkgs = {}
    packages_dir = root / 'packages'
    if packages_dir.is_dir():
        for pkg_json in sorted(packages_dir.glob('*/package.json')):
            data = json.loads(pkg_json.read_text(encoding='utf-8'))
            pnpm_pkgs[pkg_json.parent.name] = data.get('name', pkg_json.parent.name)
        name_to_node = {pkg_name: f'pnpm:{dir_name}'
                        for dir_name, pkg_name in pnpm_pkgs.items()}
        for pkg_json in sorted(packages_dir.glob('*/package.json')):
            dst = f'pnpm:{pkg_json.parent.name}'
            data = json.loads(pkg_json.read_text(encoding='utf-8'))
            deps = {**data.get('dependencies', {}), **data.get('devDependencies', {})}
            for dep_name in deps:
                if dep_name in name_to_node:
                    rdeps_edges[name_to_node[dep_name]].add(dst)

    # 1. Workspace-wide configs
    for crate in ws_crates:
        rdeps_edges['config:rust_ws'].add(f'cargo:{crate}')
    for pkg_dir in pnpm_pkgs:
        rdeps_edges['config:pnpm_ws'].add(f'pnpm:{pkg_dir}')

    # 2. Coupled docs, plugin assets, and embedded assets
    rdeps_edges['doc:companion_rules'].update({'pnpm:contracts', 'pnpm:host', 'guard:fast'})
    rdeps_edges['doc:package_bundle'].update({'script:node_packaging', 'pnpm:e2e'})
    rdeps_edges['asset:plugin_bundle'].update(
        {'pnpm:client', 'pnpm:host', 'pnpm:e2e', 'script:node_packaging'}
    )
    rdeps_edges['asset:embedded_mutator'].add('cargo:mutator')

    # 3. Granular fixtures and quality/EDN inputs
    for consumer in ('cargo:quality-core', 'cargo:crapper', 'cargo:uml-viewer'):
        dev_rdeps_edges['fixture:crap'].add(consumer)
    rdeps_edges['fixture:mutate'].add('cargo:mutator')
    dev_rdeps_edges['fixture:mutate'].add('cargo:uml-viewer')
    rdeps_edges['fixture:dry'].add('cargo:dryer')
    for consumer in ('cargo:quality-core', 'cargo:uml-viewer'):
        dev_rdeps_edges['fixture:dry'].add(consumer)
    for consumer in ('cargo:ir', 'cargo:scene', 'cargo:uml-viewer'):
        dev_rdeps_edges['fixture:ir_scene'].add(consumer)
    dev_rdeps_edges['fixture:quality'].add('cargo:uml-viewer')
    for consumer in ('cargo:ir', 'cargo:crapper', 'cargo:uml-viewer'):
        dev_rdeps_edges['fixture:lang_roots'].add(consumer)
    dev_rdeps_edges['fixture:other'].add('cargo:uml-viewer')

    rdeps_edges['config:uml_quality'].update({'cargo:ir', 'cargo:quality-dispatch'})
    dev_rdeps_edges['config:uml_quality'].add('cargo:uml-viewer')

    # .metrics/** & examples/uml-viewer.policy.edn feed real-snapshot.mjs (#153)
    for consumer in ('cargo:ir', 'cargo:scene', 'cargo:frame',
                     'cargo:viewer-session', 'cargo:uml-viewer'):
        dev_rdeps_edges['data:real_snapshot_edn'].add(consumer)
    for consumer in ('cargo:viewer-session', 'cargo:uml-viewer'):
        dev_rdeps_edges['data:examples_edn'].add(consumer)

    # 4. Role-aware panel-e2e target edges:
    # panel-e2e drives resident, ir, scene, and configured-mode quality
    # (quality-dispatch + quality-core + edn), NEVER standardTools
    # (crapper/mutator/dryer). Pruning those link-only edges avoids false 19m runs.
    for node in ('pnpm:e2e', 'pnpm:host', 'pnpm:client', 'script:node_packaging',
                 'doc:package_bundle', 'asset:plugin_bundle', 'data:real_snapshot_edn'):
        rdeps_edges[node].add('target:panel_e2e')
    for role_crate in ('cargo:viewer-session', 'cargo:frame', 'cargo:scene', 'cargo:ir'):
        rdeps_edges[role_crate].add('target:panel_e2e')
    rdeps_edges['cargo:quality-core'].add('panel_role:quality_configured')
    rdeps_edges['panel_role:quality_configured'].add('target:panel_e2e')

    rdeps_edges['repo:git_config'].add('guard:fast')
    rdeps_edges['script:guard_and_refresh'].add('guard:fast')

    return ws_crates, pnpm_pkgs, rdeps_edges, dev_rdeps_edges


def classify_node(path):
    """Map one repository-relative path to its node in the unified dependency graph."""
    normalized = path.replace('\\', '/')
    if normalized in (CLASSIFIER, CLASSIFIER_TESTS) or normalized.startswith('.github/'):
        return 'ci:control_plane'
    if normalized in HOST_DOCS:
        return 'doc:companion_rules'
    if normalized in PACKAGE_DOCS:
        return 'doc:package_bundle'
    if normalized in ('Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml',
                      'clippy.toml', '.cargo/config.toml', '.config/nextest.toml'):
        return 'config:rust_ws'
    if normalized in ('package.json', 'pnpm-lock.yaml', 'pnpm-workspace.yaml',
                      'tsconfig.base.json'):
        return 'config:pnpm_ws'
    if normalized in ('dsh-plugin.json', 'cordis.patch.yml', 'icon.svg') or normalized.startswith(
        ('fonts/', 'locale/')
    ):
        return 'asset:plugin_bundle'
    if normalized.startswith('embedded-assets/'):
        return 'asset:embedded_mutator'
    if normalized == 'uml-quality.json':
        return 'config:uml_quality'
    if normalized in ('.gitattributes', '.gitignore'):
        return 'repo:git_config'
    if normalized.startswith('crates/'):
        parts = normalized.split('/')
        if len(parts) >= 2:
            return f'cargo:{parts[1]}'
    if normalized.startswith('packages/'):
        parts = normalized.split('/')
        if len(parts) >= 2:
            return f'pnpm:{parts[1]}'
    if normalized.startswith('fixtures/crap/'):
        return 'fixture:crap'
    if normalized.startswith('fixtures/mutate/') or normalized == 'fixtures/data.txt':
        return 'fixture:mutate'
    if normalized.startswith('fixtures/dry/'):
        return 'fixture:dry'
    if normalized.startswith(('fixtures/ir-bytes/', 'fixtures/jvm-goldens/',
                              'fixtures/route-goldens/', 'fixtures/shop/',
                              'fixtures/state/')):
        return 'fixture:ir_scene'
    if normalized.startswith('fixtures/quality/'):
        return 'fixture:quality'
    if normalized.startswith('fixtures/'):
        return 'fixture:other'
    if normalized.startswith(('rust-fixture/', 'python-fixture/',
                              'typescript-fixture/', 'lua-fixture/')):
        return 'fixture:lang_roots'
    if normalized.startswith('.metrics/') or normalized == 'examples/uml-viewer.policy.edn':
        return 'data:real_snapshot_edn'
    if normalized.startswith('examples/') or normalized.endswith('.edn'):
        return 'data:examples_edn'
    if NODE_SCRIPT.match(normalized):
        return 'script:node_packaging'
    if GUARD_SCRIPT.match(normalized):
        return 'script:guard_and_refresh'
    if normalized.endswith('.md') or normalized.startswith(('docs/', 'acceptance-evidence/')):
        return 'doc:pure'
    return 'unknown'


def compute_closure(seeds, rdeps_edges, dev_rdeps_edges):
    """Return (lib_reached, test_reached) reverse-dependency closures."""
    lib_reached = set(seeds)
    queue = list(seeds)
    while queue:
        current = queue.pop()
        for nxt in rdeps_edges.get(current, ()):
            if nxt not in lib_reached:
                lib_reached.add(nxt)
                queue.append(nxt)
    test_reached = set(lib_reached)
    for node in list(lib_reached):
        test_reached.update(dev_rdeps_edges.get(node, ()))
    return lib_reached, test_reached


def _matches_any(path, rules):
    return any(path == rule or (rule.endswith('/') and path.startswith(rule))
               for rule in rules)


def full_matrix():
    flags = {key: True for key in OUTPUT_KEYS}
    flags['docs_only'] = False
    return flags


def analyze_changes(files, repository=REPO, event='pull_request', ref=''):
    """Compute affected crates/packages and CI lane flags for a file list."""
    ws_crates, pnpm_pkgs, rdeps_edges, dev_rdeps_edges = load_workspace_graph(repository)
    valid_nodes = {f'cargo:{c}' for c in ws_crates} | {f'pnpm:{p}' for p in pnpm_pkgs}

    normalized = [f.replace('\\', '/') for f in files]
    seeds = set()
    test_only_crates = set()
    uml_bin_touched = False
    has_unknown_or_ci = False

    direct_mutator_contract = False
    direct_crapper_contract = False
    direct_upgrade_e2e = False

    for path in normalized:
        node = classify_node(path)
        if node in ('ci:control_plane', 'unknown'):
            has_unknown_or_ci = True
            seeds.add(node)
        elif node.startswith(('cargo:', 'pnpm:')) and node not in valid_nodes:
            has_unknown_or_ci = True
            seeds.add('unknown')
        elif node.startswith('cargo:') and '/tests/' in path:
            test_only_crates.add(node)
        else:
            seeds.add(node)
            if node == 'cargo:uml-viewer':
                uml_bin_touched = True

        if (_matches_any(path, MUTATOR_CONTRACT_PATHS)
                or _matches_any(path, TOOLCHAIN_SETUP_SCRIPTS)
                or path.startswith('crates/mutator/tests/')
                or node in ('fixture:mutate', 'fixture:crap', 'fixture:lang_roots')):
            direct_mutator_contract = True
        if (_matches_any(path, CRAPPER_CONTRACT_PATHS)
                or _matches_any(path, TOOLCHAIN_SETUP_SCRIPTS)
                or node in ('fixture:crap', 'fixture:lang_roots')):
            direct_crapper_contract = True
        if (_matches_any(path, UPGRADE_E2E_PATHS)
                or node in ('config:pnpm_ws', 'asset:plugin_bundle', 'doc:package_bundle')):
            direct_upgrade_e2e = True

    lib_reached, test_reached = compute_closure(seeds, rdeps_edges, dev_rdeps_edges)
    test_reached.update(test_only_crates)

    docs_only = (seeds | test_only_crates) == {'doc:pure'} and not has_unknown_or_ci
    full = has_unknown_or_ci or not normalized
    post_submit_main = (event == 'push' and ref == 'refs/heads/main' and not docs_only)

    if full:
        lanes = full_matrix()
        affected_crates = sorted(ws_crates)
        affected_pnpm = sorted(pnpm_pkgs.keys())
    else:
        ts_triggers = {
            'pnpm:contracts', 'pnpm:client', 'pnpm:host', 'pnpm:e2e',
            'script:node_packaging', 'config:pnpm_ws', 'asset:plugin_bundle',
            'doc:companion_rules', 'doc:package_bundle',
        }
        release_triggers = {
            'cargo:uml-viewer', 'pnpm:contracts', 'pnpm:client', 'pnpm:host',
            'script:node_packaging', 'config:pnpm_ws', 'config:rust_ws',
            'asset:plugin_bundle', 'doc:companion_rules', 'doc:package_bundle',
        }
        mutate_refresh_triggers = (
            {f'cargo:{c}' for c in ws_crates}
            | {
                'pnpm:contracts', 'pnpm:client', 'pnpm:host',
                'config:rust_ws', 'config:pnpm_ws', 'config:uml_quality',
                'script:guard_and_refresh', 'fixture:crap', 'fixture:mutate',
            }
        )

        affected_crates = sorted(
            node.split(':', 1)[1] for node in test_reached if node.startswith('cargo:')
        )
        affected_pnpm = sorted(
            node.split(':', 1)[1] for node in (lib_reached | seeds) if node.startswith('pnpm:')
        )

        rust_hermetic = bool(affected_crates) or ('script:guard_and_refresh' in seeds)
        panel_e2e = (
            ('target:panel_e2e' in lib_reached)
            or ('cargo:quality-dispatch' in seeds)
            or uml_bin_touched
        )
        upgrade_e2e = (panel_e2e and direct_upgrade_e2e) or post_submit_main
        toolchain_crapper = direct_crapper_contract or post_submit_main
        toolchain_mutator = direct_mutator_contract or post_submit_main

        lanes = {
            'docs_only': docs_only,
            'full': False,
            'fast': not docs_only,
            'typescript': bool((lib_reached | seeds) & ts_triggers) or ('Cargo.toml' in normalized),
            'rust_hermetic': rust_hermetic or post_submit_main,
            'panel_e2e': panel_e2e or post_submit_main,
            'upgrade_e2e': upgrade_e2e,
            'mutate_refresh': bool((lib_reached | test_only_crates) & mutate_refresh_triggers)
                              or post_submit_main,
            'toolchain_crapper': toolchain_crapper,
            'toolchain_mutator': toolchain_mutator,
            'release': bool(lib_reached & release_triggers),
        }

    return {
        'files': normalized,
        'full': full,
        'docs_only': docs_only,
        'rust_crates': affected_crates,
        'pnpm_packages': affected_pnpm,
        'needs_release_build': full or ('uml-viewer' in affected_crates),
        'lanes': lanes,
    }


def listed_paths(path):
    text = Path(path).read_text(encoding='utf-8')
    return [line.strip() for line in text.splitlines() if line.strip()]


def diff_files(repository, base, head):
    """The changed paths between two commits, or Unresolved."""
    for label, value in (('base', base), ('head', head)):
        if not value or not COMMIT.fullmatch(value):
            raise Unresolved('the %s commit %r is not a commit id' % (label, value))
    try:
        completed = subprocess.run(
            ['git', '-C', str(repository), 'diff', '--name-only', '-z', '--no-renames',
             '%s...%s' % (base, head)],
            capture_output=True, text=True, check=True)
    except (OSError, subprocess.CalledProcessError) as error:
        detail = (getattr(error, 'stderr', '') or str(error)).strip()
        raise Unresolved('git diff %s...%s did not resolve: %s' % (base, head, detail))
    return [name for name in completed.stdout.split('\0') if name]


def parse(arguments):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('--event', default=None, help='the GitHub event name')
    parser.add_argument('--ref', default=None, help='the full ref, e.g. refs/tags/v1.2.3')
    parser.add_argument('--base', default=None, help='the base commit of the diff')
    parser.add_argument('--head', default=None, help='the head commit of the diff')
    parser.add_argument('--files-from', default=None,
                        help='read the changed paths from this file instead of a diff')
    parser.add_argument('--repository', default=str(REPO),
                        help='the work tree the diff runs in')
    parser.add_argument('--output', default=None,
                        help='write the flags here instead of GITHUB_OUTPUT')
    return parser.parse_args(arguments)


def resolve(options):
    """The command line wins over the workflow environment."""
    resolved = {}
    for name, variable in (('event', 'UML_CI_EVENT'), ('ref', 'UML_CI_REF'),
                           ('base', 'UML_CI_BASE_SHA'), ('head', 'UML_CI_HEAD_SHA')):
        chosen = getattr(options, name)
        resolved[name] = chosen if chosen is not None else os.environ.get(variable, '')
    return resolved


def changed(options):
    """The changed paths, or the reason the whole matrix has to run."""
    chosen = resolve(options)
    if chosen['event'] in FULL_EVENTS:
        return [], '%s runs the whole matrix' % chosen['event'], chosen
    if RELEASE_TAG.match(chosen['ref'] or ''):
        return [], '%s runs the whole matrix' % chosen['ref'], chosen
    try:
        files = (listed_paths(options.files_from) if options.files_from
                 else diff_files(options.repository, chosen['base'], chosen['head']))
    except Unresolved as error:
        return [], str(error), chosen
    if not files:
        return [], 'the change set is empty', chosen
    return files, None, chosen


def main(arguments=None):
    options = parse(arguments)
    files, reason, chosen = changed(options)
    if reason is None:
        graph_repo = REPO if (options.files_from and options.repository == str(REPO)) else options.repository
        # If testing against a synthetic git repo without workspace Cargo manifests, fall back to REPO.
        if not list((Path(graph_repo) / 'crates').glob('*/Cargo.toml')):
            graph_repo = REPO
        result = analyze_changes(files, repository=graph_repo,
                                 event=chosen['event'], ref=chosen['ref'])
        flags = result['lanes']
    else:
        flags = full_matrix()
    lanes = [key for key in OUTPUT_KEYS if flags[key]]
    print('ci-changes: files=%d' % len(files))
    print('ci-changes: reason=%s' % (reason or 'routed by the changed paths'))
    print('ci-changes: lanes=%s' % (' '.join(lanes) or '(none)'))
    lines = ['%s=%s' % (key, str(flags[key]).lower()) for key in OUTPUT_KEYS]
    destination = options.output or os.environ.get('GITHUB_OUTPUT')
    if destination:
        with open(destination, 'w' if options.output else 'a', encoding='utf-8') as handle:
            handle.write('\n'.join(lines) + '\n')
    else:
        print('\n'.join(lines))
    return 0


if __name__ == '__main__':
    sys.exit(main())
