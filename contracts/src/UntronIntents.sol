// SPDX-License-Identifier: BUSL-1.1
pragma solidity ^0.8.20;

import "./interfaces/IUntronIntents.sol";
import "@openzeppelin/contracts/token/ERC20/IERC20.sol";

/// @title Basic ERC-7683 logic for Untron Intents
/// @author Ultrasound Labs
/// @dev This contracts implements basic logic for EVM->TRON intent-based transfers.
///      It does not implement fill verification logic, which must be implemented by the inheriting contract.
abstract contract UntronIntents is IUntronIntents {
    /// @dev A mapping of user addresses to their gasless nonce.
    /// @dev Used to prevent replay attacks for gasless orders.
    mapping(address => uint256) public gaslessNonces;
    /// @dev A mapping of order IDs to their corresponding intents.
    mapping(bytes32 => Intent) internal _intents;
    /// @dev A mapping of order IDs to their fill deadlines.
    mapping(bytes32 => uint32) public fillDeadlines;

    /// @inheritdoc IUntronIntents
    function intents(bytes32 orderId) external view returns (Intent memory) {
        return _intents[orderId];
    }

    /// @dev The USDT TRC20 token on Tron
    bytes32 internal constant USDT_TRC20 = bytes32(bytes21(0x41a614f803b6fd780986a42c78ec9c7f77e6ded13c));
    /// @dev The DestinationSettler contract address on Tron
    bytes32 internal constant TRON_SETTLEMENT_ADDRESS = bytes32(bytes21(0)); // TODO:
    /// @dev The TRON SLIP-44 coin ID (used as a chain ID)
    uint32 internal constant TRON_COINID = 0x800000c3;

    /// @notice Get this network's chain ID
    /// @return uint64 The chain ID
    function _chainId() internal view returns (uint64) {
        uint64 chainId;
        assembly {
            chainId := chainid()
        }
        return chainId;
    }

    /// @notice Resolve an intent into a resolved cross-chain order
    /// @param intent The intent to resolve
    /// @return ResolvedCrossChainOrder The resolved cross-chain order
    function _resolve(Intent memory intent, uint32 fillDeadline)
        internal
        view
        returns (ResolvedCrossChainOrder memory)
    {
        // Intron swap has one input in an ERC20 token on the source chain
        Input[] memory maxSpent = new Input[](1);
        maxSpent[0] = Input(intent.inputToken, intent.inputAmount);

        // And one output in USDT TRC20 on Tron
        Output[] memory minReceived = new Output[](1);
        minReceived[0] = Output(USDT_TRC20, intent.outputAmount, bytes32(intent.to), TRON_COINID);

        // The single fill instruction is to send the output to the settlement address on Tron
        FillInstruction[] memory fillInstructions = new FillInstruction[](1);
        fillInstructions[0] = FillInstruction(TRON_COINID, TRON_SETTLEMENT_ADDRESS, "");

        return ResolvedCrossChainOrder({
            user: msg.sender,
            originChainId: _chainId(),
            openDeadline: uint32(block.timestamp),
            fillDeadline: fillDeadline,
            maxSpent: maxSpent,
            minReceived: minReceived,
            fillInstructions: fillInstructions
        });
    }

    /// @inheritdoc IOriginSettler
    function open(OnchainCrossChainOrder calldata order) external override {
        // Check that the order is not expired
        require(order.fillDeadline > block.timestamp, "Order expired");

        // Order ID is the hash of the order (OnchainCrossChainOrder or GaslessCrossChainOrder)
        bytes32 orderId = keccak256(abi.encode(order));

        // Decode the intent from the order data
        Intent memory intent = abi.decode(order.orderData, (Intent));

        // Transfer the input token from the user to this contract
        require(
            IERC20(intent.inputToken).transferFrom(msg.sender, address(this), intent.inputAmount), "Insufficient funds"
        );

        // Resolve the intent into a cross-chain order for fillers to use when filling the order
        ResolvedCrossChainOrder memory resolvedOrder = _resolve(intent, order.fillDeadline);

        // Store the intent in the intents mapping
        _intents[orderId] = intent;
        // Store the fill deadline in the fillDeadlines mapping
        fillDeadlines[orderId] = order.fillDeadline;

        // Emit an Open event
        emit Open(orderId, resolvedOrder);
    }

    /// @inheritdoc IOriginSettler
    function resolve(OnchainCrossChainOrder calldata order) external view returns (ResolvedCrossChainOrder memory) {
        // Decode the intent from the order data
        Intent memory intent = abi.decode(order.orderData, (Intent));

        // Resolve the intent into a cross-chain order for fillers to use when filling the order
        return _resolve(intent, order.fillDeadline);
    }

    /// @inheritdoc IOriginSettler
    function openFor(GaslessCrossChainOrder calldata order, bytes calldata signature, bytes calldata) external {
        // Check that the order was made for this contract
        require(order.originSettler == address(this), "Invalid origin settler");
        // Check that the order's open deadline is not expired
        require(order.openDeadline > block.timestamp, "Open deadline expired");
        // Check that the order's fill deadline is not expired
        require(order.fillDeadline > block.timestamp, "Order expired");
        // Check that the order's nonce is valid
        require(order.nonce == gaslessNonces[order.user], "Invalid nonce");
        // Increment the user's gasless nonce
        gaslessNonces[order.user]++;

        // Deserialize the signature
        (uint8 v, bytes32 r, bytes32 s) = abi.decode(signature, (uint8, bytes32, bytes32));

        // Reconstruct the message that was signed
        bytes32 messageHash = keccak256(abi.encode(order));

        // Recover the signer's address
        address signer = ecrecover(messageHash, v, r, s);

        // Verify that the signature was created by order.user
        require(signer == order.user, "Invalid signature");

        // Order ID is the hash of the order (OnchainCrossChainOrder or GaslessCrossChainOrder)
        bytes32 orderId = keccak256(abi.encode(order));

        // Decode the intent from the order data
        Intent memory intent = abi.decode(order.orderData, (Intent));

        // Transfer the input token from the user to this contract
        require(
            IERC20(intent.inputToken).transferFrom(order.user, address(this), intent.inputAmount), "Insufficient funds"
        );

        // Store the intent in the intents mapping
        _intents[orderId] = intent;
        // Store the fill deadline in the fillDeadlines mapping
        fillDeadlines[orderId] = order.fillDeadline;

        // Resolve the intent into a cross-chain order for fillers to use when filling the order
        ResolvedCrossChainOrder memory resolvedOrder = _resolve(intent, order.fillDeadline);

        // Emit an Open event
        emit Open(orderId, resolvedOrder);
    }

    /// @inheritdoc IOriginSettler
    function resolveFor(GaslessCrossChainOrder calldata order, bytes calldata)
        external
        view
        returns (ResolvedCrossChainOrder memory)
    {
        // Decode the intent from the order data
        Intent memory intent = abi.decode(order.orderData, (Intent));

        // Resolve the intent into a cross-chain order for fillers to use when filling the order
        return _resolve(intent, order.fillDeadline);
    }

    /// @inheritdoc IUntronIntents
    function reclaim(bytes32 orderId, bytes calldata proof) external {
        // Get the intent from the intents mapping
        Intent memory intent = _intents[orderId];

        // Get the fill deadline from the fillDeadlines mapping
        uint32 fillDeadline = fillDeadlines[orderId];

        // Determine who should get the funds for the intent (the filler, the user, or no one)
        address beneficiary = _determineBeneficiary(intent, proof, fillDeadline);

        // Transfer the input token from the filler to the user
        IERC20(intent.inputToken).transfer(beneficiary, intent.inputAmount);

        // Delete the intent from the intents mapping
        delete _intents[orderId];
        // Delete the fill deadline from the fillDeadlines mapping
        delete fillDeadlines[orderId];
    }

    /// @notice Determine who should get the funds for the intent (the filler, the user, or no one)
    /// @param intent The intent to validate
    /// @param proof The proof of fulfillment
    /// @param fillDeadline The deadline for the fill of the order
    /// @return address The address of the beneficiary
    /// @dev In the mock, the beneficiary is always the owner. In production, this will be verified by
    ///      the ZK proof of the fill on Tron blockchain. After deadline, the user will be able to reclaim
    ///      the funds without the proof, in case the filler doesn't fill the order and prove it before the deadline.
    function _determineBeneficiary(Intent memory intent, bytes calldata proof, uint32 fillDeadline)
        internal
        virtual
        returns (address);
}
