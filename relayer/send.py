import json
import sys
from web3 import Web3
from eth_account import Account
from eth_abi import encode

# Replace with your Base RPC URL
rpc_url = 'https://mainnet.base.org'  # Example RPC URL
w3 = Web3(Web3.HTTPProvider(rpc_url))

# Input data provided
input_data = json.loads(sys.argv[1])

# Replace with your private key
private_key = json.load(open('config.json'))['ethereum_private_key']  # The private key corresponding to userAddress
sender_address = Account.from_key(private_key).address
user_address = w3.to_checksum_address(input_data['userAddress'])

# Load contract ABIs
with open('../contracts/out/UntronIntents.sol/UntronIntents.json', 'r') as f:
    untron_intents_abi = json.load(f)["abi"]

with open('../contracts/out/IERC20Permit.sol/IERC20Permit.json', 'r') as f:
    ierc20_permit_abi = json.load(f)["abi"]

# Contract addresses (replace with actual deployed addresses)
untron_intents_address = json.load(open('config.json'))['chains'][0]['contract_address']
token_address = w3.to_checksum_address(input_data['intent']['inputs'][0]['token'])

# Create contract instances
untron_intents = w3.eth.contract(address=untron_intents_address, abi=untron_intents_abi)
token_contract = w3.eth.contract(address=token_address, abi=ierc20_permit_abi)

# Prepare permit parameters
permit = input_data['permit']
permit_deadline = int(permit['deadline'])
permit_amount = int(input_data['intent']['inputs'][0]['amount'])
permit_v = permit['v']
permit_r = permit['r']
permit_s = permit['s']

# Get the current nonce for the user
nonce = w3.eth.get_transaction_count(sender_address)

# Build the permit transaction
permit_tx = token_contract.functions.permit(
    user_address,
    untron_intents_address,
    permit_amount,
    permit_deadline,
    permit_v,
    permit_r,
    permit_s
).build_transaction({
    'from': sender_address,
    'nonce': nonce,
    'gas': 100000,
    'gasPrice': w3.to_wei('0.1', 'gwei'),
    'chainId': input_data['chainId']
})

# # Sign and send the permit transaction
# signed_permit_tx = w3.eth.account.sign_transaction(permit_tx, private_key=private_key)
# permit_tx_hash = w3.eth.send_raw_transaction(signed_permit_tx.raw_transaction)
# print(f'Permit transaction sent: {permit_tx_hash.hex()}')

# # Wait for the permit transaction to be mined
# w3.eth.wait_for_transaction_receipt(permit_tx_hash)
# print('Permit transaction mined')

# Update nonce after the permit transaction
# nonce += 1

# Compute the order ID first
resolved_order = (
    user_address,  # user
    w3.eth.chain_id,  # originChainId
    2**32 - 1,  # openDeadline (type(uint32).max in Solidity)
    int(input_data['fillDeadline']),  # fillDeadline
    [(x['token'], int(x['amount'])) for x in input_data['intent']['inputs']],  # maxSpent
    [(bytes.fromhex("000000000000000000000041a614f803b6fd780986a42c78ec9c7f77e6ded13c"), int(input_data['intent']['outputAmount']), bytes.fromhex(input_data['intent']['to'][2:]), 0x800000c3)],  # minReceived
    [(0x800000c3, bytes.fromhex("0000000000000000000000000000000000000000"), b'')]  # fillInstructions
)
print(resolved_order)
order_id = w3.keccak(encode(["(address,uint64,uint32,uint32,(address,uint256)[],(bytes32,uint256,bytes32,uint32)[],(uint32,bytes32,bytes)[])"], [resolved_order]))
print(order_id.hex())

# Update intent to include orderId
intent = (
    sender_address,  # refundBeneficiary
    [(x['token'], int(x['amount'])) for x in input_data['intent']['inputs']],  # inputs
    bytes.fromhex(input_data['intent']['to'][2:]),  # to
    int(input_data['intent']['outputAmount']),  # outputAmount
    order_id  # orderId
)

# Update encoding to match the full struct
encoded_intent = encode(["(address,(address,uint256)[],bytes21,uint256,bytes32)"], [intent])

# Prepare the GaslessCrossChainOrder struct
# Note: You may need to adjust the struct according to the contract's ABI and expected types
gasless_cross_chain_order = (
    untron_intents_address,  # originSettler
    user_address,  # user
    int(input_data['nonce']),  # nonce
    int(input_data['chainId']),  # originChainId
    int(input_data['openDeadline']),  # openDeadline
    int(input_data['fillDeadline']),  # fillDeadline
    encoded_intent  # orderData (encoded Intent struct)
)

# Prepare the signature
signature = bytes.fromhex(input_data['signature'][2:])

# Since fillerData is empty in this case
filler_data = b''

# Build the openFor transaction
open_for_tx = untron_intents.functions.openFor(
    gasless_cross_chain_order,
    signature,
    filler_data
).build_transaction({
    'from': sender_address,
    'nonce': nonce,
    'gas': 500000,
    'gasPrice': w3.to_wei('2', 'gwei'),
    'chainId': input_data['chainId']
})

# Sign and send the openFor transaction
signed_open_for_tx = w3.eth.account.sign_transaction(open_for_tx, private_key=private_key)
open_for_tx_hash = w3.eth.send_raw_transaction(signed_open_for_tx.raw_transaction)
print(f'openFor transaction sent: {open_for_tx_hash.hex()}')

# Wait for the openFor transaction to be mined
w3.eth.wait_for_transaction_receipt(open_for_tx_hash)
print('openFor transaction mined')