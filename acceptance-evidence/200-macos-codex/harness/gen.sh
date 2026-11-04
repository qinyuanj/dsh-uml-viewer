#!/bin/bash
# usage: gen.sh <name> <phase>:<id>=<menu>[,<id>=<menu>] ...
f=/tmp/spec200/scripts/$1.edn; shift
{ echo '{:version 1 :doc "spec200 driver" :steps ['; echo '         {:op :wait-frames :n 5}'
for spec in "$@"; do ph=${spec%%:*}; rest=${spec#*:}
  echo "         {:op :wait-file :evidence \"go-$ph\"}"; echo "         {:op :screenshot :evidence \"before-$ph.png\"}"
  IFS=',' read -ra pairs <<< "$rest"
  for pair in "${pairs[@]}"; do id=${pair%%=*}; op=${pair#*=}
    echo "         {:op :click-class :id :$id}"; echo '         {:op :wait-frames :n 2}'
    echo "         {:op :click-class :id :$id :button :secondary}"; echo '         {:op :wait-frames :n 3}'
    echo "         {:op :gui-click :at \"menu-$op\"}"; echo '         {:op :wait-frames :n 5}'; done
  echo "         {:op :dump-project :project \".uml-viewer/to-agent.edn\" :evidence \"to-agent-$ph.edn\"}"
  echo "         {:op :dump-gui :evidence \"gui-$ph.edn\"}"; echo "         {:op :write :evidence \"sent-$ph\" :text \"sent\"}"; done
echo '         {:op :wait-file :evidence "go-never"}]}'; } > "$f"
