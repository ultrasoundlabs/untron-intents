# pragma version 0.4.0
# @license MIT

"""
@title Untron Transfers
@notice An intent-based bridge for sending ERC20 tokens as USDT on Tron
@dev This version of the contract uses a trusted relayer to handle the swaps.
     In the future versions, Untron's ZK engine will be used for permissionless solving.
"""

# Interface for ERC20 tokens
interface IERC20:
    # Function to transfer tokens from one address to another with approval
    # Required to accept tokens from users who want to swap them
    def transferFrom(_from: address, to: address, amount: uint256) -> bool: nonpayable
    # Function to transfer tokens from the contract to another address
    # Required to send tokens to relayers or refund users
    def transfer(to: address, amount: uint256) -> bool: nonpayable

# Order struct for tracking yet unfilled swap orders
struct Order:
    # Address of the refund beneficiary.
    # This is the address that will receive the funds
    # if the order is not filled before the deadline.
    refundBeneficiary: address

    # Address of the token to swap from.
    # Must be a deployed ERC20 token on the deployed chain.
    token: address

    # Amount of the token to swap from.
    inputAmount: uint256

    # Tron address to receive Tron USDT.
    to: bytes20

    # Amount of the Tron USDT to receive.
    outputAmount: uint256

    # Deadline for the order.
    # If the order is not filled before the deadline,
    # the funds can be refunded to the refundBeneficiary.
    deadline: uint256

# Event for when an order is created
# Listened to by the relayers to track new orders
event OrderCreated:
    # ID of the created order
    orderId: bytes32
    # Details of the created order
    order: Order

# Event for when an order is filled or cancelled
# Listened to by the relayers to track orders that are no longer active
event OrderCleared:
    # ID of the order that is no longer active
    orderId: bytes32

# Addresses of USDT and USDC tokens on the deployed chain.
# They're used in compactUsdt/compactUsdc functions
# to provide a data-saving interface for efficient swaps on the L2s.
usdt: immutable(address)
usdc: immutable(address)

# Mapping of order IDs to orders.
# Only contains orders that have not been filled yet.
orders: public(HashMap[bytes32, Order])

# Mapping of addresses to their nonce.
# Used to generate unique order IDs.
nonces: public(HashMap[address, uint256])

# Address of the trusted relayer.
# Only this address can call claim() to fill orders.
trustedRelayer: public(address)

recommendedFixedFee: uint256
recommendedPercentFee: uint256

@external
def setRecommendedFee(fixedFee: uint256, percentFee: uint256):
    """
    @notice Sets the recommended fee for the contract.
    @param fixedFee The fixed fee.
    @param percentFee The percent fee.
    """
    assert msg.sender == self.trustedRelayer
    self.recommendedFixedFee = fixedFee
    self.recommendedPercentFee = percentFee

@internal
@view
def _recommendedOutputAmount(inputAmount: uint256) -> uint256:
    """
    @notice Calculates the recommended output amount for a given input amount.
    @param inputAmount The input amount.
    @return The recommended output amount.
    """
    return inputAmount * (10000 - self.recommendedPercentFee) // 10000 - self.recommendedFixedFee

@external
@view
def recommendedOutputAmount(inputAmount: uint256) -> uint256:
    """
    @notice Calculates the recommended output amount for a given input amount.
    @param inputAmount The input amount.
    @return The recommended output amount.
    """
    return self._recommendedOutputAmount(inputAmount)

@deploy
def __init__(_usdt: address, _usdc: address):
    """
    @notice Initializes the contract with USDT and USDC addresses and the trusted relayer.
    @param _usdt Address of the USDT token.
    @param _usdc Address of the USDC token.
    """
    # Store the USDT token address as an immutable
    # USDT address needs to be stored immutably to enable data-efficient compact swaps from USDT
    usdt = _usdt
    # Store the USDC token address as an immutable
    # USDC address needs to be stored immutably to enable data-efficient compact swaps from USDC
    usdc = _usdc
    # Set the initial trusted relayer address
    # A trusted relayer is required to process cross-chain swaps until ZK proofs are implemented
    self.trustedRelayer = msg.sender

