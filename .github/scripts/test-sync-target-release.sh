#!/usr/bin/env bash
set -euo pipefail

script="$(dirname "$0")/sync-target-release.sh"
mutation_log=$(mktemp)
trap 'rm -f -- "$mutation_log"' EXIT
export mutation_log
export GITHUB_REPOSITORY='example/repository'

gh() {
  [[ "$1 $2" == 'api graphql' ]] || return 1
  if [[ "$3" == --input ]]; then
    jq -ce '.variables.input' >> "$mutation_log" || return
    printf '%s\n' "$updated_issue"
  elif [[ -n "$repository_after_write" && -s "$mutation_log" ]]; then
    printf '%s\n' "$repository_after_write"
  else
    printf '%s\n' "$repository"
  fi
}
export -f gh

fixture() {
  jq -n --arg title "$1" --arg current "$2" --argjson options "$3" '{
    issue: {
      id: "I_issue",
      milestone: (if $title == "" then null else {title: $title} end),
      issueFieldValues: {nodes: [
        {field: {name: "Origin"}, name: "Public"},
        (if $current != "" then {field: {name: "Target release"}, name: $current} else empty end)
      ]}
    },
    issueFields: {nodes: [
      {name: "Other field", options: [{name: "indexer-4.6.0"}]},
      (if $options != null then {name: "Target release", options: ($options | map({name: .}))} else empty end)
    ]}
  }'
}

check() {
  local name=$1 expected_updates=$2 expected_error=${3:-}
  local output status=0
  : > "$mutation_log"
  output=$(bash "$script" 10 2>&1) || status=$?
  if [[ -n "$expected_error" ]]; then
    if [[ "$status" == 0 || "$output" != *"$expected_error"* ]]; then
      printf '%s: expected error %s, got %s\n' "$name" "$expected_error" "$output" >&2
      return 1
    fi
  elif [[ "$status" != 0 ]]; then
    printf '%s: %s\n' "$name" "$output" >&2
    return 1
  fi

  jq -es --argjson expected "$expected_updates" '
    . == ($expected | map({id: "I_issue", issueFieldUpdates: [({fieldName: "Target release"} + .)]}))
  ' "$mutation_log" > /dev/null || {
    printf '%s: unexpected issue updates\n' "$name" >&2
    cat "$mutation_log" >&2
    return 1
  }
  printf '%s passed\n' "$name"
}

export repository repository_after_write='' updated_issue
repository=$(fixture '4.5.0' '' '["indexer-4.5.0"]')
updated_issue='{"id":"I_issue","milestone":{"title":"4.5.0"}}'
check 'Bare milestone title' '[{"operation":"SET","value":"indexer-4.5.0"}]'

repository=$(fixture 'indexer-4.5.0' '' '["indexer-4.5.0"]')
updated_issue='{"id":"I_issue","milestone":{"title":"indexer-4.5.0"}}'
check 'Prefixed milestone title' '[{"operation":"SET","value":"indexer-4.5.0"}]'

repository=$(fixture '4.4.0-rc.6' 'indexer-4.5.0' '["indexer-4.4.0-rc.6"]')
updated_issue='{"id":"I_issue","milestone":{"title":"4.4.0-rc.6"}}'
check 'Changed milestone title' '[{"operation":"SET","value":"indexer-4.4.0-rc.6"}]'

repository=$(fixture '4.6.0' 'indexer-4.5.0' '["indexer-4.5.0"]')
check 'Missing option' '[]' 'Add "indexer-4.6.0" to the Target release dropdown'

repository=$(fixture '4.6.0' '' 'null')
check 'Missing dropdown' '[]' 'The Target release dropdown is unavailable'

repository=$(fixture '4.5.0' 'indexer-4.5.0' '["indexer-4.5.0"]')
check 'Already synced' '[]'

repository=$(fixture '' 'indexer-4.5.0' '[]')
updated_issue='{"id":"I_issue","milestone":null}'
check 'Removed milestone' '[{"operation":"CLEAR"}]'

repository=$(fixture '' '' '[]')
check 'Already clear' '[]'

repository=$(fixture '4.5.0' '' '["indexer-4.5.0"]')
repository_after_write=$(fixture '' 'indexer-4.5.0' '["indexer-4.5.0"]')
check 'Milestone removed during sync' '[{"operation":"SET","value":"indexer-4.5.0"},{"operation":"CLEAR"}]'
