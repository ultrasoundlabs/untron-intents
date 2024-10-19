from tronpy import Tron
from tronpy.providers import HTTPProvider
from tronpy.keys import PrivateKey
from web3 import Web3
from decimal import Decimal
import json
import asyncio
from base58 import b58encode_check
import os
import requests
from requests.packages.urllib3.exceptions import InsecureRequestWarning

# Disable the warning that appears when making unverified HTTPS requests
requests.packages.urllib3.disable_warnings(InsecureRequestWarning)

config = json.load(open("config.json"))
client = Tron(HTTPProvider("https://api.trongrid.io", api_key=config["trongrid_api_key"]))
abi = json.load(open("abi.json"))["abi"]

sunswap_v2 = client.get_contract("TXF1xDbVGdxFGbovmmmXvBGu8ZiE3Lq4mR")

private_key = PrivateKey(bytes.fromhex(config["tron_private_key"][2:]))
from_address = private_key.public_key.to_base58check_address()

web3 = Web3(Web3.HTTPProvider(config["rpc"]))
account = web3.eth.account.from_key(config["ethereum_private_key"])
contract = web3.eth.contract(address=config["contract_address"], abi=abi)

async def send_usdt(to_address, amount):
    # custom technique allowing for cheaper transfers
    # than just TRC20 transfer() call
    txn = (
        sunswap_v2.functions.swapTokensForExactTokens(
            amount,
            999999999999999999999999,
            [
                "TPXxtMtQg95VX8JRCiQ5SXqSeHjuNaMsxi",
                "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t"
            ],
            to_address,
            9999999999
        )
        .with_owner(private_key.public_key.to_base58check_address())
        .fee_limit(2_000_000)
        .build()
        .sign(private_key)
    )
    return txn.broadcast().wait()

async def is_profitable(spent, received):
    response = requests.get("https://untron.finance/intents/assets", verify=False)
    assets = response.json()

    spent_asset = next((asset for asset in assets if asset["contractAddress"] == spent["token"]), None)

    if not spent_asset:
        print("Asset not found in assets")
        return False
    decimals = int(spent_asset["decimals"])

    usd_rate = 1  # TODO: fix this

    response = requests.get("https://untron.finance/intents/information", verify=False)
    flat_fee = Decimal(response.json()["fees"]["flatFee"])
    percent_fee = Decimal(response.json()["fees"]["pctFee"])
    max_output_amount = Decimal(response.json()["maxOutputAmount"])
    print(flat_fee, percent_fee, max_output_amount)

    if Decimal(received["amount"]) / Decimal('1e6') > max_output_amount * 3:  # max_output_amount is 1/3 of the liquidity
        print("Received amount is greater than max output amount")
        return False

    # Convert spent amount to its actual value considering decimals
    spent_amount = Decimal(spent["amount"]) / (Decimal(10) ** decimals)
    
    # Calculate receive amount in USDT (6 decimals) at which the swap is profitable
    max_receive = (spent_amount * Decimal(usd_rate) * (Decimal('1') - percent_fee)) * Decimal('1e6')

    print(spent_amount, max_receive)

    # Compare with received amount (already in USDT decimals)
    return Decimal(received["amount"]) <= max_receive - (flat_fee * Decimal('1e6'))

async def run_fill(spent, received, instruction):
    if not await is_profitable(spent, received):
        print("Swap is not profitable")
        return False

    print("Swap is profitable, performing swap")
    to_address = b58encode_check(received["recipient"][11:]).decode()
    amount = received["amount"]
    print(f"Transfer: {amount} USDT to {to_address}")

    try:
        result = await send_usdt(to_address, amount)
        print(f"USDT transfer result: {result}")
        return True
    except Exception as e:
        print(f"Failed to send USDT: {e}")
        return False

async def reclaim(web3, order_id, resolved_order, contract, account):
    print(f"Reclaiming order {order_id.hex()}")
    
    tx = contract.functions.reclaim(resolved_order, b'').build_transaction({
        'from': account.address,
        'nonce': web3.eth.get_transaction_count(account.address),
    })
    signed_tx = account.sign_transaction(tx)
    tx_hash = web3.eth.send_raw_transaction(signed_tx.raw_transaction)
    receipt = web3.eth.wait_for_transaction_receipt(tx_hash)
    print(f"Reclaim transaction sent. Transaction hash: {receipt['transactionHash'].hex()}")

LAST_BLOCK_FILE = 'last_block.txt'

def save_last_block(block_number):
    with open(LAST_BLOCK_FILE, 'w') as f:
        f.write(str(block_number))

def load_last_block():
    if os.path.exists(LAST_BLOCK_FILE):
        with open(LAST_BLOCK_FILE, 'r') as f:
            return int(f.read().strip())
    return None

async def process_open_event(event):
    order_id = event['args']['orderId']
    resolved_order = event['args']['resolvedOrder']
    
    print("New Open event detected:", resolved_order)

    try:
        fill_success = await run_fill(resolved_order['maxSpent'][0], resolved_order['minReceived'][0], resolved_order['fillInstructions'][0])
        if fill_success:
            print(f"Successfully filled order: {order_id.hex()}")
            await reclaim(web3, order_id, resolved_order, contract, account)
        else:
            print(f"Failed to fill order: {order_id.hex()}")
    except Exception as e:
        print(f"Error processing order {order_id.hex()}: {e}")

async def listen_for_deposits():
    print(f"Listening for Open events on contract {config['contract_address']}")

    last_block = load_last_block() or web3.eth.get_block_number()
    print(f"Starting from block {last_block}")

    while True:
        current_block = web3.eth.get_block_number()
        if current_block > last_block:
            for block_number in range(last_block + 1, current_block + 1):
                block = web3.eth.get_block(block_number, full_transactions=True)
                for tx in block.transactions:
                    if tx['to'] == config['contract_address']:
                        receipt = web3.eth.get_transaction_receipt(tx.hash)
                        for log in receipt.logs:
                            if log['topics'][0] == web3.keccak(text="Open(bytes32,(address,uint64,uint32,uint32,(address,uint256)[],(bytes32,uint256,bytes32,uint32)[],(uint32,bytes32,bytes)[]))"):
                                event = contract.events.Open().process_log(log)
                                await process_open_event(event)

            last_block = current_block
            save_last_block(last_block)

        await asyncio.sleep(1)  # Check for new blocks every second

async def main():
    print("Initializing relayer")
    await listen_for_deposits()

if __name__ == "__main__":
    asyncio.run(main())