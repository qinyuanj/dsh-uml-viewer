# IR byte references (#201 seam 1)

Each `<fixture>.edn` is the exact `uml-viewer ir` output for the fixture
project of the same name, generated on **Linux**. `crates/ir/tests/ir_bytes.rs`
regenerates the IR on whichever host runs the suite and requires the same
bytes, so a Windows run (CRLF working tree included) proves the path model and
the four scanners are host-independent. `.gitattributes` keeps these files LF
under `core.autocrlf`.

On a mismatch the test keeps its output and names it; `python
scripts/compare_ir.py <output> <reference>` then reports the first structural
difference.

Regenerate only when the IR itself is meant to change, and only on Linux,
from a clean LF export of the commit (a Windows `git archive` with
`-c core.autocrlf=false` also works when the worktree's `.git` is not
readable from WSL):

```sh
git -c core.autocrlf=false archive HEAD | tar -x -C /tmp/uv-tree
cd /tmp/uv-tree && cargo build --locked -p uml-viewer
for fx in rust-fixture python-fixture typescript-fixture lua-fixture; do
  target/debug/uml-viewer ir --root $fx --policy policy.edn \
    --output <checkout>/fixtures/ir-bytes/$fx.edn
done
```