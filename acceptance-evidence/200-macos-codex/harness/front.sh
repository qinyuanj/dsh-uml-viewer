#!/bin/sh
p=$(pgrep -f 'uml-viewer viewer' | head -1)
osascript -e "tell application \"System Events\" to set frontmost of (first process whose unix id is $p) to true" >/dev/null 2>&1
