"""The regeneration guard for `fixtures/mutate/help.txt`.

The binary reads the fixture through `include_str!` and the tests compare the
binary with that same fixture, so a regeneration that republished the pinned
original's `HELP` string would delete `--copy` (ADR 0016) and the bare `..`
rule from `uml-viewer mutate --help` with no test going red. The regeneration
script therefore refuses such a text; this pins that refusal, and the fixture it
protects.
"""
from pathlib import Path
import importlib.util
import tempfile
import unittest

REPO = Path(__file__).resolve().parents[1]
HELP_FIXTURE = REPO / 'fixtures' / 'mutate' / 'help.txt'
# The pinned original's HELP string, as far as this guard is concerned: an option
# table without --copy and without the bare .. rule. Shortened, because only the
# markers matter to the guard.
ORIGINAL_HELP = """Usage: mutator [options] [path-or-filter ...]

Options:
  -h, --help                    Print this help and exit.
  --root <path>                 Project root. Metrics are written here.
  --exclude <glob>              Leave out this project-relative path or glob.
  --scan                        List mutation sites. Do not run tests or
                                write metrics.
"""


def goldens():
    """The regeneration script as a module; importing it runs nothing."""
    spec = importlib.util.spec_from_file_location(
        'mutate_goldens', REPO / 'scripts' / 'mutate_goldens.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def scratch():
    return Path(tempfile.mkdtemp()) / 'help.txt'


class HelpFixture(unittest.TestCase):
    def test_the_fixture_states_this_ports_rules(self):
        # The help wraps its paragraphs; a rule may break across lines.
        text = ' '.join(HELP_FIXTURE.read_text(encoding='utf-8').split())
        self.assertEqual(goldens().port_help_gaps(text), [])
        self.assertIn('--copy <path>', text)
        self.assertIn('parent directory itself', text)


class PublishGuard(unittest.TestCase):
    def test_the_original_text_is_refused_and_names_both_rules(self):
        destination = scratch()
        with self.assertRaises(SystemExit) as raised:
            goldens().publish_help(destination, ORIGINAL_HELP)
        message = str(raised.exception)
        self.assertIn(str(destination), message)
        self.assertIn('--copy <path>', message)
        self.assertIn('parent directory itself', message)
        self.assertFalse(destination.exists(), 'the guard wrote the original text')

    def test_a_text_missing_one_rule_is_refused_for_that_rule_only(self):
        # Flattened, so a rule that wraps is one removable string.
        text = ' '.join(HELP_FIXTURE.read_text(encoding='utf-8').split())
        for missing in ('--copy <path>', 'parent directory itself'):
            destination = scratch()
            with self.assertRaises(SystemExit) as raised:
                goldens().publish_help(destination, text.replace(missing, ''))
            self.assertIn(f'{missing}: ', str(raised.exception))
            self.assertFalse(destination.exists(), missing)

    def test_this_ports_text_is_published(self):
        destination = scratch()
        text = HELP_FIXTURE.read_text(encoding='utf-8')
        goldens().publish_help(destination, text)
        self.assertEqual(destination.read_bytes(), text.encode('utf-8'))


if __name__ == '__main__':
    unittest.main()
