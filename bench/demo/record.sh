#!/usr/bin/env bash
export PATH=/tmp/demo/bin:$PATH
cd /tmp/demo
type_run() {
  printf '\033[1;32m❯\033[0m '
  local s="$1"
  for ((i=0;i<${#s};i++)); do printf '%s' "${s:$i:1}"; sleep 0.035; done
  sleep 0.5; printf '\n'
  eval "$1"
  sleep "${2:-2}"
}
clear
type_run 'anymd attention.pdf --pages 8 | head -24' 3
type_run 'anymd search "masked language model" . --max 2' 3
type_run 'anymd budget.xlsx | head -7' 2.5
type_run 'claude -p "What BLEU did the big Transformer get? Cite the page in attention.pdf" --mcp-config .mcp.json --allowedTools mcp__anymd__read' 4
