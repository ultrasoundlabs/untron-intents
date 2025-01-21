#!/bin/bash

# Build the contract
mox build

# Load environment variables from .env file
if [ -f .env ]; then
    export $(cat .env | grep -v '^#' | xargs)
else
    echo ".env file not found"
    exit 1
fi

# Read bytecode from UntronTransfers.json
if [ -f out/UntronTransfers.json ]; then
    BYTECODE=$(jq -r '.bytecode' out/UntronTransfers.json)
else
    echo "out/UntronTransfers.json not found"
    exit 1
fi

# Deploy contract
cast send --rpc-url $RPC_URL --private-key $PRIVATE_KEY --create $BYTECODE