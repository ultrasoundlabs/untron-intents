import argparse
import json
from eth_account import Account
from web3 import Web3
from base58 import b58decode_check
from eth_abi import encode
from eth_account.messages import encode_typed_data

def encode_order(order):
    return encode(["(address,address,uint256,uint64,uint32,uint32,bytes)"], [order])

def sign_gasless_order(account, domain_separator, intent_typehash, intent, order_id):
    input_typehash = Web3.keccak(text="Input(address token,uint256 amount)")
    encoded_inputs = Web3.keccak(b"".join([Web3.keccak(encode(["(bytes32,address,uint256)"], [(input_typehash, input[0], input[1])])) for input in intent[1]]))
    struct_hash = Web3.keccak(encode(["(bytes32,address,bytes32,bytes32,uint256,bytes32)"], [(intent_typehash, account.address, encoded_inputs, intent[2], intent[3], order_id)]))

    message = b"\x19\x01" + domain_separator + struct_hash
    message_hash = Web3.keccak(message)
    signature = account.unsafe_sign_hash(message_hash)

    return signature

def permit(web3, account, input_token, input_amount, spender):

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
    
    nonce = token_contract.functions.nonces(account.address).call()
    
        # Set deadline to 1 hour from now
    deadline = web3.eth.get_block('latest')['timestamp'] + 3600

    types = {
        "EIP712Domain": [
            {"name": "name", "type": "string"},
            {"name": "version", "type": "string"},
            {"name": "chainId", "type": "uint256"},
            {"name": "verifyingContract", "type": "address"},
        ],
        "Permit": [
            {"name": "owner", "type": "address"},
            {"name": "spender", "type": "address"},
            {"name": "value", "type": "uint256"},
            {"name": "nonce", "type": "uint256"},
            {"name": "deadline", "type": "uint256"},
            ]
        }
    
    # Prepare the message
    permit_data = {
        "owner": account.address,
        "spender": spender,
        "value": input_amount,
        "nonce": nonce,
        "deadline": deadline
    }

    version = 0
    while True:
        version += 1

        # Prepare the domain
        domain = {
            "name": token_contract.functions.name().call(),
            "version": str(version),
            "chainId": web3.eth.chain_id,
            "verifyingContract": input_token
        }
    
        message = {
            "types": types,
            "primaryType": "Permit",
            "domain": domain,
            "message": permit_data,
        }

        # Encode the message
        encoded_message = encode_typed_data(full_message=message)
        
        # Sign the message
        signed_message = account.sign_message(encoded_message)
        
        # Extract signature components
        v = signed_message.v
        r = signed_message.r.to_bytes(32, byteorder='big')
        s = signed_message.s.to_bytes(32, byteorder='big')

        # Call the permit function
        try:
            token_contract.functions.permit(
                account.address,
                spender,
                input_amount,
                deadline,
                v,
                r,
                s
            ).call({'from': spender})
            print(message)
            print(f"Permit function call emulation successful with version {version}")
            break
        except:
            print(f"{version} is invalid for the token")

    return deadline, v, r, s

def main():
    parser = argparse.ArgumentParser(description="Create and sign a gasless order for Untron Intents")
    parser.add_argument("--rpc", required=True, help="RPC URL")
    parser.add_argument("--private-key", required=True, help="Private key of the user")
    parser.add_argument("--sender-private-key", required=True, help="Private key of the tx sender")
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

    account = Account.from_key(args.private_key)

    deadline, v, r, s = permit(web3, account, args.input_token, args.input_amount, args.origin_settler)

    with open("abi.json") as f:
        abi = json.load(f)["abi"]
    contract = web3.eth.contract(args.origin_settler, abi=abi)
    domain_separator = contract.functions.DOMAIN_SEPARATOR().call()
    intent_typehash = contract.functions.INTENT_TYPEHASH().call()
    nonce = contract.functions.gaslessNonces(account.address).call()

    print(chain_id, domain_separator.hex(), intent_typehash.hex(), nonce)

    to = b58decode_check(args.to)
    
    intent = (
        account.address,
        [(args.input_token, args.input_amount)],
        to,
        args.output_amount
    )

    print(intent)

    encoded_intent = encode(["(address,(address,uint256)[],bytes21,uint256)"], [intent])
    order = (args.origin_settler, account.address, nonce, chain_id, args.open_deadline, args.fill_deadline, encoded_intent)

    # Call resolveFor function to get ResolvedCrossChain order struct
    resolved_order = contract.functions.resolveFor(order, b"").call()
    print(resolved_order)

    # Generate order ID by keccak hashing the serialized order
    order_id = Web3.keccak(encode(["(address,uint256,uint256,uint256,(address,uint256)[],(bytes32,uint256,bytes32,uint256)[],(uint256,bytes32,bytes)[])"], [resolved_order]))

    print(f"Generated order ID: {order_id.hex()}")

    signed_message = sign_gasless_order(
        account,
        domain_separator,
        intent_typehash,
        intent,
        order_id
    )

    sig = signed_message.r.to_bytes(32, byteorder='big') + signed_message.s.to_bytes(32, byteorder='big') + signed_message.v.to_bytes(1)
    print(sig.hex())

    # Run call emulation first
    try:
        result = contract.functions.permitAndOpenFor(order, sig, b"", [deadline], [v], [r], [s]).call({
            'from': Account.from_key(args.sender_private_key).address,
        })
        print("Call emulation successful. Result:", result)
    except Exception as e:
        print("Call emulation failed:", str(e))
        return

    # Build the transaction
    tx = contract.functions.permitAndOpenFor(order, sig, b"", [deadline], [v], [r], [s]).build_transaction({
        'from': Account.from_key(args.sender_private_key).address,
        'nonce': web3.eth.get_transaction_count(Account.from_key(args.sender_private_key).address),
        'gas': 3000000,  # Adjust as needed
        'gasPrice': web3.eth.gas_price,
    })

    # Sign and send the transaction
    signed_tx = Account.from_key(args.sender_private_key).sign_transaction(tx)
    tx_hash = web3.eth.send_raw_transaction(signed_tx.raw_transaction)
    
    # Wait for the transaction receipt
    tx_receipt = web3.eth.wait_for_transaction_receipt(tx_hash)
    print(tx_receipt)

if __name__ == "__main__":
    main()