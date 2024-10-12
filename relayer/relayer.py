from tronpy import Tron
from tronpy.providers import HTTPProvider
from tronpy.keys import PrivateKey
from web3 import Web3
import aiohttp
import json
import asyncio
from base58 import b58encode_check
import os
from eth_abi import encode

config = json.load(open("config.json"))
client = Tron(HTTPProvider("https://api.trongrid.io", api_key=config["trongrid_api_key"]))
abi = json.load(open("abi.json"))["abi"]

usdt = client.get_contract("TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t")

private_key = PrivateKey(bytes.fromhex(config["tron_private_key"][2:]))
from_address = private_key.public_key.to_base58check_address()

web3 = Web3(Web3.WebsocketProvider(config["rpc"]))
account = web3.eth.account.from_key(config["ethereum_private_key"])
contract = web3.eth.contract(address=config["contract_address"], abi=abi)

async def send_usdt(to_address, amount):
    txn = (
        usdt.functions.transfer(to_address, amount)
        .with_owner(from_address)
        .fee_limit(28_000_000)
        .build()
        .sign(private_key)
    )
    return txn.broadcast().wait()

async def is_profitable(spent, received):
    # TODO: Implement actual profitability check
    return True

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
    tx_hash = web3.eth.send_raw_transaction(signed_tx.rawTransaction)
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