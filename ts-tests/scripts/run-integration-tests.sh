#!/bin/bash

# This script serves as an emulation of the CI integration test execution
# DEPENDENCIES:
# - earthly
# - cargo
# - just
# - docker
# - npm/node
# - yarn

# ENVIRONMENT
# Please set the two following env variables before running the script

# - Check on TOP variable set - Make it point to the root of the indexer repo
if [[ -z "$TOP" ]]; then
  echo "[ERROR] - TOP must be set and point to your indexer repo root folder"
  exit 1
fi

# - Check on ENV_TYPE variable set  (compose, devnet01, qanet, testnet, testnet02)
if [[ -z "$ENV_TYPE" || (
  "$ENV_TYPE" != "compose" &&
  "$ENV_TYPE" != "nodedev01" &&
  "$ENV_TYPE" != "qanet" &&
  "$ENV_TYPE" != "testnet" &&
  "$ENV_TYPE" != "testnet02") ]]; then
  echo "[ERROR] - ENV_TYPE must be set to either 'compose', 'qanet', 'nodedev01', 'testnet' or 'testnet02'"
  exit 1
fi

# - Check on DEPLOYMENT variable set (cloud, standalone)
if [[ -z "$DEPLOYMENT" || ("$DEPLOYMENT" != "cloud" && "$DEPLOYMENT" != "standalone") ]]; then
  echo "[ERROR] - DEPLOYMENT must be set to either 'standalone' or 'cloud'"
  exit 1
fi

# ALSO there are some secrets that need to be set
check_is_set_or_error() {

  local variable_name=$1
  local error_message=$2

  if [ -z "${!variable_name}" ]; then
    echo "$error_message"
    exit 1
  fi
}

check_is_set_or_error POSTGRES_PASSWORD "[ERROR] - Please set a value for POSTGRES_PASSWORD"
check_is_set_or_error APP__INFRA__SECRET "[ERROR] - Please set a value for APP__INFRA__SECRET"
check_is_set_or_error APP__INFRA__PUB_SUB__PASSWORD "[ERROR] - Please set a value for APP__INFRA__PUB_SUB__PASSWORD"
check_is_set_or_error APP__INFRA__STORAGE__PASSWORD "[ERROR] - Please set a value for APP__INFRA__STORAGE__PASSWORD="
check_is_set_or_error APP__INFRA__ZSWAP_STATE_STORAGE__PASSWORD "[ERROR] - Please set a value for APP__INFRA__ZSWAP_STATE_STORAGE__PASSWORD"

if [[ "$ENV_TYPE" == "compose" ]]; then
  if [[ "$DEPLOYMENT" == "cloud" ]]; then
    echo "[INFO] - Building indexer in multi-container mode"
    just docker-wallet-indexer dev &
    just docker-chain-indexer dev &
    just docker-indexer-api dev
  else
    echo "[INFO] - Building indexer in standalone mode"
    just docker-indexer-standalone dev
  fi
fi

pushd $TOP/ts-tests/

yarn && yarn compact
yarn ci_integration_tests

popd
