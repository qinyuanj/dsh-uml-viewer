#!/bin/sh
# usage: launch.sh <A|B> <driver> [extra ./uml args]
W=/home/lzx/dev/github/uml-viewer/.claude/worktrees/spec192-199
BASE=/tmp/spec199-run1
n=$1; d=$2; shift 2
mkdir -p "$BASE/evidence/$n"
cd "$BASE/$n 项目" || exit 1
UML_VIEWER_TOOL_ROOT=$W UML_COMPANION_NO_ATTACH=1 \
UML_VIEWER_TEST_DRIVER=$BASE/scripts/$d.edn UML_VIEWER_EVIDENCE=$BASE/evidence/$n \
UML_CJK_FONT="$BASE/$n 项目/.uml-viewer/fonts/SarasaMonoSC-Regular.ttf" \
exec ./uml "$@"
