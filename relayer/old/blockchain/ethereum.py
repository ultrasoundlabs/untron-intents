import logging
from typing import Optional, Dict, Any
from web3.contract import AsyncContract
from eth_account.account import Account
from web3 import AsyncWeb3, AsyncHTTPProvider

from ..config import CONFIG, GAS_PRICE_MULTIPLIER, MAX_GAS_PRICE, ABIS

logger = logging.getLogger(__name__)

class EthereumClient:
    def __init__(self):
        """Initialize Ethereum client and load contracts."""
        # Initialize web3 instances and contracts for each chain
        self.web3_by_chain = {}
        self.contracts_by_chain = {}
        self.factory_by_chain = {}
        self.account = Account.from_key(CONFIG["ethereum_private_key"])
        
        for chain in CONFIG["chains"]:
            web3 = AsyncWeb3(AsyncHTTPProvider(chain["rpc"]))
            
            # Initialize UntronTransfers contract
            contract = web3.eth.contract(
                address=chain["transfers_contract_address"],
                abi=ABIS["transfers"]
            )
            
            # Initialize factory contract for this chain
            factory = web3.eth.contract(
                address=chain["receiver_factory_address"],
                abi=ABIS["factory"]
            )
            
            self.web3_by_chain[chain["name"]] = web3
            self.contracts_by_chain[chain["name"]] = contract
            self.factory_by_chain[chain["name"]] = factory
            
            logger.info(
                f"Initialized Ethereum client for chain {chain['name']} "
                f"at {chain['transfers_contract_address']}, "
                f"factory at {chain['receiver_factory_address']}"
            )

    def get_web3(self, chain_name: str) -> AsyncWeb3:
        """Get Web3 instance for a specific chain."""
        if chain_name not in self.web3_by_chain:
            raise ValueError(f"Unknown chain: {chain_name}")
        return self.web3_by_chain[chain_name]

    def get_contract(self, chain_name: str) -> AsyncContract:
        """Get UntronTransfers contract for a specific chain."""
        if chain_name not in self.contracts_by_chain:
            raise ValueError(f"Unknown chain: {chain_name}")
        return self.contracts_by_chain[chain_name]

    def get_factory(self, chain_name: str) -> AsyncContract:
        """Get ReceiverFactory contract for a specific chain."""
        if chain_name not in self.factory_by_chain:
            raise ValueError(f"Unknown chain: {chain_name}")
        return self.factory_by_chain[chain_name]

    async def generate_receiver_address(self, tron_bytes: bytes) -> str:
        """
        Generate receiver address from Tron address bytes using the factory's
        deterministic address generation (CREATE2).
        """
        factory = self.get_factory(CONFIG["chains"][0]["name"]) # TODO: more flexible logic
        logger.info(f"Generating receiver address for Tron bytes: {tron_bytes.hex()}")
        address = await factory.functions.generateReceiverAddress(tron_bytes).call()
        logger.info(f"Generated receiver address: {address}")
        return address

    async def call_intron(self, chain_name: str, tron_bytes: bytes) -> Dict[str, Any]:
        """
        Call intron() through the factory contract, which handles receiver deployment
        and proper parameter passing.
        """
        factory = self.get_factory(chain_name)
        logger.info(f"Calling intron via factory for Tron bytes: {tron_bytes.hex()}")
        
        try:
            nonce = await self.web3_by_chain[chain_name].eth.get_transaction_count(self.account.address)
            gas_price = await self.web3_by_chain[chain_name].eth.gas_price
            chain_id = await self.web3_by_chain[chain_name].eth.chain_id
            
            # Build intron transaction through factory
            tx = await factory.functions.intron(tron_bytes).build_transaction({
                "chainId": chain_id,
                "gas": 500000,  # Fixed gas limit for intron calls
                "gasPrice": min(
                    int(gas_price * GAS_PRICE_MULTIPLIER),
                    MAX_GAS_PRICE
                ),
                "nonce": nonce,
            })
            
            # Sign and send transaction
            signed_tx = self.account.sign_transaction(tx)
            tx_hash = await self.web3_by_chain[chain_name].eth.send_raw_transaction(signed_tx.raw_transaction)
            
            # Wait for receipt
            receipt = await self.web3_by_chain[chain_name].eth.wait_for_transaction_receipt(tx_hash)
            logger.info(f"Factory intron() call successful, tx hash: {receipt['transactionHash'].hex()}")
            
            return receipt
            
        except Exception as e:
            logger.error(f"Error calling factory intron(): {e}")
            raise

    def decode_order_created_event(self, chain_name: str, event_data: Dict[str, Any]) -> Dict[str, Any]:
        """
        Decode OrderCreated event data into a more usable format.
        Returns a dictionary with parsed event data.
        """
        print(event_data)
        contract = self.get_contract(chain_name)
        event = contract.events.OrderCreated()
        decoded = event.process_log(event_data)
        print(decoded)
        
        return decoded.args

    async def claim_order(self, chain_name: str, order_id: bytes) -> Optional[str]:
        """
        Call claim() on UntronTransfers contract for a specific order.
        Returns transaction hash if successful.
        """
        logger.info(f"Claiming order {order_id.hex()} on chain {chain_name}")
        
        web3 = self.get_web3(chain_name)
        contract = self.get_contract(chain_name)
        
        try:
            # Build transaction
            tx = await contract.functions.claim(order_id).build_transaction({
                "from": self.account.address,
                "nonce": await web3.eth.get_transaction_count(self.account.address),
                "gas": 3000000,
                "gasPrice": min(
                    int(await web3.eth.gas_price * GAS_PRICE_MULTIPLIER),
                    MAX_GAS_PRICE
                ),
            })
            
            # Sign and send transaction
            signed_tx = self.account.sign_transaction(tx)
            tx_hash = await web3.eth.send_raw_transaction(signed_tx.raw_transaction)
            
            # Wait for receipt
            receipt = await web3.eth.wait_for_transaction_receipt(tx_hash)
            tx_hash_hex = receipt["transactionHash"].hex()
            
            logger.info(f"Successfully claimed order. Tx hash: {tx_hash_hex}")
            return tx_hash_hex
            
        except Exception as e:
            logger.error(f"Error claiming order: {e}")
            return None

    async def recommended_output_amount(self, chain_name: str, input_amount: int) -> int:
        """
        Call the recommendedOutputAmount function on the UntronTransfers contract to
        determine the output amount based on the provided input amount.
        """
        contract = self.get_contract(chain_name)
        try:
            output_amount = await contract.functions.recommendedOutputAmount(input_amount).call()
            logger.info(f"Recommended output amount for input {input_amount} is {output_amount}")
            return output_amount
        except Exception as e:
            logger.error(f"Error calling recommendedOutputAmount: {e}")
            raise

# Create singleton instance
ethereum = EthereumClient()
