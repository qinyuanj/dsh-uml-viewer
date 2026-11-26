#!/usr/bin/env python3
"""Route one CI run to the lanes its changed files can break.

The tables below are the only place a path becomes a lane: both workflows read
the flags this command writes, so a path rule is never duplicated in YAML.

Everything that cannot be routed safely falls back to the whole matrix: a path
the tables do not recognise, a workflow or classifier edit, a manual or
scheduled run, a release tag, a diff that cannot be read, and an empty change
set.
"""
import argparse
import os
from pathlib import Path
import re
import subprocess
import sys

REPO = Path(__file__).resolve().parents[1]
CLASSIFIER = 'scripts/ci-changes.py'
CLASSIFIER_TESTS = 'scripts/test_ci_changes.py'

# The plugin build reads these bytes, so their content is a build input:
# packages/host/src/prompt.ts imports the prompt section as text, and the
# packaging contract tests copy the other two into their scratch archive.
HOST_DOCS = ('docs/companion-rules.md',)
PACKAGE_DOCS = ('README.md', 'docs/plugin-development.md')

# Paths owned by one domain. A domain is a build input several lanes share, so a
# new file joins the domain its siblings already live in.
DOMAINS = {
    'rust': ('crates/', 'embedded-assets/', 'Cargo.toml', 'Cargo.lock',
             'rust-toolchain.toml', 'clippy.toml'),
    'plugin': ('packages/', 'package.json', 'pnpm-lock.yaml',
               'pnpm-workspace.yaml', 'tsconfig.base.json'),
    'assets': ('dsh-plugin.json', 'cordis.patch.yml', 'icon.svg', 'fonts/', 'locale/'),
    'fixtures': ('fixtures/', 'rust-fixture/', 'python-fixture/',
                 'typescript-fixture/', 'lua-fixture/'),
    'quality_config': ('uml-quality.json',),
    'repo': ('.gitattributes', '.gitignore'),
    'ci': ('.github/',),
}
NODE_SCRIPT = re.compile(r'^scripts/[^/]+[.]mjs$')
GUARD_SCRIPT = re.compile(r'^scripts/[^/]+[.](py|ps1)$')

# The quality tools link into the viewer binary but the panel harness never
# drives them, so a change confined to them skips panel-e2e (#157, story 6).
QUALITY_TOOLS = ('crates/crapper/', 'crates/mutator/', 'crates/dryer/')

# The domains each lane can break. The fast gate is Tier 1 and covers every
# domain except a pure document.
LANES = {
    'typescript': ('plugin', 'assets', 'node_scripts', 'host_docs', 'package_docs'),
    'panel_e2e': ('rust', 'plugin', 'assets', 'node_scripts', 'host_docs',
                  'package_docs'),
    'crap': ('rust', 'rust_quality_tools', 'fixtures', 'quality_config'),
    'mutate': ('rust', 'rust_quality_tools', 'plugin', 'fixtures', 'quality_config',
               'guard_scripts'),
    'quality': ('rust', 'rust_quality_tools', 'fixtures', 'quality_config',
                'guard_scripts', 'edn'),
    'check': ('rust', 'rust_quality_tools', 'fixtures', 'quality_config',
              'guard_scripts', 'edn', 'host_docs', 'repo'),
    'release': ('rust', 'rust_quality_tools', 'plugin', 'assets', 'node_scripts',
                'package_docs', 'host_docs'),
}

OUTPUT_KEYS = ('docs_only', 'full', 'fast', 'typescript', 'panel_e2e',
               'crap', 'mutate', 'quality', 'check', 'release')

# Every domain a path can land in; a lane may only name one of these.
DOMAIN_NAMES = ('rust', 'rust_quality_tools', 'plugin', 'assets', 'fixtures',
                'quality_config', 'repo', 'ci', 'node_scripts', 'guard_scripts',
                'edn', 'host_docs', 'package_docs', 'docs', 'unknown')

FULL_EVENTS = ('workflow_dispatch', 'schedule')
RELEASE_TAG = re.compile(r'^refs/tags/v')
COMMIT = re.compile(r'^[0-9a-f]{7,64}$')


class Unresolved(Exception):
    """The changed files cannot be read safely, so the whole matrix runs."""


def owns(path, prefix):
    return path == prefix or (prefix.endswith('/') and path.startswith(prefix))


def domains_for(path):
    """The domains one changed path belongs to."""
    if path in HOST_DOCS:
        return {'host_docs'}
    if path in PACKAGE_DOCS:
        return {'package_docs'}
    if path in (CLASSIFIER, CLASSIFIER_TESTS):
        return {'ci'}
    if any(path.startswith(prefix) for prefix in QUALITY_TOOLS):
        return {'rust_quality_tools'}
    found = {name for name, prefixes in DOMAINS.items()
             if any(owns(path, prefix) for prefix in prefixes)}
    if found:
        return found
    if NODE_SCRIPT.match(path):
        return {'node_scripts'}
    if GUARD_SCRIPT.match(path):
        return {'guard_scripts'}
    if path.endswith('.edn') or path.startswith('examples/'):
        return {'edn'}
    if path.endswith('.md') or path.startswith(('docs/', 'acceptance-evidence/')):
        return {'docs'}
    return {'unknown'}


def routes(domains):
    """The lane flags for one change set."""
    flags = {name: bool(domains.intersection(owners)) for name, owners in LANES.items()}
    flags['fast'] = any(domain != 'docs' for domain in domains)
    flags['full'] = bool(domains.intersection({'ci', 'unknown'}))
    flags['docs_only'] = domains == {'docs'}
    if flags['full']:
        flags.update({name: True for name in LANES})
        flags['fast'] = True
    return flags


def full_matrix():
    flags = {key: True for key in OUTPUT_KEYS}
    flags['docs_only'] = False
    return flags


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
            ['git', '-C', repository, 'diff', '--name-only', '-z', '--no-renames',
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
        return [], '%s runs the whole matrix' % chosen['event']
    if RELEASE_TAG.match(chosen['ref'] or ''):
        return [], '%s runs the whole matrix' % chosen['ref']
    try:
        files = (listed_paths(options.files_from) if options.files_from
                 else diff_files(options.repository, chosen['base'], chosen['head']))
    except Unresolved as error:
        return [], str(error)
    if not files:
        return [], 'the change set is empty'
    return files, None


def main(arguments=None):
    options = parse(arguments)
    files, reason = changed(options)
    if reason is None:
        flags = routes(set().union(*(domains_for(path) for path in files)))
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
