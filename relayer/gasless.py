import argparse
import json
from eth_account import Account
from eth_account.messages import encode_structured_data
from web3 import Web3
from base58 import b58decode_check
from eth_abi import encode
import requests

def encode_order(order):
    return encode(["(address,address,uint256,uint64,uint32,uint32,bytes)"], [order])

def sign_gasless_order(private_key, order_id, order_data, domain_separator, intent_typehash):
    struct_hash = Web3.keccak(intent_typehash + order_data + order_id)

    message = b"\x19\x01" + domain_separator + struct_hash

    return Account.signHash(Web3.keccak(message), private_key)

def permit(web3, private_key, sender_private_key, input_token, input_amount, spender):
    # Get the token contract
    token_contract = web3.eth.contract(address=input_token, abi=[
        {
            "constant": True,
            "inputs": [],
            "name": "name",
            "outputs": [{"name": "", "type": "string"}],
            "type": "function"
        },
        {
            "constant": True,
            "inputs": [],
            "name": "symbol",
            "outputs": [{"name": "", "type": "string"}],
            "type": "function"
        },
        {
            "constant": True,
            "inputs": [],
            "name": "decimals",
            "outputs": [{"name": "", "type": "uint8"}],
            "type": "function"
        },
        {
            "constant": True,
            "inputs": [{"name": "owner", "type": "address"}],
            "name": "nonces",
            "outputs": [{"name": "", "type": "uint256"}],
            "type": "function"
        },
        {
            "constant": True,
            "inputs": [],
            "name": "DOMAIN_SEPARATOR",
            "outputs": [{"name": "", "type": "bytes32"}],
            "type": "function"
        },
        {
            "inputs": [
                {"name": "owner", "type": "address"},
                {"name": "spender", "type": "address"},
                {"name": "value", "type": "uint256"},
                {"name": "deadline", "type": "uint256"},
                {"name": "v", "type": "uint8"},
                {"name": "r", "type": "bytes32"},
                {"name": "s", "type": "bytes32"}
            ],
            "name": "permit",
            "outputs": [],
            "type": "function"
        }
    ])
    
    # Get the current nonce for the owner
    owner = Account.from_key(private_key).address
    nonce = token_contract.functions.nonces(owner).call()
    
    # Get the domain separator
    domain_separator = token_contract.functions.DOMAIN_SEPARATOR().call()
    
    # Define the permit type hash
    PERMIT_TYPEHASH = Web3.keccak(text="Permit(address owner,address spender,uint256 value,uint256 nonce,uint256 deadline)")
    
    # Set deadline to 1 hour from now
    deadline = web3.eth.get_block('latest')['timestamp'] + 3600
    
    # Construct the permit data
    permit_data = encode(
        ['bytes32', 'address', 'address', 'uint256', 'uint256', 'uint256'],
        [PERMIT_TYPEHASH, owner, spender, input_amount, nonce, deadline]
    )
    
    # Construct the full message to sign
    message = b"\x19\x01" + domain_separator + Web3.keccak(permit_data)
    
    # Sign the message
    signed_message = Account.signHash(Web3.keccak(message), private_key)
    
    # signature components and deadline
    v = signed_message.v
    r = signed_message.r.to_bytes(32, byteorder='big')
    s = signed_message.s.to_bytes(32, byteorder='big')
    
    print(requests.post("http://localhost:3000/intents/permit", json={
        "tokenAddress": input_token,
        "owner": Account.from_key(private_key).address,
        "spender": spender,
        "value": str(input_amount),
        "deadline": str(deadline),
        "v": str(v),
        "r": "0x" + r.hex(),
        "s": "0x" + s.hex()
    }).json())

def main():
    parser = argparse.ArgumentParser(description="Create and sign a gasless order for Untron Intents")
    parser.add_argument("--rpc", required=True, help="RPC URL")
    parser.add_argument("--private-key", required=True, help="Private key of the user")
    parser.add_argument("--sender-private-key", required=True, help="Private key of the tx sender")
    parser.add_argument("--user", required=True, help="User address")
    parser.add_argument("--open-deadline", type=int, required=True, help="Open deadline timestamp")
    parser.add_argument("--fill-deadline", type=int, required=True, help="Fill deadline timestamp")
    parser.add_argument("--input-token", required=True, help="Input token address")
    parser.add_argument("--input-amount", type=int, required=True, help="Input amount")
    parser.add_argument("--to", required=True, help="Recipient Tron address (21 bytes)")
    parser.add_argument("--output-amount", type=int, required=True, help="Output amount")
    parser.add_argument("--origin-settler", required=True, help="Origin settler contract address")
    
    args = parser.parse_args()

    web3 = Web3(Web3.HTTPProvider(args.rpc))
    chain_id = web3.eth.chain_id

    # permit(web3, args.private_key, args.sender_private_key, args.input_token, args.input_amount, args.origin_settler)

    contract = web3.eth.contract(args.origin_settler, abi=json.load(open("abi.json"))["abi"])
    domain_separator = contract.functions.DOMAIN_SEPARATOR().call()
    intent_typehash = contract.functions.INTENT_TYPEHASH().call()
    nonce = contract.functions.gaslessNonces(args.user).call()

    print(chain_id, domain_separator.hex(), intent_typehash.hex(), nonce)

    to = b58decode_check(args.to)
    
    intent = (
        args.user,
        args.input_token,
        args.input_amount,
        to,
        args.output_amount
    )

    order = (args.origin_settler, args.user, nonce, chain_id, args.open_deadline, args.fill_deadline, encode(["(address,address,uint256,bytes21,uint256)"], [intent]))
    print(order)

    signed_message = sign_gasless_order(
        args.private_key,
        Web3.keccak(encode_order(order)),
        order[-1],
        domain_separator,
        intent_typehash
    )

    v = signed_message.v
    r = signed_message.r.to_bytes(32, byteorder='big')
    s = signed_message.s.to_bytes(32, byteorder='big')
    sig = encode(["(uint8,bytes32,bytes32)"], [(v, r, s)])
    print(sig.hex())

    print(requests.post("http://localhost:3000/intents/gasless-order", json={
        "user": args.user,
        "openDeadline": str(args.open_deadline),
        "fillDeadline": str(args.fill_deadline),
        "nonce": str(nonce),
        "orderData": encode_order(order).hex(),
        "signature": "0x" + sig.hex()
    }).json())

if __name__ == "__main__":
    main()