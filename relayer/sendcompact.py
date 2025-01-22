#!/usr/bin/env python3

import argparse
import json
from web3 import Web3
from eth_account import Account
from eth_account.signers.local import LocalAccount
from base58 import b58decode_check

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
        description="Approve ERC20 token and submit compactUsdt() to UntronTransfers contract."
    )
    parser.add_argument("--rpc", required=True, help="HTTP RPC endpoint URL")
    parser.add_argument("--private-key", required=True, help="Hex-encoded private key (no 0x prefix required).")
    parser.add_argument("--input-amount", required=True, help="Amount of USDT to swap (uint256).")
    parser.add_argument("--output-amount", required=True, help="Amount of Tron USDT to receive (uint256).")
    parser.add_argument("--token-address", required=True, help="Address of the ERC20 token to swap.")
    parser.add_argument("--tron-address", required=True, help="Tron recipient address in Tron format (base58).")
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

    # 5) Call compactUsdt(...) on UntronTransfers
    # Pack the data into bytes32:
    # - First 6 bytes: input amount
    # - Next 6 bytes: output amount
    # - Last 20 bytes: Tron address
    
    # Convert Tron address from base58 format and remove network byte
    tron_bytes = b58decode_check(args.tron_address)
    tron_bytes20 = tron_bytes[1:]  # Remove first byte (network identifier)
    
    # Convert amounts to 6-byte representations
    input_bytes = input_amount.to_bytes(6, 'big')
    output_bytes = output_amount.to_bytes(6, 'big')
    
    # Concatenate all parts into 32 bytes
    swap_data = input_bytes + output_bytes + tron_bytes20
    print(f"swap_data: {swap_data.hex()}")

    compact_tx = untron_contract.functions.compactUsdt(swap_data).build_transaction({
        "chainId": w3.eth.chain_id,
        "nonce": w3.eth.get_transaction_count(account.address),
        "from": account.address,
        "gasPrice": w3.eth.gas_price,
    })

    signed_compact_tx = account.sign_transaction(compact_tx)
    tx_compact_hash = w3.eth.send_raw_transaction(signed_compact_tx.raw_transaction)
    print(f"compactUsdt() transaction sent, hash: {tx_compact_hash.hex()}")
    tx_compact_receipt = w3.eth.wait_for_transaction_receipt(tx_compact_hash)
    print(f"compactUsdt() transaction confirmed in block {tx_compact_receipt.blockNumber}")

    print("Done!")

if __name__ == "__main__":
    main()