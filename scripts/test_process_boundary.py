"""Production Rust code launches processes only through the named adapters.

Four review-found bugs came from launches outside an adapter (a `.cmd`
wrapper re-expanding %VAR%, a lost timeout, a bypassed Job Object).
clippy.toml forbids `std::process::Command::new`; this scan makes every
escape from that lint in production sources (`crates/*/src`) a deliberate,
listed one. A crate root may lift it for its own tests only
(`#![cfg_attr(test, allow(clippy::disallowed_methods))]`). The Herdr
companion and its OS adapter were deleted with the Python entry (#18); the
viewer launches no process at all.
"""
from pathlib import Path
import re
import unittest

REPO = Path(__file__).resolve().parents[1]
ALLOW = re.compile(r'allow\(\s*clippy::disallowed_methods\s*\)')
TEST_ONLY = re.compile(r'cfg_attr\(\s*test\s*,\s*allow\(\s*clippy::disallowed_methods\s*\)\s*\)')
# file -> why it may launch.
ADAPTERS = {
    'crates/quality-core/src/process.rs': 'the quality tools\' one process boundary (ADR 0012)',
    'crates/mutator/src/isolation.rs': 'the Linux namespace view launcher (PR_SET_PDEATHSIG, process group)',
    'crates/dryer/src/original/mod.rs': 'the ported pytest cases build real git repositories',
}
FIX = ('launch through crates/quality-core/src/process.rs, or add the file to ADAPTERS '
       'in scripts/test_process_boundary.py with the reason it is a boundary')


def violations(files):
    """`(relative path, text)` pairs -> one line per lint escape outside ADAPTERS."""
    found = []
    for path, text in files:
        if path in ADAPTERS:
            continue
        for number, line in enumerate(text.splitlines(), 1):
            if ALLOW.search(line) and not TEST_ONLY.search(line):
                found.append(f'{path}:{number}: {line.strip()}; {FIX}')
    return found


def production_sources():
    for path in sorted((REPO / 'crates').glob('*/src/**/*.rs')):
        yield path.relative_to(REPO).as_posix(), path.read_text(encoding='utf-8')


class ProcessBoundary(unittest.TestCase):
    def test_production_sources_escape_the_launch_lint_only_in_the_adapters(self):
        found = violations(production_sources())
        self.assertEqual([], found, '\n' + '\n'.join(found))

    def test_every_listed_adapter_still_exists(self):
        missing = [path for path in ADAPTERS if not (REPO / path).is_file()]
        self.assertEqual([], missing)

    def test_the_companion_adapter_is_gone(self):
        self.assertFalse((REPO / 'crates/companion').exists())
        self.assertEqual([], sorted(p.name for p in (REPO / 'scripts').glob('uml_*.py')))

    def test_the_scan_spares_test_only_lifts_and_names_every_other_escape(self):
        source = ('#![cfg_attr(test, allow(clippy::disallowed_methods))]\n'
                  'fn a() {}\n'
                  '#[allow(clippy::disallowed_methods)]\n'
                  'fn b() { std::process::Command::new("x"); }\n'
                  '    #![allow( clippy::disallowed_methods )]\n')
        self.assertEqual([f'crates/viewer/src/app.rs:3: #[allow(clippy::disallowed_methods)]; {FIX}',
                          f'crates/viewer/src/app.rs:5: #![allow( clippy::disallowed_methods )]; {FIX}'],
                         violations([('crates/viewer/src/app.rs', source)]))
        self.assertEqual([], violations([('crates/quality-core/src/process.rs', source)]))


if __name__ == '__main__':
    unittest.main()
