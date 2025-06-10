import os
import json
from web3 import Web3
from dotenv import load_dotenv

# This function will deploy a contract.
# It takes the web3 instance, the account to deploy from, the contract's ABI and bytecode,
# and any constructor arguments the contract needs.
def deploy_contract(w3, account, contract_abi, contract_bytecode, args=[]):
    """
    Deploys a smart contract to the blockchain.

    This function simplifies the deployment process by handling the creation
    of the contract object, transaction building, signing, and sending.

    Parameters:
    - w3: An initialized Web3 instance connected to an Ethereum node.
    - account: The account object from which the deployment transaction will be sent.
             This account needs to have enough funds to cover gas fees.
    - contract_abi: The Application Binary Interface (ABI) of the contract. The ABI
                    defines how to interact with the contract's functions.
    - contract_bytecode: The compiled bytecode of the contract, which is the code
                         that will be stored on the blockchain.
    - args: A list of arguments to be passed to the contract's constructor upon
            deployment. Defaults to an empty list if there are no arguments.

    Returns:
    The deployed contract object, which can be used to interact with the
    contract on the blockchain.
    """
    # Create a contract object from the ABI and bytecode.
    # This object is a Python representation of your smart contract.
    contract = w3.eth.contract(abi=contract_abi, bytecode=contract_bytecode)

    # Build the transaction.
    # This transaction will deploy the contract. If the contract has a constructor
    # that takes arguments, they are passed here.
    # 'nonce' is a number that is used once for each transaction from an account.
    # It prevents replay attacks. We get the latest nonce for our account.
    transaction = contract.constructor(*args).build_transaction({
        'from': account.address,
        'nonce': w3.eth.get_transaction_count(account.address),
    })

    # Sign the transaction with the private key of the account.
    # This proves that we authorize this transaction.
    signed_txn = w3.eth.account.sign_transaction(transaction, private_key=account.key)

    print("Deploying contract...")
    # Send the raw signed transaction to the network.
    tx_hash = w3.eth.send_raw_transaction(signed_txn.raw_transaction)
    print(f"Transaction hash: {tx_hash.hex()}")

    # Wait for the transaction to be mined.
    # This means the transaction has been included in a block on the blockchain.
    tx_receipt = w3.eth.wait_for_transaction_receipt(tx_hash)
    print(f"Contract deployed at: {tx_receipt.contractAddress}")

    # Return the deployed contract object, now with an address.
    return w3.eth.contract(address=tx_receipt.contractAddress, abi=contract_abi)

def main():
    """
    Main function to run the deployment script.

    This script orchestrates the deployment of the UntronReceiver and
    ReceiverFactory contracts across multiple blockchain networks.
    It reads configuration from a .env file, iterates through specified
    RPC endpoints, and executes the deployment and ownership transfer
    for each chain.
    """
    # Load environment variables from a .env file in the same directory.
    # This is a secure way to handle sensitive data like private keys.
    load_dotenv()

    # Get the private key from environment variables.
    # IMPORTANT: Create a .env file and add your private key like this:
    # PRIVATE_KEY="your_private_key_here"
    private_key = os.getenv("PRIVATE_KEY")
    if not private_key:
        raise ValueError("PRIVATE_KEY not found in .env file")

    # Get the RPC URLs from environment variables.
    # In your .env file, add a comma-separated list of RPC URLs:
    # RPC_URLS="http://127.0.0.1:8545,https://mainnet.infura.io/v3/your_project_id"
    rpc_urls_str = os.getenv("RPC_URLS")
    if not rpc_urls_str:
        raise ValueError("RPC_URLS not found in .env file. Please add a comma-separated list of RPCs.")
    
    rpc_urls = [url.strip() for url in rpc_urls_str.split(',')]

    # The address that will be the new owner of the ReceiverFactory.
    new_owner_address = "0xf178905915f55dd34Ba1980942354dc64109118F"

    # --- Load Contract Artifacts ---
    # We read the compiled contract information (ABI and bytecode) from the JSON files
    # produced by your smart contract compilation tool (e.g., Foundry, Hardhat).
    with open("out/UntronReceiver.json") as f:
        receiver_artifact = json.load(f)
    with open("out/ReceiverFactory.json") as f:
        factory_artifact = json.load(f)

    receiver_abi = receiver_artifact["abi"]
    receiver_bytecode = receiver_artifact["bytecode"]
    factory_abi = factory_artifact["abi"]
    factory_bytecode = factory_artifact["bytecode"]


    # --- Iterate and Deploy on each Chain ---
    for rpc_url in rpc_urls:
        print(f"\n--- Processing chain: {rpc_url} ---")
        try:
            # Connect to an Ethereum node (the chain).
            w3 = Web3(Web3.HTTPProvider(rpc_url))
            
            if not w3.is_connected():
                print(f"Failed to connect to {rpc_url}")
                continue

            # Get the account object from the private key.
            # This account will be used to send transactions.
            account = w3.eth.account.from_key(private_key)
            print(f"Using deployer account: {account.address}")

            # 1. Deploy the UntronReceiver contract
            print("\nDeploying UntronReceiver...")
            receiver_contract = deploy_contract(w3, account, receiver_abi, receiver_bytecode)
            receiver_contract_address = receiver_contract.address

            # 2. Deploy the ReceiverFactory contract
            # The ReceiverFactory constructor requires the address of the UntronReceiver contract.
            print("\nDeploying ReceiverFactory...")
            factory_contract = deploy_contract(w3, account, factory_abi, factory_bytecode, args=[receiver_contract_address])

            # 3. Transfer ownership of the ReceiverFactory
            print(f"\nTransferring ownership of ReceiverFactory to {new_owner_address}...")
            
            # Build the transaction to call the 'transfer_ownership' function.
            transfer_tx = factory_contract.functions.transfer_ownership(new_owner_address).build_transaction({
                'from': account.address,
                'nonce': w3.eth.get_transaction_count(account.address),
                'gas': 200000, # You might need to adjust gas
                'gasPrice': w3.eth.gas_price
            })

            # Sign and send the ownership transfer transaction.
            signed_transfer_tx = w3.eth.account.sign_transaction(transfer_tx, private_key=account.key)
            transfer_tx_hash = w3.eth.send_raw_transaction(signed_transfer_tx.raw_transaction)
            print(f"Ownership transfer transaction hash: {transfer_tx_hash.hex()}")
            
            # Wait for the transaction to be mined.
            w3.eth.wait_for_transaction_receipt(transfer_tx_hash)
            print("Ownership transfer complete.")
            
            # Verify new owner
            # A simple way to verify is to call the 'owner()' public variable/function if it exists
            owner = factory_contract.functions.owner().call()
            print(f"Verified new owner: {owner}")


        except Exception as e:
            print(f"An error occurred on chain {rpc_url}: {e}")

if __name__ == "__main__":
    main()
