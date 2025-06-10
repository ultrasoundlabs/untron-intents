import os
import argparse
from web3 import Web3
from dotenv import load_dotenv

def sweep_balance(w3, from_account, to_address):
    """
    Sweeps the entire native currency (e.g., ETH) balance from one account to another.

    This function calculates the total balance of the `from_account`, estimates the
    transaction fee (gas), and sends the remaining amount to the `to_address`.

    Parameters:
    - w3: An initialized Web3 instance connected to a node.
    - from_account: The account object (with private key) whose balance will be swept.
    - to_address: The checksummed address that will receive the funds.

    Returns:
    A tuple containing a boolean indicating success and the transaction hash or an
    error message.
    """
    # Get the current balance of the account we are sweeping from.
    # The balance is returned in Wei, the smallest unit of Ether.
    balance = w3.eth.get_balance(from_account.address)
    print(f"Balance: {w3.from_wei(balance, 'ether')} ETH")

    # Get the current recommended gas price from the network.
    gas_price = w3.eth.gas_price * 3

    # try:
    #     # On L2s like Base, we estimate gas to account for L1 data fees.
    #     # A simple transfer's gas cost isn't affected by the value, so we use 0.
    #     gas_limit = w3.eth.estimate_gas({
    #         'to': to_address,
    #         'from': from_account.address,
    #         'value': 0,
    #         'gasPrice': gas_price
    #     })
    # except Exception as e:
    #     # Fallback for chains where estimation might fail.
    #     print(f"Could not estimate gas, falling back to 21000. Error: {e}")
    #     gas_limit = 21000
    gas_limit = 21000

    amount_to_send = int(balance * 0.9)

    # If the balance is less than or equal to the transaction fee, we can't send anything.
    if amount_to_send <= 0 or gas_limit * gas_price * 20 > amount_to_send:
        print("Balance is too low to cover transaction fees. No transaction will be sent.")
        return False, "Balance too low."

    print(f"Amount to send: {w3.from_wei(amount_to_send, 'ether')} ETH")

    # Build the transaction dictionary.
    transaction = {
        'to': to_address,
        'value': amount_to_send,
        'gas': gas_limit,
        'gasPrice': gas_price,
        'nonce': w3.eth.get_transaction_count(from_account.address),
        'chainId': w3.eth.chain_id # Including chainId is a good practice to prevent replay attacks on other chains.
    }

    # Sign the transaction with the sender's private key.
    signed_tx = w3.eth.account.sign_transaction(transaction, from_account.key)

    # Send the signed transaction to the network.
    print("Sending transaction...")
    tx_hash = w3.eth.send_raw_transaction(signed_tx.raw_transaction)

    # Wait for the transaction to be confirmed.
    receipt = w3.eth.wait_for_transaction_receipt(tx_hash)
    
    return True, receipt.transactionHash.hex()

def main():
    """
    Main function to parse arguments and orchestrate the sweeping process across multiple chains.
    """
    # --- Argument Parsing ---
    # We use argparse to create a command-line interface for our script.
    # This makes it easy to pass the required private key when running the script.
    parser = argparse.ArgumentParser(description="Sweep all ETH from an account to a destination address across multiple chains.")
    parser.add_argument("private_key", help="The private key of the account to sweep funds from.")
    args = parser.parse_args()

    # --- Configuration Loading ---
    # Load environment variables from the .env file.
    load_dotenv()

    # Get the destination address from environment variables.
    # IMPORTANT: Add the address you want to send funds TO in your .env file.
    # DESTINATION_ADDRESS="your_destination_address_here"
    destination_private_key_str = os.getenv("PRIVATE_KEY")
    if not destination_private_key_str:
        raise ValueError("PRIVATE_KEY not found in .env file.")
    
    destination_private_key = Web3().eth.account.from_key(destination_private_key_str)
    destination_address = destination_private_key.address

    # Get the RPC URLs from environment variables.
    rpc_urls_str = os.getenv("RPC_URLS")
    if not rpc_urls_str:
        raise ValueError("RPC_URLS not found in .env file.")
    
    rpc_urls = [url.strip() for url in rpc_urls_str.split(',')]

    # --- Main Loop ---
    for rpc_url in rpc_urls:
        print(f"\n--- Processing chain: {rpc_url} ---")
        try:
            # Connect to the blockchain node.
            w3 = Web3(Web3.HTTPProvider(rpc_url))
            
            # The destination address needs to be in checksum format for web3.py
            destination_address = w3.to_checksum_address(destination_address)
            print(f"Destination address: {destination_address}")

            # Create an account object from the provided private key.
            # This is the account we are sweeping funds FROM.
            from_account = w3.eth.account.from_key(args.private_key)
            print(f"Sweeping funds from: {from_account.address}")
            
            # Call the sweep function.
            success, result = sweep_balance(w3, from_account, destination_address)

            if success:
                print(f"Successfully swept funds. Transaction hash: {result}")
            else:
                print(f"Could not sweep funds: {result}")

        except Exception as e:
            print(f"An error occurred on chain {rpc_url}: {e}")

if __name__ == "__main__":
    main() 