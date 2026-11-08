"""Production scripts launch processes only through the launch helpers.

Four review-found bugs came from launches outside the platform module (a
`.cmd` wrapper re-expanding %VAR%, a lost timeout, a bypassed Job Object).
`uml_posix.run` (literal argv) and `uml_posix.spawn` (detached) are
the only places a production script may name a process-launch API; this
ast scan makes that a deterministic check. The test_* scripts are the
outside harness and are not scanned.
"""
import ast
from pathlib import Path
import unittest

SCRIPTS = Path(__file__).resolve().parent
PRODUCTION = ('uml_posix.py', 'uml_launch.py', 'uml_quality.py', 'uml_windows.py')
# (file, top-level function) allowed to name a launch API.
HELPERS = {('uml_posix.py', 'run'), ('uml_posix.py', 'spawn')}
FIX = 'launch through uml_posix.run (literal argv) or uml_posix.spawn (detached)'
LAUNCHERS = {
    'subprocess': {'run', 'call', 'check_call', 'check_output', 'Popen',
                   'getoutput', 'getstatusoutput'},
    'os': {'system', 'popen', 'startfile', 'fork', 'forkpty', 'posix_spawn', 'posix_spawnp'},
    'asyncio': {'create_subprocess_exec', 'create_subprocess_shell'},
    'pty': {'spawn', 'fork'},
}


def launcher(module, name):
    """Whether `module.name` starts a process (`os.spawn*`/`os.exec*` included)."""
    if module == 'os' and name.startswith(('spawn', 'exec')):
        return True
    return name in LAUNCHERS.get(module, ())


class Scan(ast.NodeVisitor):
    def __init__(self, filename):
        self.filename = filename
        self.modules = {}   # local name -> module
        self.names = {}     # local name -> (module, attribute)
        self.function = None
        self.found = []

    def visit_Import(self, node):
        for alias in node.names:
            if alias.name in LAUNCHERS:
                self.modules[alias.asname or alias.name] = alias.name

    def visit_ImportFrom(self, node):
        for alias in node.names:
            if node.module in LAUNCHERS:
                self.names[alias.asname or alias.name] = (node.module, alias.name)
                if launcher(node.module, alias.name):
                    self.report(node, node.module, alias.name)

    def visit_FunctionDef(self, node):
        outer = self.function
        self.function = outer or node.name
        self.generic_visit(node)
        self.function = outer

    visit_AsyncFunctionDef = visit_FunctionDef

    def visit_Attribute(self, node):
        if isinstance(node.value, ast.Name) and node.value.id in self.modules:
            module = self.modules[node.value.id]
            if launcher(module, node.attr):
                self.report(node, module, node.attr)
        self.generic_visit(node)

    def visit_Name(self, node):
        if node.id in self.names and launcher(*self.names[node.id]):
            self.report(node, *self.names[node.id])

    def report(self, node, module, name):
        if (self.filename, self.function) not in HELPERS:
            self.found.append(f'{self.filename}:{node.lineno}: {module}.{name}; {FIX}')


def violations(source, filename):
    """Every launch-API reference in `source` outside the launch helpers."""
    scan = Scan(filename)
    scan.visit(ast.parse(source, filename))
    return scan.found


class ProcessBoundary(unittest.TestCase):
    def test_production_scripts_launch_only_through_the_helpers(self):
        found = [line for name in PRODUCTION
                 for line in violations((SCRIPTS / name).read_text(encoding='utf-8'), name)]
        self.assertEqual([], found, '\n' + '\n'.join(found))

    def test_the_scan_follows_aliases_and_spares_only_the_helpers(self):
        source = ('import subprocess as sp, os\n'
                  'from subprocess import check_output as co, DEVNULL\n'
                  'def run(argv):\n'
                  '    return sp.run(argv)\n'
                  'def other():\n'
                  '    sp.Popen([], stdin=DEVNULL, creationflags=sp.CREATE_NO_WINDOW)\n'
                  '    os.execv("x", [])\n'
                  '    os.getpid()\n'
                  '    return co\n')
        self.assertEqual([f'uml_posix.py:2: subprocess.check_output; {FIX}',
                          f'uml_posix.py:6: subprocess.Popen; {FIX}',
                          f'uml_posix.py:7: os.execv; {FIX}',
                          f'uml_posix.py:9: subprocess.check_output; {FIX}'],
                         violations(source, 'uml_posix.py'))
        self.assertEqual(f'uml_launch.py:4: subprocess.run; {FIX}',
                         violations(source, 'uml_launch.py')[1])


if __name__ == '__main__':
    unittest.main()
