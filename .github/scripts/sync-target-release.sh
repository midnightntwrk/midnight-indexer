#!/usr/bin/env bash
set -euo pipefail

issue_number=$1
field_name='Target release'
readonly release_prefix='indexer-'

for attempt in 1 2 3; do
  # Queued events can describe a milestone the issue has since left.
  # shellcheck disable=SC2016
  repository=$(gh api graphql \
    -f owner="${GITHUB_REPOSITORY%/*}" \
    -f repository="${GITHUB_REPOSITORY#*/}" \
    -F number="$issue_number" \
    -f query='
      query($owner: String!, $repository: String!, $number: Int!) {
        repository(owner: $owner, name: $repository) {
          issue(number: $number) {
            id
            milestone { title }
            issueFieldValues(first: 25) {
              nodes {
                ... on IssueFieldSingleSelectValue {
                  field { ... on IssueFieldSingleSelect { name } }
                  name
                }
              }
            }
          }
          issueFields(first: 25) {
            nodes {
              ... on IssueFieldSingleSelect {
                name
                options { name }
              }
            }
          }
        }
      }' --jq '.data.repository')

  issue_id=$(jq -er '.issue.id' <<< "$repository")
  # An organization owner creates each Target release option in the issue field settings.
  update=$(jq -c --arg field_name "$field_name" --arg release_prefix "$release_prefix" '
    (.issue.milestone.title | if . == null then null else $release_prefix + ltrimstr($release_prefix) end) as $target_release
    | ([.issue.issueFieldValues.nodes[] | select(.field.name == $field_name) | .name] | first) as $current
    | ([.issueFields.nodes[]? | select(.name == $field_name)] | first) as $release_field
    | if $current == $target_release then null
      elif $target_release == null then {fieldName: $field_name, operation: "CLEAR"}
      elif $release_field == null then error("The \($field_name) dropdown is unavailable to this workflow")
      elif (any($release_field.options[]; .name == $target_release) | not)
        then error("Add \($target_release | @json) to the \($field_name) dropdown in the organization settings")
      else {fieldName: $field_name, operation: "SET", value: $target_release}
      end
  ' <<< "$repository")

  # GitHub can notify subscribers about field updates.
  if [[ "$update" == null ]]; then
    exit 0
  fi

  milestone_title=$(jq -c '.issue.milestone.title' <<< "$repository")
  # A field-specific mutation preserves values set by other workflows.
  updated_issue=$(jq -n --arg issue_id "$issue_id" --argjson update "$update" '{
    query: "mutation($input: UpdateIssueInput!) { updateIssue(input: $input) { issue { id milestone { title } } } }",
    variables: {input: {id: $issue_id, issueFieldUpdates: [$update]}}
  }' | gh api graphql --input - --jq '.data.updateIssue.issue')
  jq -e --arg issue_id "$issue_id" '.id == $issue_id' <<< "$updated_issue" > /dev/null

  # Issue assignment and milestone edits can write to the same issue concurrently.
  if [[ $(jq -c '.milestone.title' <<< "$updated_issue") == "$milestone_title" ]]; then
    exit 0
  fi
done

printf 'Issue #%s changed milestones during all %s sync attempts.\n' "$issue_number" "$attempt" >&2
exit 1
