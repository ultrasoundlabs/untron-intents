#!/usr/bin/env python3

import argparse
import json
import time
from web3 import Web3
from eth_account import Account
from eth_account.signers.local import LocalAccount

# -----------------------------------------------------------------------
# Replace with your actual UntronTransfers contract address here:
UNTRON_TRANSFERS_ADDRESS = "0xC775104eCD6395bfC70a0860e83148dcb4a3ef8c"

# Standard ERC20 ABI (truncated to just 'approve', 'transferFrom', etc.)
ERC20_ABI = [
    {
        "constant": False,
        "inputs": [
            {"name": "spender", "type": "address"},
            {"name": "value", "type": "uint256"}
        ],
        "name": "approve",
        "outputs": [{"name": "", "type": "bool"}],
        "payable": False,
        "stateMutability": "nonpayable",
        "type": "function"
    },
]

# Minimal ABI for the UntronTransfers contract (only intron(...) needed here).
# Replace with the actual snippet of the ABI covering intron(...) or the full ABI as needed.
UNTRON_ABI = json.load(open("../out/UntronTransfers.json"))["abi"]

def main():
    parser = argparse.ArgumentParser(
        description="Approve ERC20 token and submit intron() to UntronTransfers contract."
    )
    parser.add_argument("--rpc", required=True, help="HTTP RPC endpoint URL")
    parser.add_argument("--private-key", required=True, help="Hex-encoded private key (no 0x prefix required).")
    parser.add_argument("--token-address", required=True, help="ERC20 token address.")
    parser.add_argument("--input-amount", required=True, help="Amount of token to swap (uint256).")
    parser.add_argument("--output-amount", required=True, help="Amount of Tron USDT to receive (uint256).")

    # Optionally accept Tron recipient, or just hardcode for demonstration:
    parser.add_argument("--tron-bytes20", default="0x11223344556677889900AABBCCDDEEFF00112233",
                        help="Tron recipient address in bytes20 hex form.")
    args = parser.parse_args()

    # 1) Connect to RPC via web3
    w3 = Web3(Web3.HTTPProvider(args.rpc))
    if not w3.is_connected():
        raise ConnectionError(f"Could not connect to RPC at {args.rpc}")

    # 2) Load account from private key
    private_key_hex = args.private_key
    if private_key_hex.startswith("0x"):
        private_key_hex = private_key_hex[2:]  # remove 0x if present
    account: LocalAccount = Account.from_key(bytes.fromhex(private_key_hex))

    # 3) Prepare contract instances
    token_contract = w3.eth.contract(address=w3.to_checksum_address(args.token_address), abi=ERC20_ABI)
    untron_contract = w3.eth.contract(address=w3.to_checksum_address(UNTRON_TRANSFERS_ADDRESS), abi=UNTRON_ABI)

    # Convert string input amounts to int
    input_amount = int(args.input_amount)
    output_amount = int(args.output_amount)

    # 4) Approve UntronTransfers contract to transfer tokens
    # Build transaction
    approve_tx = token_contract.functions.approve(
        w3.to_checksum_address(UNTRON_TRANSFERS_ADDRESS),
        input_amount
    ).build_transaction({
        "chainId": w3.eth.chain_id,
        "nonce": w3.eth.get_transaction_count(account.address),
        "from": account.address,
        "gasPrice": w3.eth.gas_price,
    })

    # Sign and send
    signed_approve = account.sign_transaction(approve_tx)
    tx_approve_hash = w3.eth.send_raw_transaction(signed_approve.raw_transaction)
    print(f"Approve transaction sent, hash: {tx_approve_hash.hex()}")
    tx_approve_receipt = w3.eth.wait_for_transaction_receipt(tx_approve_hash)
    print(f"Approve transaction confirmed in block {tx_approve_receipt.blockNumber}")

    # 5) Call intron(...) on UntronTransfers
    # The order needs: refundBeneficiary, token, inputAmount, to, outputAmount, deadline
    # We'll set:
    #   refundBeneficiary = the same account that created the transaction
    #   token = args.token_address
    #   inputAmount = input_amount
    #   to = bytes20 Tron address (passed in from args)
    #   outputAmount = output_amount
    #   deadline = now + 1 day (arbitrary example)
    DEADLINE = int(time.time()) + 24 * 3600

    order = (
        account.address,                   # refundBeneficiary
        w3.to_checksum_address(args.token_address),  # token
        input_amount,                      # inputAmount
        args.tron_bytes20,                 # to (bytes20)
        output_amount,                     # outputAmount
        DEADLINE                           # deadline
    )

    intron_tx = untron_contract.functions.intron(order).build_transaction({
        "chainId": w3.eth.chain_id,
        "nonce": w3.eth.get_transaction_count(account.address),
        "from": account.address,
        "gasPrice": w3.eth.gas_price,
    })

    signed_intron_tx = account.sign_transaction(intron_tx)
    tx_intron_hash = w3.eth.send_raw_transaction(signed_intron_tx.raw_transaction)
    print(f"intron() transaction sent, hash: {tx_intron_hash.hex()}")
    tx_intron_receipt = w3.eth.wait_for_transaction_receipt(tx_intron_hash)
    print(f"intron() transaction confirmed in block {tx_intron_receipt.blockNumber}")

    print("Done!")


if __name__ == "__main__":
    main()