@internal
def _orderId(order: Order, nonce: uint256) -> bytes32:
    """
    @notice Generates a unique order ID for a given order and nonce.
    @param order The order to generate an ID for.
    @param nonce The nonce (initiator's order counter) for the order.
    @return The unique order ID.
    """
    # Generate a unique hash by combining the encoded order data with the nonce
    # Each order needs a unique identifier to track its state and prevent replay attacks
    # We could technically store just the bools whether this order took place,
    # but then it would require to post the entire order onchain twice,
    # while we want to minimize data usage on L2s.
    # Hence why we store it on a relatively cheap L2 state
    # and clean it up after the order is filled.
    return sha256(concat(abi_encode(order), convert(nonce, bytes32)))

@internal
def _intron(order: Order):
    """
    @notice An internal entry-point function for "intron" (In[to] Tron) swaps.
    @param order The requested order.
    @dev This function is used by intron() and compactSwap() functions.
    """
    # Transfer tokens from the sender to this contract
    # The contract needs to hold the tokens until the cross-chain swap is completed
    extcall IERC20(order.token).transferFrom(msg.sender, self, order.inputAmount)
    # Generate a unique order ID using the order data and sender's nonce
    # Each order needs a unique identifier for efficient tracking and claiming in the storage
    orderId: bytes32 = self._orderId(order, self.nonces[msg.sender])
    # Store the order in the orders mapping
    # Order details must be stored to allow efficient claiming or refunding
    self.orders[orderId] = order
    # Increment the sender's nonce to prevent order ID collisions
    # Nonce must increase to ensure each order has a unique ID
    self.nonces[msg.sender] += 1

    log OrderCreated(orderId, order)

@external
def intron(order: Order):
    """
    @notice An entry-point function for "intron" (In[to] Tron) swaps.
    @param order The requested order.
    @dev Whenever feasible, prefer using compact swap functions instead.
         This will reduce footprint of the transaction and save L2 gas.
    """
    # Call the internal intron function to process the order
    # Provides a public interface for creating full-featured swap orders
    self._intron(order)

@external
def cancel(orderId: bytes32):
    """
    @notice Cancels an expired order.
    @param orderId The ID of the order to cancel.
    @dev This function is used by the initiator of the order
         to cancel it if it expires.
    """
    # Retrieve the order from storage
    # Need to access order details to verify cancellation conditions
    order: Order = self.orders[orderId]
    # Verify the order has expired
    # Orders can only be cancelled after their deadline to prevent premature cancellations
    assert order.deadline < block.timestamp
    # Return the tokens to the refund beneficiary
    # Tokens must be returned to the beneficiary when order is cancelled
    extcall IERC20(order.token).transfer(order.refundBeneficiary, order.inputAmount)
    # Clear the order from storage
    # Cancelled orders must be removed to prevent double-spending
    # and to save storage space
    self.orders[orderId] = empty(Order)

    # Log the order as cleared
    # This is used by the relayers to track orders that are no longer active
    log OrderCleared(orderId)

@external
def claim(orderId: bytes32):
    """
    @notice Claims funds for a filled order.
    @param orderId The ID of the order to claim.
    @dev This function is used by the trusted relayer.
    """
    # Verify the caller is the trusted relayer
    # Only the trusted relayer can claim funds for cross-chain swaps
    assert msg.sender == self.trustedRelayer

    # Retrieve the order from storage
    # Need to access order details to process the claim
    order: Order = self.orders[orderId]
    # Verify the order has expired
    # Orders can only be claimed before their deadline,
    # otherwise they can only be refunded to the initiator
    assert order.deadline >= block.timestamp

    # Transfer the tokens to the relayer
    # Relayer receives the tokens as compensation for executing the cross-chain swap
    extcall IERC20(order.token).transfer(msg.sender, order.inputAmount)
    # Clear the order from storage
    # Claimed orders must be removed to prevent double-spending
    self.orders[orderId] = empty(Order)

    # Log the order as cleared
    # This is used by the relayers to track orders that are no longer active
    log OrderCleared(orderId)

