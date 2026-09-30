#!/usr/bin/env bash
set -euo pipefail

required=(
  CHORO_GIT_WORKFLOW_ACCEPTANCE_REPO
  CHORO_GIT_WORKFLOW_ACCEPTANCE_SOURCE
  CHORO_GIT_WORKFLOW_ACCEPTANCE_DESTINATION
  CHORO_GIT_WORKFLOW_ACCEPTANCE_ALLOW_MUTATION
)
for name in "${required[@]}"; do
  if [[ -z "${!name:-}" ]]; then
    echo "$name must be set" >&2
    exit 2
  fi
done
if [[ "$CHORO_GIT_WORKFLOW_ACCEPTANCE_ALLOW_MUTATION" != "1" ]]; then
  echo "Set CHORO_GIT_WORKFLOW_ACCEPTANCE_ALLOW_MUTATION=1 only for a disposable GitHub fixture" >&2
  exit 2
fi

acceptance_repo="$CHORO_GIT_WORKFLOW_ACCEPTANCE_REPO"
git -C "$acceptance_repo" rev-parse --show-toplevel >/dev/null

snapshot_local_git_state() {
  git -C "$acceptance_repo" symbolic-ref -q HEAD || git -C "$acceptance_repo" rev-parse HEAD
  git -C "$acceptance_repo" ls-files --stage
  git -C "$acceptance_repo" status --porcelain=v2 --untracked-files=all
  git -C "$acceptance_repo" diff --binary
  git -C "$acceptance_repo" diff --cached --binary
  while IFS= read -r path; do
    printf '%s ' "$path"
    git -C "$acceptance_repo" hash-object -- "$path"
  done < <(git -C "$acceptance_repo" ls-files --others --exclude-standard)
  git -C "$acceptance_repo" for-each-ref --format='%(refname) %(objectname)'
}

before_state="$(snapshot_local_git_state)"
verify_local_state_unchanged() {
  local command_status=$?
  local after_state
  after_state="$(snapshot_local_git_state)"
  if [[ "$after_state" != "$before_state" ]]; then
    echo "Git Workflow acceptance changed the checked-out branch, index, working tree, files, or local refs" >&2
    diff -u <(printf '%s\n' "$before_state") <(printf '%s\n' "$after_state") || true
    exit 1
  fi
  echo "Local Git state unchanged before and after the live GitHub acceptance scenario"
  exit "$command_status"
}
trap verify_local_state_unchanged EXIT

cargo test -p ide-app live_github_acceptance_suite -- --ignored --nocapture
