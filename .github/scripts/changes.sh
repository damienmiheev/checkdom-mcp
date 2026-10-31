#!/usr/bin/env bash
# Decide which parts of the repository a run has to check.
#
# Usage: changes.sh SPEC [SPEC ...], where SPEC is
#   NAME=ERE   NAME is true when some changed path matches ERE
#   NAME!=ERE  NAME is true when some changed path does NOT match ERE
#              (use this for "everything except docs", so new kinds of files
#              are checked by default)
# Results go to $GITHUB_OUTPUT. Every NAME is true when there is nothing
# reliable to compare with: scheduled and manual runs, a new branch, or a base
# commit that can't be fetched (e.g. after a force-push).
#
# Inputs (environment, never interpolated into the script): EVENT and
# PUSH_BEFORE (push.before). A pull_request run compares its merge commit
# with the merge commit's first parent (check out with fetch-depth: 2).
set -euo pipefail

all=true
paths=()
base=""
case "${EVENT:-}" in
  pull_request) base="$(git rev-parse --verify --quiet 'HEAD^1' || true)" ;;
  push)
    if [[ -n "${PUSH_BEFORE:-}" && ! "$PUSH_BEFORE" =~ ^0+$ ]] &&
      git fetch --quiet --no-tags --depth=1 origin "$PUSH_BEFORE"; then
      base="$PUSH_BEFORE"
    fi
    ;;
esac

if [[ -n "$base" ]]; then
  list="$(mktemp)"
  # --no-renames lists both sides of a rename; -z keeps unusual names unquoted.
  if git diff --name-only --no-renames -z "$base" HEAD >"$list"; then
    mapfile -d '' -t paths <"$list"
    all=false
  fi
  rm -f "$list"
fi

if [[ "$all" == true ]]; then
  echo "No base to compare with: checking everything."
else
  echo "Changed files (${#paths[@]}):"
  # Indented, so a file name can never be read as a workflow command.
  for p in "${paths[@]}"; do printf '  %q\n' "$p"; done
fi

# Exit status 0 = match, 1 = no match, 2 = invalid pattern.
matches() { [[ "$1" =~ $2 ]]; }

for spec in "$@"; do
  if [[ "$spec" == *'!='* ]]; then
    name="${spec%%!=*}" pattern="${spec#*!=}" want=1
  else
    name="${spec%%=*}" pattern="${spec#*=}" want=0
  fi
  # An invalid pattern must fail the run, not silently skip a job.
  rc=0
  matches "" "$pattern" || rc=$?
  if ((rc > 1)); then
    echo "::error::invalid pattern for $name: $pattern"
    exit 1
  fi

  value=false
  if [[ "$all" == true ]]; then
    value=true
  else
    for p in "${paths[@]}"; do
      rc=0
      matches "$p" "$pattern" || rc=$?
      if ((rc == want)); then
        value=true
        break
      fi
    done
  fi
  echo "$name=$value" | tee -a "$GITHUB_OUTPUT"
done
