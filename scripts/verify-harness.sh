#!/usr/bin/env bash
set -euo pipefail

root_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root_dir"

required=(
  README.md AGENTS.md CONTRIBUTING.md SECURITY.md
  docs/GOAL.md docs/DOMAIN.md docs/ROADMAP.md docs/CHANGELOG.md
  docs/DEVELOPMENT.md docs/DEPLOYMENT.md
  .agents/agent-harness/README.md
  .agents/agent-harness/task-template.md
  .agents/agent-harness/verification-matrix.md
  .agents/decisions/README.md
  .agents/PROGRESS.md .agents/DECISIONS.md
)

for path in "${required[@]}"; do
  if [[ ! -s "$path" ]]; then
    echo "missing required harness file: $path" >&2
    exit 1
  fi
done

if (( $(wc -w < AGENTS.md) > 1200 )); then
  echo "AGENTS.md exceeds the 1,200-word context budget" >&2
  exit 1
fi

if rg -n 'GOAL_fix\.md|DAG/chain|provider-native representation' --glob '*.md' .; then
  echo "obsolete or contradictory domain language found" >&2
  exit 1
fi

echo "agent harness: ok"
