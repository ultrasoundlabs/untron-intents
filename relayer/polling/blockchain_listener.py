import asyncio
import logging
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy import select

from ..blockchain.ethereum import ethereum
from ..blockchain.tron import tron
from ..config import CONFIG, ETHEREUM_POLL_INTERVAL
from ..database import get_session
from ..database.models import Receiver, ProcessedIntent
from ..utils import address_to_topic, save_last_block, load_last_block, is_profitable, decode_tron_address

logger = logging.getLogger(__name__)

async def process_transfer_event(
    chain_name: str,
    event_data: dict,
    session: AsyncSession
) -> bool:
    """
    Process a token transfer event to a receiver contract.
    Returns True if the transfer was successfully processed.
    """
    # Extract transfer details
    to_address = "0x" + event_data["topics"][2][12:].hex()  # Third topic is 'to' address
    input_amount = int.from_bytes(event_data["data"])  # Data field contains amount
    token_address = event_data["address"]
    
    # Check if this is a transfer to one of our receiver contracts
    receiver = await session.execute(
        select(Receiver).where(Receiver.eth_address.ilike(to_address))
    )
    receiver = receiver.scalar_one_or_none()
    
    if not receiver:
        logger.info(f"Transfer to unknown receiver: {to_address}")
        return False  # Not a transfer to our receiver
    
    logger.info(
        f"Processing transfer to receiver {to_address} "
        f"of {input_amount} from token {token_address}"
    )
    
    # Generate a unique intent ID from the transaction hash
    intent_id = event_data["transactionHash"].hex()
    
    # Check if this transfer was already processed using eth_tx_hash as primary key
    existing = await session.get(ProcessedIntent, intent_id)
    if existing is not None:
        logger.info(f"Transfer {intent_id} was already processed (source: {existing.source})")
        return True
    
    # Call the smart contract to determine the recommended output amount
    recommended_output = await ethereum.recommended_output_amount(chain_name, input_amount)
    
    # Convert to Tron address and send USDT using recommended output amount
    tx_hash = await tron.send_usdt(receiver.tron_address, recommended_output)
    
    if not tx_hash:
        logger.error(f"Failed to send USDT for transfer {intent_id}")
        return False
    
    # Call intron() through factory to create the order on-chain
    try:
        tron_bytes = decode_tron_address(receiver.tron_address)
        if not tron_bytes:
            logger.error(f"Failed to decode Tron address {receiver.tron_address} for receiver {to_address}")
            return False
            
        receipt = await ethereum.call_intron(chain_name, tron_bytes)
        print(receipt)
        if not receipt:
            logger.error(f"Failed to call factory intron() for receiver {to_address}")
            return False
            
        logger.info(f"Successfully called factory intron() for receiver {to_address}, tx: {receipt['transactionHash'].hex()}")
    except Exception as e:
        logger.error(f"Error calling factory intron() for receiver {to_address}: {e}")
        return False
    
    # Record the processed intent only after both Tron transfer and intron() succeed
    processed = ProcessedIntent(
        eth_tx_hash=receipt["transactionHash"].hex(),
        tron_tx_hash=tx_hash,
        amount=str(input_amount),  # Store as string to avoid precision loss
        token=token_address,
        source="receiver",
        is_claimed=False  # New transfers start as unclaimed
    )
    session.add(processed)
    await session.commit()
    
    logger.info(f"Successfully processed transfer {intent_id}")
    return True

async def process_order_created_event(
    chain_name: str,
    event_data: dict,
    session: AsyncSession
) -> bool:
    """
    Process an OrderCreated event from the UntronTransfers contract.
    Returns True if the order was successfully processed.
    """
    # Decode event data
    order = ethereum.decode_order_created_event(chain_name, event_data)
    order_id_hex = order.orderId.hex()
    
    logger.info(f"Processing OrderCreated event. orderId = {order_id_hex}")
    
    # Check if this order was already processed using eth_tx_hash as primary key
    existing = await session.get(ProcessedIntent, event_data["transactionHash"].hex())
    if existing is not None:
        if existing.source == "receiver":
            # Order was filled by a receiver, we need to claim it
            if existing.is_claimed:
                logger.info(f"Order {order_id_hex} was already filled and claimed via receiver")
                return True
            else:
                logger.info(f"Order {order_id_hex} was filled by receiver but not claimed, claiming now...")
                eth_tx_hash = await ethereum.claim_order(chain_name, order.orderId)
                if eth_tx_hash:
                    existing.is_claimed = True
                    await session.commit()
                    logger.info(f"Successfully claimed receiver-filled order {order_id_hex}")
            return True
        else:
            # Order was processed directly, no need to do anything
            logger.info(f"Order {order_id_hex} was already processed directly")
            return True
    
    # Get chain configuration
    chain_config = next(c for c in CONFIG["chains"] if c["name"] == chain_name)
    
    # Check if order is profitable using the recommended output amount
    if not is_profitable(chain_config, order.order.token, order.order.inputAmount, order.order.outputAmount):
        logger.info(f"Order {order_id_hex} is not profitable: input {order.order.inputAmount} vs. recommended output {order.order.outputAmount}")
        return False
    
    # Convert to Tron address and send USDT using recommended output amount
    to_address = tron.eth_address_to_tron(order.order.to)
    tx_hash = await tron.send_usdt(to_address, order.order.outputAmount)
    
    if not tx_hash:
        logger.error(f"Failed to send USDT for order {order_id_hex}")
        return False
    
    # Record the processed intent
    processed = ProcessedIntent(
        eth_tx_hash=event_data["transactionHash"].hex(),
        tron_tx_hash=tx_hash,
        amount=str(order.order.inputAmount),  # Store as string to avoid precision loss
        token=order.order.token,  # Use the token from the order
        source="order",
        is_claimed=False  # New orders start as unclaimed
    )
    session.add(processed)
    await session.commit()
    
    # Claim the order on Ethereum
    eth_tx_hash = await ethereum.claim_order(chain_name, order.orderId)
    if eth_tx_hash:
        processed.is_claimed = True
        await session.commit()
        logger.info(f"Successfully processed and claimed order {order_id_hex}")
        return True
    else:
        logger.error(f"Failed to claim order {order_id_hex}")
        return False

