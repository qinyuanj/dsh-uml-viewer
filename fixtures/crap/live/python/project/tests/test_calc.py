from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))
from demo.calc import choose


class CalcTest(unittest.TestCase):
    def test_positive(self):
        self.assertEqual(6, choose(3))


if __name__ == "__main__":
    unittest.main()
