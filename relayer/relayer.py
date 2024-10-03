from tronpy import Tron
from tronpy.keys import PrivateKey
from web3 import Web3
import aiohttp
import json
import asyncio
from base58 import b58encode_check

client = Tron()
config = json.load(open("config.json"))
abi = json.load(open("abi.json"))["abi"]

usdt = client.get_contract("TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t")

private_key = PrivateKey(bytes.fromhex(config["tron_private_key"]))
from_address = private_key.public_key.to_base58check_address()

async def rent_energy(to_address):
    session = aiohttp.ClientSession()
    session.headers["key"] = config["feee_api_key"]

    async with session.get(f"https://feee.io/open/v2/api/query") as resp:
        result = await resp.json()
        balance = result["data"]["trx_money"]
        if balance < 20:
            print("Not enough balance on feee account")
            return False

        print(f"Balance on feee account: {balance} TRX")

    async with session.get(f"https://feee.io/open/v2/order/estimate_energy?from_address={from_address}&to_address={to_address}") as resp:
        result = await resp.json()

    energy_used = result["data"]["energy_used"]

    print(f"Transfer to {to_address} will use {energy_used} energy")

    async with session.post(f"https://feee.io/open/v2/order/submit?receive_address={from_address}&resource_value={energy_used}&rent_duration=10&rent_time_unit=m") as resp:
        result = await resp.json()
    
    order_no = result["data"]["order_no"]
    print("Created order:", order_no)
    
    for i in range(101): # 5 minutes

        if result["data"]["business_status"] >= 3:
            print("Energy rented")
            return True

        await asyncio.sleep(3)

        async with session.post(f"https://feee.io/open/v2/order/query?order_no={order_no}") as resp:
            result = await resp.json()
        
        print(f"Order status: {result['data']['business_status']}")

    async with session.post(f"https://feee.io/open/v2/order/cancel?order_no={order_no}") as resp:
        result = await resp.json()

    print("Canceled order:", result)

    return False

async def send_usdt(to_address, amount):

    # Build the transaction
    txn = (
        usdt.functions.transfer(to_address, amount)
        .with_owner(from_address)
        .fee_limit(5_000_000)
        .build()
        .sign(private_key)
    )

    success = await rent_energy(to_address)
    if not success:
        print("Failed to rent energy")
        return

    # Send the transaction
    result = txn.broadcast().wait()

    return result

async def is_profitable(spent, received):
    return True # TODO: change this

async def run_fill(spent, received, instruction):
    if not is_profitable(spent, received):
        print("Swap is not profitable")
        return

    print("Swap is profitable, performing swap")

    to_address = b58encode_check(received["recipient"][11:])
    amount = received["amount"]

    print(f"Transfer: {amount} USDT to {to_address}")

    await send_usdt(to_address, amount)

async def reclaim(order_id, contract, account):
    print(f"Reclaiming order {order_id}")
    
    # Send reclaim transaction to the contract
    tx = await contract.functions.reclaim(order_id, b'').build_transaction({
        'from': account.address,
        'nonce': await contract.web3.eth.get_transaction_count(account.address),
    })
    signed_tx = account.sign_transaction(tx)
    tx_hash = await contract.web3.eth.send_raw_transaction(signed_tx.rawTransaction)
    receipt = await contract.web3.eth.wait_for_transaction_receipt(tx_hash)
    print(f"Reclaim transaction sent. Transaction hash: {receipt['transactionHash'].hex()}")

async def listen_for_deposits(web3, account, contract_address):
    contract = web3.eth.contract(address=contract_address, abi=abi)
    open_filter = contract.events.Open.create_filter(fromBlock='latest')

    print(f"Listening for Open events on contract {contract_address}")

    while True:
        async for event in open_filter.get_new_entries():
            order_id = event['args']['orderId']
            resolved_order = event['args']['resolvedOrder']
            
            print("New Open event detected:", resolved_order)

            await run_fill(resolved_order['maxSpent'][0], resolved_order['minReceived'][0], resolved_order['fillInstructions'][0])

            print("Filled order:", order_id.hex())

            await reclaim(order_id, contract, account)

        await asyncio.sleep(1)  # Poll for new events every second

async def main():
    print("Initializing relayer")

    for network in config["networks"]:
        web3 = Web3(Web3.HTTPProvider(network["rpc"]))
        account = web3.eth.account.from_key(config["ethereum_private_key"])
        await listen_for_deposits(web3, network["contract_address"], account)

if __name__ == "__main__":
    asyncio.run(main())