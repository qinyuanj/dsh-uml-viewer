"""Tracked goldens, EDN and Rust sources and the prompt rules hold no CR in the working tree.

A host with `core.autocrlf=true` checked golden fixtures out CRLF and the
failure surfaced later as a golden mismatch. This names the files instead.
Files `.gitattributes` marks `eol=crlf` (the `.cmd` entries) are exempt.
"""
from pathlib import Path
import subprocess
import unittest

REPO = Path(__file__).resolve().parents[1]
FIX = ('Fix: delete them and run `git checkout -- <file>...`; .gitattributes then '
       'writes them LF on every host.')


def git(*args, data=None):
    return subprocess.run(['git', '-C', str(REPO), *args], input=data,
                          capture_output=True, check=True).stdout


def guarded(path):
    # docs/companion-rules.md is the plugin prompt section, embedded verbatim.
    return (path.startswith('fixtures/') or path.endswith(('.edn', '.rs'))
            or path == 'docs/companion-rules.md')


def tracked():
    return [path for path in git('ls-files', '-z').decode('utf-8').split('\0')
            if path and guarded(path)]


def crlf_by_attribute(paths):
    """The paths whose `eol` attribute is `crlf`."""
    fields = git('check-attr', '-z', '--stdin', 'eol',
                 data='\0'.join(paths).encode('utf-8')).decode('utf-8').split('\0')
    return {fields[i] for i in range(0, len(fields) - 2, 3) if fields[i + 2] == 'crlf'}


def carriage_returns(paths):
    return [path for path in paths
            if (REPO / path).is_file() and b'\r' in (REPO / path).read_bytes()]


def report(found):
    shown = [f'  {path}' for path in found[:20]]
    if len(found) > 20:
        shown.append(f'  ... and {len(found) - 20} more')
    return '\n'.join([f'{len(found)} tracked file(s) contain CR in the working tree:',
                      *shown, FIX])


def work_tree():
    try:
        return git('rev-parse', '--is-inside-work-tree').strip() == b'true'
    except (OSError, subprocess.CalledProcessError):
        return False


@unittest.skipUnless(work_tree(), 'needs a git work tree')
class LineEndings(unittest.TestCase):
    def test_guarded_files_have_no_carriage_return(self):
        paths = tracked()
        exempt = crlf_by_attribute(paths)
        found = [path for path in carriage_returns(paths) if path not in exempt]
        if found:
            self.fail('\n' + report(found))


if __name__ == '__main__':
    unittest.main()
