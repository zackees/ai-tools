#!/usr/bin/env bash
# Aggregate verdict for the `CI summary` required check.
#
# Lists every job of this workflow run attempt through the GitHub API and
# fails if any job (other than this one) concluded failure, cancelled,
# timed_out, startup_failure or action_required, or is still not complete.
# Deliberately does NOT use `needs.*.result`: that misses jobs that never
# started and jobs outside the needs graph.
#
# Env: GH_TOKEN, GITHUB_REPOSITORY, GITHUB_RUN_ID, GITHUB_RUN_ATTEMPT,
#      SELF_JOB_NAME (this job's display name, excluded from the verdict).
set -euo pipefail

url="repos/${GITHUB_REPOSITORY}/actions/runs/${GITHUB_RUN_ID}/attempts/${GITHUB_RUN_ATTEMPT}/jobs?per_page=100"
jobs=""
for attempt in 1 2 3 4 5; do
  if jobs="$(gh api --paginate "$url" --jq '.jobs[] | [.name, .status, (.conclusion // "none")] | @tsv')"; then
    break
  fi
  jobs=""
  echo "::warning::jobs API call failed (attempt ${attempt}/5); retrying"
  sleep $((attempt * 10))
done
if [ -z "$jobs" ]; then
  echo "::error::could not list jobs for run ${GITHUB_RUN_ID} attempt ${GITHUB_RUN_ATTEMPT}"
  exit 1
fi

bad=0
seen=0
while IFS=$'\t' read -r name status conclusion; do
  [ "$name" = "$SELF_JOB_NAME" ] && continue
  seen=$((seen + 1))
  printf '%-40s %-12s %s\n' "$name" "$status" "$conclusion"
  case "$conclusion" in
    failure|cancelled|timed_out|startup_failure|action_required)
      echo "::error::job '${name}' concluded ${conclusion}"
      bad=1 ;;
  esac
  if [ "$status" != "completed" ]; then
    echo "::error::job '${name}' is ${status}, not completed"
    bad=1
  fi
done <<< "$jobs"

if [ "$seen" -eq 0 ]; then
  echo "::error::no upstream jobs found in this run"
  exit 1
fi
exit "$bad"
