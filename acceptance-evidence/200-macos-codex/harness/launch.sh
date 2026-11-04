#!/bin/sh
# usage: launch.sh <A|B> <driver|-> [extra ./uml args]
# ATTACH=1 keeps the Terminal.app attach; default is UML_COMPANION_NO_ATTACH=1.
W=/Users/billyq/Dev/unclebob/uml-viewer
BASE=/tmp/spec200
n=$1; d=$2; shift 2
mkdir -p "$BASE/evidence/${EVDIR:-$n}"
cd "$BASE/$n 项目" || exit 1
export UML_VIEWER_TOOL_ROOT=$W UML_VIEWER_EVIDENCE=$BASE/evidence/${EVDIR:-$n}
export UML_CJK_FONT="$BASE/$n 项目/.uml-viewer/fonts/SarasaMonoSC-Regular.ttf"
# Homebrew rustup does not put ~/.cargo/bin on PATH; crapper installs cargo-llvm-cov there.
export PATH="$HOME/.cargo/bin:$PATH"
[ "${ATTACH:-0}" = 1 ] || export UML_COMPANION_NO_ATTACH=1
[ "$d" = - ] || export UML_VIEWER_TEST_DRIVER=$BASE/scripts/$d.edn
exec ./uml "$@"
