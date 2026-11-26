#!/usr/bin/env bash
set -euo pipefail

: "${FETCH_TOKEN:?Set a GitHub token with read access to the dependency repositories}"

# The GitHub HTTPS credential helper reads the token when Git invokes it.
# shellcheck disable=SC2016
CARGO_NET_GIT_FETCH_WITH_CLI=true \
GIT_CONFIG_COUNT=2 \
GIT_CONFIG_KEY_0=credential.https://github.com.helper \
GIT_CONFIG_VALUE_0='' \
GIT_CONFIG_KEY_1=credential.https://github.com.helper \
GIT_CONFIG_VALUE_1='!helper() { if [ "$1" = get ]; then printf "username=x-access-token\npassword=%s\n" "$FETCH_TOKEN"; fi; }; helper' \
    cargo fetch --locked
