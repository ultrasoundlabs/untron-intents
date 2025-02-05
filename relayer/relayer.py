import json
import asyncio
import os
from web3 import Web3
from base58 import b58encode_check

# Tron-related imports
from tronpy import Tron
from tronpy.providers import HTTPProvider
from tronpy.keys import PrivateKey

# ------------------------------------------------------------------------------
# Load config
# ------------------------------------------------------------------------------
config = json.load(open("config.json"))

# ------------------------------------------------------------------------------
# Initialize Tron client (or your Tron API provider of choice)
# ------------------------------------------------------------------------------
client = Tron(
    provider=HTTPProvider(
        "https://api.trongrid.io",
        api_key=config["trongrid_api_key"]
    )
)

private_key = PrivateKey(bytes.fromhex(config["tron_private_key"][2:]))
from_address = private_key.public_key.to_base58check_address()
print(f"Relayer Tron address: {from_address}")

# If you need to do a specialized USDT swap, get your DEX contract, e.g.:
sunswap_v2 = client.get_contract("TXF1xDbVGdxFGbovmmmXvBGu8ZiE3Lq4mR")  # Example

# ------------------------------------------------------------------------------
# Load UntronTransfers ABI
# ------------------------------------------------------------------------------
with open("../out/UntronTransfers.json") as f:
    untron_transfers_artifact = json.load(f)
untron_abi = untron_transfers_artifact["abi"]

# ------------------------------------------------------------------------------
# Helpers for Tron side
# ------------------------------------------------------------------------------
async def send_usdt(to_address: str, amount: int):
    """
    Example function that performs a Tron-based token transfer or swap.
    Here, we do a "swapTokensForExactTokens" on SunSwap as a demonstration.
    Adjust to your real logic for transferring USDT, USDC, or other tokens.
    """
    print(f"[Tron] Swapping into USDT => to: {to_address} amount: {amount}")
    try:
        txn = (
            sunswap_v2.functions.swapTokensForExactTokens(
                amount,
                999999999999999999999999,  # large max input
                [
                    "TPXxtMtQg95VX8JRCiQ5SXqSeHjuNaMsxi",  # e.g. TRX -> USDT route
                    "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t"
                ],
                to_address,
                9999999999  # large deadline
            )
            .with_owner(from_address)
            .fee_limit(2_000_000)
            .build()
            .sign(private_key)
        )
        receipt = txn.broadcast().wait()
        print(f"[Tron] Swap/transfer tx result: {receipt}")
        return True
    except Exception as e:
        print(f"[Tron] Error sending USDT: {e}")
        return False

def is_profitable(chain, order):
    """
    Check if an order is profitable based on configured fees for the token on the specific chain.
    Uses basis points (1/10000) for percentage calculations to avoid floats.
    Returns False if token is not in allowed list for the chain.
    """
    token = order["token"]
    if token not in chain["tokens"]:
        print(f"Token {token} not in allowed tokens list for chain {chain['name']}")
        return False

    token_config = chain["tokens"][token]
    input_amount = order["inputAmount"]
    output_amount = order["outputAmount"]
    
    # Calculate total fee (static + percentage)
    percentage_fee = (output_amount * token_config["percentage_fee_bps"]) // 10000
    total_fee = token_config["static_fee"] + percentage_fee
    
    # Order is profitable if input covers output plus fees
    is_profitable = input_amount >= (output_amount + total_fee)

    print("input_amount", input_amount)
    print("output_amount", output_amount)
    print("total_fee", total_fee)
    print("is_profitable", is_profitable)

    if not is_profitable:
        print(f"[{chain['name']}] Order not profitable - Input: {input_amount}, Output: {output_amount}, Fee: {total_fee}")
    
    return is_profitable

# ------------------------------------------------------------------------------
# Reclaim or claim on UntronTransfers (in the new contract, it's named 'claim')
# ------------------------------------------------------------------------------
def claim_order(web3, contract, account, order_id):
    """
    Once your Tron side is done, call contract.functions.claim(orderId).
    This finalizes the order and emits OrderFilled.
    """
    print(f"[EVM] Claiming order {order_id.hex()} ...")
    try:
        tx = contract.functions.claim(order_id).build_transaction({
            "from": account.address,
            "nonce": web3.eth.get_transaction_count(account.address),
            "gas": 3000000,
            "gasPrice": web3.eth.gas_price,
        })
        signed_tx = account.sign_transaction(tx)
        tx_hash = web3.eth.send_raw_transaction(signed_tx.raw_transaction)
        receipt = web3.eth.wait_for_transaction_receipt(tx_hash)
        print(f"[EVM] claim() success. Tx hash: {receipt['transactionHash'].hex()}")
    except Exception as e:
        print(f"[EVM] Failed to claim order: {e}")

