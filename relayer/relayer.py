from tronpy import Tron
from tronpy.providers import HTTPProvider
from tronpy.keys import PrivateKey
from web3 import Web3
import aiohttp
import json
import asyncio
from base58 import b58encode_check
from flask import Flask, request
from threading import Thread

config = json.load(open("config.json"))
client = Tron(HTTPProvider("https://api.trongrid.io", api_key=config["trongrid_api_key"]))
abi = json.load(open("abi.json"))["abi"]

usdt = client.get_contract("TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t")

private_key = PrivateKey(bytes.fromhex(config["tron_private_key"][2:]))
from_address = private_key.public_key.to_base58check_address()

web3 = Web3(Web3.WebsocketProvider(config["rpc"]))
account = web3.eth.account.from_key(config["ethereum_private_key"])
contract = web3.eth.contract(address=config["contract_address"], abi=abi)

def send_erc20_permit(token_address, owner, spender, value, deadline, v, r, s):
    # Create contract instance
    erc20_abi = [{"inputs":[{"internalType":"address","name":"owner","type":"address"},{"internalType":"address","name":"spender","type":"address"},{"internalType":"uint256","name":"value","type":"uint256"},{"internalType":"uint256","name":"deadline","type":"uint256"},{"internalType":"uint8","name":"v","type":"uint8"},{"internalType":"bytes32","name":"r","type":"bytes32"},{"internalType":"bytes32","name":"s","type":"bytes32"}],"name":"permit","outputs":[],"stateMutability":"nonpayable","type":"function"}]
    token_contract = web3.eth.contract(address=token_address, abi=erc20_abi)

    # Build transaction
    nonce = web3.eth.get_transaction_count(account.address)
    tx = token_contract.functions.permit(
        owner, spender, int(value), int(deadline), int(v), bytes.fromhex(r[2:]), bytes.fromhex(s[2:])
    )

    print(tx.call())

    tx = tx.build_transaction({
        'from': account.address,
        'nonce': nonce,
    })

    # Sign and send transaction
    signed_tx = account.sign_transaction(tx)
    tx_hash = web3.eth.send_raw_transaction(signed_tx.rawTransaction)

    # Wait for transaction receipt
    receipt = web3.eth.wait_for_transaction_receipt(tx_hash)
    print(f"ERC20 Permit transaction sent. Transaction hash: {receipt['transactionHash'].hex()}")

    return receipt

def send_gasless_order(user, open_deadline, fill_deadline, nonce, order_data, signature):

    # Prepare the function arguments
    order = {
        'originSettler': contract.address,
        'user': user,
        'nonce': int(nonce),
        'originChainId': 8453,
        'openDeadline': int(open_deadline),
        'fillDeadline': int(fill_deadline),
        'orderData': bytes.fromhex(order_data)
    }
    extra_data = b''  # Empty bytes for the third parameter

    # Build and send the transaction
    tx = contract.functions.openFor(order, bytes.fromhex(signature), extra_data).build_transaction({
        'from': account.address,
        'nonce': web3.eth.get_transaction_count(account.address),
    })

    signed_tx = web3.eth.account.sign_transaction(tx, private_key)
    tx_hash = web3.eth.send_raw_transaction(signed_tx.rawTransaction)

    # Wait for the transaction receipt
    receipt = web3.eth.wait_for_transaction_receipt(tx_hash)

app = Flask(__name__)

@app.route("/permit", methods=["POST"])
def permit():
    token_address = request.args.get("token_address")
    owner = request.args.get("owner")
    spender = request.args.get("spender")
    value = request.args.get("value")
    deadline = request.args.get("deadline")
    v = request.args.get("v")
    r = request.args.get("r")
    s = request.args.get("s")
    try:
        send_erc20_permit(token_address, owner, spender, value, deadline, v, r, s)
        return "OK", 200
    except Exception as e:
        return str(e), 400

@app.route("/gasless_order", methods=["POST"])
def gasless_order():
    user = request.args.get("user")
    open_deadline = request.args.get("open_deadline")
    fill_deadline = request.args.get("fill_deadline")
    nonce = request.args.get("nonce")
    order_data = request.args.get("order_data")
    signature = request.args.get("signature")
    try:
        send_gasless_order(user, open_deadline, fill_deadline, nonce, order_data, signature)
        return "OK", 200
    except Exception as e:
        return str(e), 400

def run_flask():
    app.run(host='0.0.0.0', port=5124)