@external
def setTrustedRelayer(newRelayer: address):
    """
    @notice Sets a new trusted relayer.
    @param newRelayer The new relayer address.
    @dev This function is used to set a new relayer.
         Only the current relayer can set a new one.
    """
    # Verify the caller is the current trusted relayer
    # Only the current relayer should be able to transfer their role
    assert msg.sender == self.trustedRelayer
    # Update the trusted relayer address
    # New relayer address must be stored to enable future claims
    self.trustedRelayer = newRelayer

@internal
def _compactSwap(token: address, swapData: bytes32):
    """
    @notice An internal entry-point function for "compact" swaps.
    @param token The address of the token to swap.
    @param swapData The compressed 32-byte data for the swap.
                    It contains the input amount (6 bytes), 
                    output amount in Tron USDT (6 bytes),
                    and the recipient Tron address (20 bytes compressed).
                    Deadline is hardcoded to 1 day.
                    While 6 bytes are reasonable for 6-decimal USDT,
                    some tokens, especially 18-decimal ones,
                    might require more bytes for the input amount.
                    In this case, consider using intron() instead.
    @dev This function is used by compactSwap() function.
    """
    # Extract the input amount from the first 6 bytes
    # This would only work good for token amounts which would fit into 6 bytes.
    # This is not a problem for 6-decimal USDT, but might be for 18-decimal tokens.
    # In this case, consider using intron() instead.
    inputAmount: uint256 = convert(swapData, uint256) >> 208
    # Extract the output amount from the next 6 bytes
    # Output amount is in Tron USDT, so it's limited to ~281m USDT, which is reasonable
    outputAmount: uint256 = convert(swapData, uint256) >> 160
    # Extract the output amount from the other data
    outputAmount &= convert(max_value(uint48), uint256)
    # if output amount is 0, use recommended output amount
    if outputAmount == 0:
        outputAmount = self._recommendedOutputAmount(inputAmount)
    # Extract the Tron address from the remaining 20 bytes
    # Recipient address must be decoded from the compact format to create the order
    to: bytes20 = convert(convert(convert(swapData, uint256) << 96, bytes32), bytes20)
    # Create an order struct with a 1-day deadline
    # It is created from the decoded compact data for data-efficient processing
    order: Order = Order(refundBeneficiary=msg.sender, token=token, inputAmount=inputAmount, to=to, outputAmount=outputAmount, deadline=block.timestamp + 86400)
    # Process the order using the internal intron function
    # Created order must be processed like any other swap order
    self._intron(order)

@external
def compactSwap(token: address, swapData: bytes32):
    """
    @notice An entry-point function for "compact" swaps.
    @param token The address of the token to swap.
    @param swapData The compressed 32-byte data for the swap.
    @dev This function uses _compactSwap() internally.
    """
    # Call the internal compact swap function with the provided token and data
    # Provides a public interface for creating data-efficient swap orders with custom tokens
    self._compactSwap(token, swapData)

@external
def compactUsdt(swapData: bytes32):
    """
    @notice An entry-point function for "compact" swaps from USDT.
    @param swapData The compressed 32-byte data for the swap.
    @dev This function uses _compactSwap() internally.
    """
    # Call the internal compact swap function with the USDT token address
    # Provides a data-efficient public interface specifically for USDT swaps
    self._compactSwap(usdt, swapData)

@external
def compactUsdc(swapData: bytes32):
    """
    @notice An entry-point function for "compact" swaps from USDC.
    @param swapData The compressed 32-byte data for the swap.
    @dev This function uses _compactSwap() internally.
    """
    # Call the internal compact swap function with the USDC token address
    # Provides a data-efficient public interface specifically for USDC swaps
    self._compactSwap(usdc, swapData)