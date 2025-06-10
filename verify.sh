#!/bin/bash

# ==============================================================================
# Etherscan Vyper Contract Verification Script
# ==============================================================================
#
# This script sends a POST request to the Etherscan API to verify a
# deployed Vyper contract.
#
# Usage:
# 1. Fill in the variables in the "CONFIGURATION" section below.
# 2. Create the Vyper JSON-input file (e.g., vyper_input.json).
# 3. Make the script executable: chmod +x verify.sh
# 4. Run the script: ./verify.sh
#

# ==============================================================================
# CONFIGURATION
# ==============================================================================

# Your Etherscan API Key from https://etherscan.io/myapikey
API_KEY="NMS7C8JPHC9UTRFKTNK93CSYS72UBHG9JX"

# The chain ID where the contract is deployed.
CHAIN_ID="480"

# The contract address you want to verify.
CONTRACT_ADDRESS="0x4B3445ad15b39954Aa0fdE07DaC56ECBD63172fe"

# The contract name in "path/to/contract.vy:ContractName" format.
# This must match the path and name inside your vyper_input.json file.
CONTRACT_NAME="src/UntronTransfers.vy:UntronTransfers"

# The path to the Vyper compiler JSON input file.
# This JSON should contain language, sources, and settings.
SOURCE_CODE_JSON_FILE="transfers.json"

# The Vyper compiler version used.
# Format: "vyper:x.y.z"
COMPILER_VERSION="vyper:0.4.0"

# Set to 1 if optimization was used during compilation, 0 otherwise.
OPTIMIZATION_USED="1"

# ABI-encoded constructor arguments, if any. Leave empty if none.
# DO NOT prefix with 0x.
CONSTRUCTOR_ARGS="000000000000000000000000102d758f688a4c1c5a80b116bd945d445546028200000000000000000000000079a02482a880bce3f13e09da970dc34db4cd24d1"


# ==============================================================================
# SCRIPT LOGIC
# ==============================================================================

# Etherscan API endpoint
API_URL="https://api.etherscan.io/v2/api?chainid=$CHAIN_ID"

# Check if the source code file exists
if [ ! -f "$SOURCE_CODE_JSON_FILE" ]; then
    echo "Error: Source code JSON file not found at '$SOURCE_CODE_JSON_FILE'"
    exit 1
fi

# Read the source code from the file
SOURCE_CODE=$(cat "$SOURCE_CODE_JSON_FILE")

echo "Verifying contract..."
echo "  Contract Address: $CONTRACT_ADDRESS"
echo "  Contract Name:    $CONTRACT_NAME"
echo "  Chain ID:         $CHAIN_ID"
echo "  Compiler Version: $COMPILER_VERSION"

# Making the POST request to Etherscan
# We use --data-urlencode for sourceCode to handle special characters.
curl -X POST "$API_URL" \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -d "chainid=$CHAIN_ID" \
  -d "module=contract" \
  -d "action=verifysourcecode" \
  -d "apikey=$API_KEY" \
  -d "codeformat=vyper-json" \
  --data-urlencode "sourceCode=$SOURCE_CODE" \
  -d "contractaddress=$CONTRACT_ADDRESS" \
  -d "contractname=$CONTRACT_NAME" \
  -d "compilerversion=$COMPILER_VERSION" \
  -d "optimizationUsed=$OPTIMIZATION_USED" \
  -d "constructorArguments=$CONSTRUCTOR_ARGS"

echo -e "\n\nVerification request sent. Check the response above."
echo "If successful, the 'result' field will be a GUID. You can use this GUID to check the verification status." 