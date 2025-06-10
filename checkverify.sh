#!/bin/bash

# Get the GUID from command line argument
GUID=$1
CHAIN_ID=$2

if [ -z "$GUID" ] || [ -z "$CHAIN_ID" ]; then
    echo "Please provide the GUID and chain ID as arguments"
    echo "Usage: ./checkverify.sh <guid> <chain_id>"
    exit 1
fi

# Your Etherscan API Key from verify.sh
API_KEY="NMS7C8JPHC9UTRFKTNK93CSYS72UBHG9JX"

# Make the API request
curl -X GET "https://api.etherscan.io/v2/api?chainid=$CHAIN_ID" \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -d "module=contract" \
  -d "action=checkverifystatus" \
  -d "chainid=$CHAIN_ID" \
  -d "guid=$GUID" \
  -d "apikey=$API_KEY"

echo -e "\n"