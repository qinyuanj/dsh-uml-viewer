"""Development-only generator. Run the pinned oracle in a temporary copy.

Usage (repository root):
  python fixtures/crap/live/oracle.py

Oracle must be checked out at D:/dev/github/spec203-notes/ref/crapper
commit 4068857bc44e6383cf4a5e4d0b5c1111b49ce0b4 with Python 3.11+,
tree-sitter-language-pack and coverage>=7 installed. Node and npm are needed
for TypeScript; c8 10.1.3 is installed only in the temporary project, whose
npm cache is also temporary. No golden bytes are manually edited.
"""

import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

LIVE = Path(__file__).resolve().parent
PIN = "4068857bc44e6383cf4a5e4d0b5c1111b49ce0b4"
ORACLE = Path(os.environ.get("CRAPPER_ORACLE", "D:/dev/github/spec203-notes/ref/crapper"))
PYTHON_FAILURE = """from pathlib import Path
import sys
import unittest
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))
from demo.calc import choose
class FailingTest(unittest.TestCase):
    def test_failure(self):
        self.assertEqual(0, choose(3))
"""
TYPESCRIPT_FAILURE = """import assert from 'node:assert/strict';
import test from 'node:test';
import { choose } from './calc.js';
test('fails', () => assert.equal(choose(3), 0));
"""


def generate(language: str) -> None:
    head = subprocess.check_output(["git", "-C", str(ORACLE), "rev-parse", "HEAD"], text=True).strip()
    if head != PIN:
        raise SystemExit(f"oracle at {head}, expected {PIN}")
    env = dict(os.environ, PYTHONPATH=str(ORACLE / "src"), PIP_DISABLE_PIP_VERSION_CHECK="1")
    for failing in (False, True):
        if failing and language.startswith("lua"):
            continue
        with tempfile.TemporaryDirectory(prefix=f"crap-oracle-{language}-") as directory:
            project = Path(directory) / "project"
            source = LIVE / ("lua/modules" if language == "lua-modules" else language) / "project"
            shutil.copytree(source, project)
            env["npm_config_cache"] = str(Path(directory) / "npm-cache")
            if language == "typescript":
                subprocess.run(["npm.cmd" if os.name == "nt" else "npm", "install", "--ignore-scripts", "--no-audit", "--no-fund", "--no-global"],
                               cwd=project, env=env, check=True)
            if failing:
                if language == "python":
                    (project / "tests/test_calc.py").write_text(PYTHON_FAILURE, encoding="utf-8")
                else:
                    (project / "src/calc.test.js").write_text(TYPESCRIPT_FAILURE, encoding="utf-8")
            shards = ["--lua-shards", "2"] if language.startswith("lua") else []
            runner = "import sys; sys.path.insert(0, sys.argv[1]); from crapper.cli import main; main(sys.argv[2:])"
            completed = subprocess.run([sys.executable, "-I", "-c", runner, str(ORACLE / "src"), "--root", str(project), *shards],
                                       cwd=project, env=env)
            if completed.returncode != 0:
                raise SystemExit(f"oracle failed: {language}, failing={failing}")
            report = project / "target/coverage" / ("lua" if language == "lua-modules" else language) / "lcov.info"
            if not report.is_file():
                raise SystemExit(f"oracle wrote no {report}")
            destination = LIVE / ("lua/modules" if language == "lua-modules" else language)
            shutil.copyfile(project / ".metrics/crap.edn", destination / ("failing.edn" if failing else "expected.edn"))
            print(f"oracle wrote {language}/{'failing' if failing else 'expected'}.edn")


if __name__ == "__main__":
    for name in sys.argv[1:] or ["python", "typescript", "lua"]:
        generate(name)
