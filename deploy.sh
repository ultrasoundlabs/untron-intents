source .env

NETWORK=$1

if [ -z "$NETWORK" ]; then
    echo "Please provide the NETWORK as an argument"
    echo "Usage: ./deploy.sh <network>"
    exit 1
fi

echo "y" | mox deploy UntronTransfers --network $NETWORK --private-key $PRIVATE_KEY