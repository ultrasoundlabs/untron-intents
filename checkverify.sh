#!/bin/bash

# Get the GUID from command line argument
GUID=$1

if [ -z "$GUID" ]; then
    echo "Please provide the GUID as an argument"
    echo "Usage: ./checkverify.sh <guid>"
    exit 1
fi

# Your Etherscan API Key from verify.sh
API_KEY="NMS7C8JPHC9UTRFKTNK93CSYS72UBHG9JX"

# Make the API request
curl -X GET "https://api.etherscan.io/v2/api" \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -d "module=contract" \
  -d "action=checkverifystatus" \
  -d "chainid=1" \
  -d "guid=$GUID" \
  -d "apikey=$API_KEY"

echo -e "\n"