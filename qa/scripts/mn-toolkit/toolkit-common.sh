#!/bin/bash

# Common functions for midnight-node-toolkit scripts

# Get the toolkit container name and extract environment
get_toolkit_environment() {
    TOOLKIT_CONTAINER=`docker ps -a --format "table {{.Image}}\t{{.Names}}\t{{.Status}}"|grep midnight-node-toolkit|awk '{print $2}'`
    
    if [ -z "$TOOLKIT_CONTAINER" ]; then
        echo "No toolkit container found"
        return 1
    fi
    
    ENVIRONMENT=${TOOLKIT_CONTAINER#toolkit-}
    echo "$ENVIRONMENT"
}

# Validate environment is one of the supported environments
validate_environment() {
    local env=$1
    if [ "$env" != "undeployed" ] && [ "$env" != "devnet" ] && [ "$env" != "testnet02" ] && [ "$env" != "nodedev01" ] && [ "$env" != "qanet" ]; then
        echo "Invalid environment: $env"
        echo "Supported environments: undeployed, devnet, testnet02, nodedev01, qanet"
        return 1
    fi
    return 0
}

# Get network ID and node URL for an environment
get_network_config() {
    local env=$1
    
    case $env in
        "undeployed")
            NETWORK_ID="undeployed"
            NODE_URL="ws://localhost:9944"
            ;;
        "devnet")
            NETWORK_ID="devnet"
            NODE_URL="wss://rpc.devnet.midnight.network"
            ;;
        "testnet02")
            NETWORK_ID="testnet"
            NODE_URL="wss://rpc.testnet02.midnight.network"
            ;;
        "qanet")
            NETWORK_ID="devnet"
            NODE_URL="wss://rpc.qanet.dev.midnight.network"
            ;;
        "nodedev01")
            NETWORK_ID="devnet"
            NODE_URL="wss://rpc.node-dev-01.dev.midnight.network"
            ;;
        *)
            echo "Unknown environment: $env"
            return 1
            ;;
    esac
    
    # Export so they're available to caller
    export NETWORK_ID
    export NODE_URL
}

