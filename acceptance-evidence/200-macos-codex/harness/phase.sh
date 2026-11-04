#!/bin/sh
# usage: phase.sh <A|B> <name> <session> [timeout_s]
BASE=/tmp/spec200; n=$1; ph=$2; s=$3; to=${4:-300}
E=$BASE/evidence/${EVDIR:-$n}; P="$BASE/$n 项目/.uml-viewer"
date -u +%FT%TZ > $E/go-$ph.time; touch $E/go-$ph
i=0; while [ ! -f $E/sent-$ph ] && [ $i -lt 60 ]; do sleep 1; i=$((i+1)); done
echo "sent-$ph after ${i}s: $(cat $E/to-agent-$ph.edn 2>/dev/null)"
i=0; while [ $i -lt $to ]; do
  a=$(cat "$P/to-agent.edn"); v=$(cat "$P/to-viewer.edn" 2>/dev/null)
  case "$a" in *":queue []"*) case "$v" in *":queue []"*|"") st=$(herdr --session $s agent get w1:p1 2>/dev/null | grep -o '"agent_status":"[a-z]*"'); [ "$st" = '"agent_status":"idle"' ] && break;; esac;; esac
  sleep 3; i=$((i+3)); done
echo "settled after ${i}s"; echo "to-agent: $(cat "$P/to-agent.edn")"; echo "to-viewer: $(cat "$P/to-viewer.edn")"
herdr --session $s pane read w1:p1 --source recent --lines 200 > $E/pane-$ph.txt 2>&1