# ------------------------------------------------------------------------------
# Process the OrderCreated event
# ------------------------------------------------------------------------------
async def process_order_created_event(web3, contract, account, event, chain):
    """
    Called when we see an OrderCreated log. 
    'event' includes orderId + order struct:
      order = (refundBeneficiary, token, inputAmount, to, outputAmount, deadline)
    """
    args = event["args"]
    order_id = args["orderId"]
    order = args["order"]

    print(f"New OrderCreated event. orderId = {order_id.hex()}")
    print("Order struct:", order)

    # Basic check (optional)
    if not is_profitable(chain, order):
        print("Order not considered profitable; skipping.")
        return

    # Convert `bytes20 to` to a Tron base58 address
    raw_addr = b"\x41" + order["to"]
    to_address = b58encode_check(raw_addr).decode()
    print(f"Tron recipient: {to_address}")

    # The amount to deliver on Tron side:
    amount = order["outputAmount"]

    # Example: do a Tron side transfer or swap
    success = await send_usdt(to_address, amount)
    if not success:
        print(f"Tron side transfer failed for order {order_id.hex()}")
        return

    # If Tron side is successful, call `claim(orderId)` on UntronTransfers
    claim_order(web3, contract, account, order_id)

# ------------------------------------------------------------------------------
# Utilities to store last processed block (so we can resume)
# ------------------------------------------------------------------------------
LAST_BLOCK_FILE_TEMPLATE = "backups/last_block_{}.txt"

def save_last_block(chain_name, block_number):
    os.makedirs(os.path.dirname(LAST_BLOCK_FILE_TEMPLATE.format(chain_name)), exist_ok=True)
    with open(LAST_BLOCK_FILE_TEMPLATE.format(chain_name), "w") as f:
        f.write(str(block_number))

def load_last_block(chain_name):
    file_path = LAST_BLOCK_FILE_TEMPLATE.format(chain_name)
    if os.path.exists(file_path):
        with open(file_path, "r") as f:
            return int(f.read().strip())
    return None

# ------------------------------------------------------------------------------
# Listen for new blocks, parse transactions for OrderCreated logs
# ------------------------------------------------------------------------------
async def listen_for_orders(chain):
    web3 = Web3(Web3.HTTPProvider(chain["rpc"]))
    contract = web3.eth.contract(address=chain["contract_address"], abi=untron_abi)

    # The local EVM account used to sign claims
    account = web3.eth.account.from_key(config["ethereum_private_key"])

    # Verify this account is the trusted relayer
    trusted_relayer = contract.functions.trustedRelayer().call()
    if trusted_relayer.lower() != account.address.lower():
        print(f"[{chain['name']}] Account {account.address} is not the trusted relayer {trusted_relayer}")
        exit(1)

    chain_name = chain["name"]
    print(f"[{chain_name}] Listening for OrderCreated events at {chain['contract_address']}")

    # Resume from last saved block or current
    last_block = load_last_block(chain_name) or web3.eth.block_number
    print(f"[{chain_name}] Starting from block {last_block}")

    # Get the event signature for OrderCreated
    order_created_event = contract.events.OrderCreated()
    event_signature = "0x" + Web3.keccak(text="OrderCreated(bytes32,(address,address,uint256,bytes20,uint256,uint256))").hex()

    while True:
        current_block = web3.eth.block_number
        # If new blocks are available, process them in chunks
        if current_block > last_block:
            chunk_size = 1000  # Adjust based on your RPC provider's limits
            from_block = last_block + 1

            while from_block <= current_block:
                to_block = min(from_block + chunk_size - 1, current_block)

                try:
                    logs = web3.eth.get_logs({
                        "fromBlock": from_block,
                        "toBlock": to_block,
                        "address": chain["contract_address"],
                        "topics": [event_signature]  # Filter by OrderCreated event signature
                    })

                    for log in logs:
                        try:
                            event = order_created_event.process_log(log)
                            print(f"[{chain_name}] Found OrderCreated event in block {log['blockNumber']}")
                            await process_order_created_event(web3, contract, account, event, chain)
                        except Exception as e:
                            print(f"[{chain_name}] Error processing log: {e}")
                            continue

                except Exception as e:
                    print(f"[{chain_name}] Error fetching logs for blocks {from_block}-{to_block}: {e}")
                    # If the chunk size is too large, we could implement retry logic with smaller chunks
                    # For now, we'll just continue to the next chunk
                    pass

                from_block = to_block + 1

            last_block = current_block
            save_last_block(chain_name, last_block)

        await asyncio.sleep(2)  # Poll interval in seconds

# ------------------------------------------------------------------------------
# Main entry (launches async tasks for each chain in config)
# ------------------------------------------------------------------------------
async def main():
    print("Initializing UntronTransfers relayer...")
    tasks = []
    for chain in config["chains"]:
        print(f"Launching listener for chain: {chain['name']}")
        tasks.append(asyncio.create_task(listen_for_orders(chain)))

    print("All chain listeners launched.")
    await asyncio.gather(*tasks)

if __name__ == "__main__":
    while True:
        try:
            asyncio.run(main())
        except Exception as e:
            print(f"Top-level error: {e}")