async def rent_energy(to_address):
    session = aiohttp.ClientSession()
    session.headers["key"] = config["feee_api_key"]

    async with session.get(f"https://feee.io/open/v2/api/query") as resp:
        result = await resp.json()
        print(result)
        balance = result["data"]["trx_money"]
        if balance < 20:
            print("Not enough balance on feee account")
            return False

        print(f"Balance on feee account: {balance} TRX")

    async with session.get(f"https://feee.io/open/v2/order/estimate_energy?from_address={from_address}&to_address={to_address}") as resp:
        result = await resp.json()
        print(result)

    energy_used = str(result["data"]["energy_used"])

    print(f"Transfer to {to_address} will use {energy_used} energy")

    print(f"https://feee.io/open/v2/order/submit?resource_type=1&receive_address={from_address}&resource_value={energy_used}&rent_time_second=600")
    async with session.post(f"https://feee.io/open/v2/order/submit?resource_type=1&receive_address={from_address}&resource_value={energy_used}&rent_time_second=600") as resp:
        result = await resp.json()
        print(result)
    
    order_no = result["data"]["order_no"]
    print("Created order:", order_no)
    
    for i in range(101): # 5 minutes

        if result["data"]["business_status"] >= 3:
            print("Energy rented")
            return True

        await asyncio.sleep(3)

        async with session.post(f"https://feee.io/open/v2/order/query?order_no={order_no}") as resp:
            result = await resp.json()
            print(result)
        
        print(f"Order status: {result['data']['business_status']}")

    async with session.post(f"https://feee.io/open/v2/order/cancel?order_no={order_no}") as resp:
        result = await resp.json()
        print(result)

    print("Canceled order:", result)

    return False

async def send_usdt(to_address, amount):

    # Build the transaction
    txn = (
        usdt.functions.transfer(to_address, amount)
        .with_owner(from_address)
        .fee_limit(28_000_000)
        .build()
        .sign(private_key)
    )

    # TODO: remove
    # success = await rent_energy(to_address)
    # if not success:
    #     print("Failed to rent energy")
    #     return

    # Send the transaction
    result = txn.broadcast().wait()

    return result

async def is_profitable(spent, received):
    return True

async def run_fill(spent, received, instruction):
    if not await is_profitable(spent, received):
        print("Swap is not profitable")
        return

    print("Swap is profitable, performing swap")

    to_address = b58encode_check(received["recipient"][11:]).decode()
    amount = received["amount"]

    print(f"Transfer: {amount} USDT to {to_address}")

    await send_usdt(to_address, amount)

async def reclaim(web3, order_id, contract, account):
    print(f"Reclaiming order {order_id}")
    
    # Send reclaim transaction to the contract
    tx = contract.functions.reclaim(order_id, b'').build_transaction({
        'from': account.address,
        'nonce': web3.eth.get_transaction_count(account.address),
    })
    signed_tx = account.sign_transaction(tx)
    tx_hash = web3.eth.send_raw_transaction(signed_tx.rawTransaction)
    receipt = web3.eth.wait_for_transaction_receipt(tx_hash)
    print(f"Reclaim transaction sent. Transaction hash: {receipt['transactionHash'].hex()}")

async def listen_for_deposits():
    print(f"Listening for Open events on contract {config['contract_address']}")

    last_block = web3.eth.get_block_number()

    while True:
        current_block = web3.eth.get_block_number()
        if current_block > last_block:
            for block_number in range(last_block + 1, current_block + 1):
                block = web3.eth.get_block(block_number, full_transactions=True)
                for tx in block.transactions:
                    if tx['to'] == config['contract_address']:
                        receipt = web3.eth.get_transaction_receipt(tx.hash)
                        for log in receipt.logs:
                            print(log['topics'][0])
                            if log['topics'][0] == web3.keccak(text="Open(bytes32,(address,uint64,uint32,uint32,(address,uint256)[],(bytes32,uint256,bytes32,uint32)[],(uint32,bytes32,bytes)[]))"):
                                event = contract.events.Open().process_log(log)
                                order_id = event['args']['orderId']
                                resolved_order = event['args']['resolvedOrder']
                                
                                print("New Open event detected:", resolved_order)

                                try:
                                    await run_fill(resolved_order['maxSpent'][0], resolved_order['minReceived'][0], resolved_order['fillInstructions'][0])
                                except Exception as e:
                                    print(f"Failed to fill order: {e}")

                                print("Filled order:", order_id.hex())

                                await reclaim(web3, order_id, contract, account) # TODO: only reclaim if successful fill

            last_block = current_block

        await asyncio.sleep(1)  # Check for new blocks every second

async def main():
    print("Initializing relayer")

    # Start Flask server in a separate thread
    flask_thread = Thread(target=run_flask)
    flask_thread.start()

    await listen_for_deposits()

if __name__ == "__main__":
    asyncio.run(main())