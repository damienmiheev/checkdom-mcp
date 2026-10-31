#!/usr/bin/env bash
# Decide which parts of the repository a run has to check.
#
# Usage: changes.sh NAME=ERE [NAME=ERE ...]
# For each pair, writes NAME=true to $GITHUB_OUTPUT when a changed file's path
# matches the extended regex ERE, else NAME=false. Every NAME is true when
# there is nothing reliable to compare with: scheduled and manual runs, a new
# branch, or a base commit that can't be fetched (e.g. after a force-push).
#
# Inputs (environment, never interpolated into the script): EVENT, PR_BASE
# (pull_request.base.sha), PUSH_BEFORE (push.before). HEAD is the checked-out
# commit: the PR merge commit, or the pushed commit.
set -euo pipefail

case "${EVENT:-}" in
  pull_request) base="${PR_BASE:-}" ;;
  push) base="${PUSH_BEFORE:-}" ;;
  *) base="" ;;
esac

all=true
files=""
if [[ -n "$base" && ! "$base" =~ ^0+$ ]] &&
  git fetch --quiet --no-tags --depth=1 origin "$base" &&
  files="$(git diff --name-only "$base" HEAD)"; then
  all=false
fi

if [[ "$all" == true ]]; then
  echo "No base to compare with: checking everything."
else
  echo "Changed files:"
  printf '%s\n' "$files"
fi

for spec in "$@"; do
  name="${spec%%=*}"
  pattern="${spec#*=}"
  if [[ "$all" == true ]] || grep -qE "$pattern" <<<"$files"; then
    value=true
  else
    value=false
  fi
  echo "$name=$value" | tee -a "$GITHUB_OUTPUT"
done