async def poll_blockchain_events(chain_name: str) -> None:
    """
    Poll for both OrderCreated events and token transfers in a unified way.
    Processes both types of events for each block before moving to the next.
    """
    web3 = ethereum.get_web3(chain_name)
    transfers_contract = ethereum.get_contract(chain_name)
    
    # Get chain configuration
    chain_config = next(c for c in CONFIG["chains"] if c["name"] == chain_name)
    token_addresses = [
        token_data["address"]
        for token_data in chain_config["tokens"].values()
    ]
    
    # Load the last processed block
    last_block = await load_last_block(f"{chain_name}_events")
    if not last_block:
        last_block = await web3.eth.block_number
        
    logger.info(f"Starting blockchain event polling for chain {chain_name} from block {last_block}")
    
    while True:
        try:
            # Update list of receiver addresses on each iteration
            async with get_session() as session:
                receivers = await session.execute(select(Receiver))
                receiver_addresses = [r.eth_address for r in receivers.scalars().all()]
            
            current_block = await web3.eth.block_number
            
            if current_block > last_block:
                # Process blocks in chunks to avoid timeout
                chunk_size = 1000
                from_block = last_block + 1
                
                while from_block <= current_block:
                    to_block = min(from_block + chunk_size - 1, current_block)
                    
                    try:
                        async with get_session() as session:
                            # Get OrderCreated events
                            try:
                                order_logs = await web3.eth.get_logs({
                                    "fromBlock": from_block,
                                    "toBlock": to_block,
                                    "address": transfers_contract.address,
                                    "topics": [
                                        "0x" + web3.keccak(text="OrderCreated(bytes32,(address,address,uint256,bytes20,uint256,uint256))").hex()
                                    ]
                                })
                                
                            except Exception as e:
                                logger.error(f"Error getting order logs: {e}")
                                order_logs = []
                            
                            # Get Transfer events to our receiver contracts
                            try:
                                if receiver_addresses:
                                    transfer_logs = await web3.eth.get_logs({
                                        "fromBlock": from_block,
                                        "toBlock": to_block,
                                        "address": token_addresses,
                                        "topics": [
                                            "0x" + web3.keccak(text="Transfer(address,address,uint256)").hex(),
                                            None,  # from address (any)
                                            [address_to_topic(addr) for addr in receiver_addresses]  # to addresses (our receivers)
                                        ]
                                    })
                                else:
                                    transfer_logs = []
                            except Exception as e:
                                logger.error(f"Error getting transfer logs: {e}")
                                transfer_logs = []
                            
                            # Log raw event data for debugging
                            if order_logs:
                                logger.info(f"First order log: {order_logs[0]}")
                            if transfer_logs:
                                logger.info(f"First transfer log: {transfer_logs[0]}")
                                
                            # Process all events in this block range
                            for log in order_logs:
                                try:
                                    await process_order_created_event(chain_name, log, session)
                                except Exception as e:
                                    logger.error(f"Error processing order log: {e}")
                                    continue
                                    
                            for log in transfer_logs:
                                try:
                                    await process_transfer_event(chain_name, log, session)
                                except Exception as e:
                                    logger.error(f"Error processing transfer log: {e}")
                                    continue
                            
                        # Update last processed block
                        last_block = to_block
                        await save_last_block(f"{chain_name}_events", last_block)
                        from_block = to_block + 1
                        
                    except Exception as e:
                        logger.error(f"Error processing blocks {from_block}-{to_block}: {e}")
                        await asyncio.sleep(ETHEREUM_POLL_INTERVAL)
                        continue
            
            await asyncio.sleep(ETHEREUM_POLL_INTERVAL)
            
        except Exception as e:
            logger.error(f"Error in polling loop: {e}")
            await asyncio.sleep(ETHEREUM_POLL_INTERVAL)

async def start_blockchain_listeners() -> None:
    """Start blockchain event listeners for all configured chains."""
    listeners = [
        poll_blockchain_events(chain["name"])
        for chain in CONFIG["chains"]
    ]
    await asyncio.gather(*listeners)
