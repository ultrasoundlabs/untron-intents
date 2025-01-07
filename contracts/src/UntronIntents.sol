// SPDX-License-Identifier: BUSL-1.1
pragma solidity ^0.8.20;

import "./interfaces/IUntronIntents.sol";
import "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import "@openzeppelin/contracts/token/ERC20/extensions/IERC20Permit.sol";
import "@openzeppelin/contracts-upgradeable/proxy/utils/Initializable.sol";
import "permit2/interfaces/IPermit2.sol";
import "permit2/interfaces/ISignatureTransfer.sol";
import "permit2/libraries/SignatureVerification.sol";

/// @title Basic ERC-7683 logic for Untron Intents
/// @author Ultrasound Labs
/// @dev This contract implements basic logic for EVM->TRON intent-based transfers.
///      It does not implement fill verification logic, which must be implemented by the inheriting contract.
abstract contract UntronIntents is IUntronIntents, Initializable {
    /// @dev Library for signature verification
    using SignatureVerification for bytes;

    /// @notice Mapping of user addresses to their gasless nonce
    /// @dev Used to prevent replay attacks for gasless orders
    mapping(address => uint256) public gaslessNonces;

    /// @notice Mapping of order IDs to the hashes of their intents
    mapping(bytes32 => bytes32) public orders;

    /// @notice Mapping of order IDs to their fill deadlines
    mapping(bytes32 => uint32) public fillDeadlines;

    /// @dev The USDT TRC20 token address on Tron
    bytes32 internal constant USDT_TRC20 =
        bytes32(uint256(uint168(bytes21(0x41a614f803b6fd780986a42c78ec9c7f77e6ded13c))));

    /// @dev The DestinationSettler contract address on Tron
    /// @notice TODO: This needs to be set to the actual address
    bytes32 internal constant TRON_SETTLEMENT_ADDRESS = bytes32(uint256(uint168(bytes21(0))));

    /// @dev The TRON SLIP-44 coin ID (used as a chain ID)
    uint32 internal constant TRON_COINID = 0x800000c3;

    /// @dev EIP-712 domain type hash
    bytes32 internal constant EIP712_DOMAIN_TYPEHASH =
        keccak256("EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)");

    /// @dev EIP-712 input type hash
    bytes32 internal constant INPUT_TYPEHASH = keccak256("Input(address token,uint256 amount)");

    /// @dev EIP-712 intent type hash
    bytes32 public constant INTENT_TYPEHASH = keccak256(
        "Intent(address refundBeneficiary,Input[] inputs,bytes21 to,uint256 outputAmount,bytes32 orderId)Input(address token,uint256 amount)"
    );

    /// @dev EIP-712 witness type string
    string constant witnessTypeString =
        "Intent witness)Intent(address refundBeneficiary,(address,uint256)[] inputs,bytes21 to,uint256 outputAmount,bytes32 orderId)TokenPermissions(address token,uint256 amount)";

    /// @dev EIP-712 domain separator
    bytes32 public DOMAIN_SEPARATOR;

    /// @dev Permit2 contract instance
    IPermit2 public permit2;

    /// @notice Initializes the contract
    /// @param _permit2 Address of the Permit2 contract
    function __UntronIntents_init(address _permit2) internal onlyInitializing {
        // Initialize the domain separator
        DOMAIN_SEPARATOR = keccak256(
            abi.encode(EIP712_DOMAIN_TYPEHASH, keccak256("UntronIntents"), keccak256("1"), block.chainid, address(this))
        );
        // Initialize the Permit2 contract instance
        permit2 = IPermit2(_permit2);
    }

    /// @notice Get this network's chain ID
    /// @return uint64 The chain ID
    function _chainId() internal view returns (uint64) {
        uint64 chainId;
        assembly {
            chainId := chainid()
        }
        return chainId;
    }

    /// @notice Disperses tokens from one address to another
    /// @param inputs Array of Input structs containing token addresses and amounts
    /// @param from Address to transfer tokens from
    /// @param to Address to transfer tokens to
    function disperse(Input[] memory inputs, address from, address to) internal {
        for (uint256 i = 0; i < inputs.length; i++) {
            if (from == address(this)) {
                require(IERC20(inputs[i].token).transfer(to, inputs[i].amount), "Transfer failed");
            } else {
                require(IERC20(inputs[i].token).transferFrom(from, to, inputs[i].amount), "Transfer failed");
            }
        }
    }

    /// @notice Resolve an intent into a resolved cross-chain order
    /// @param intent The intent to resolve
    /// @param fillDeadline The deadline for filling the order
    /// @return ResolvedCrossChainOrder The resolved cross-chain order
    function _resolve(Intent memory intent, uint32 fillDeadline)
        internal
        view
        returns (ResolvedCrossChainOrder memory)
    {
        // Create output array with USDT TRC20 on Tron
        Output[] memory minReceived = new Output[](1);
        minReceived[0] = Output(USDT_TRC20, intent.outputAmount, bytes32(uint256(uint168(intent.to))), TRON_COINID);

        // Create fill instruction to send output to settlement address on Tron
        FillInstruction[] memory fillInstructions = new FillInstruction[](1);
        fillInstructions[0] = FillInstruction(TRON_COINID, TRON_SETTLEMENT_ADDRESS, "");

        // Return the resolved cross-chain order
        return ResolvedCrossChainOrder({
            user: msg.sender,
            originChainId: _chainId(),
            openDeadline: uint32(block.timestamp),
            fillDeadline: fillDeadline,
            maxSpent: intent.inputs,
            minReceived: minReceived,
            fillInstructions: fillInstructions
        });
    }

    /// @inheritdoc IOriginSettler
    function resolve(OnchainCrossChainOrder calldata order) external view returns (ResolvedCrossChainOrder memory) {
        // Decode the intent from the order data
        Intent memory intent = abi.decode(order.orderData, (Intent));

        // Resolve the intent into a cross-chain order for fillers to use when filling the order
        return _resolve(intent, order.fillDeadline);
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

    /// @notice Compute the message hash for EIP-712 signing
    /// @param orderId The ID of the order
    /// @param intent The intent to hash
    /// @return bytes32 The computed message hash
    function _messageHash(bytes32 orderId, Intent memory intent) internal view returns (bytes32) {
        // Reconstruct the message that was signed using EIP-712
        // For nested structs, the typehash of the inner struct is hashed
        bytes32[] memory encodedInputs = new bytes32[](intent.inputs.length);
        for (uint256 i = 0; i < intent.inputs.length; i++) {
            encodedInputs[i] = keccak256(
                abi.encode(INPUT_TYPEHASH, intent.inputs[i].token, intent.inputs[i].amount)
            );
        }

        bytes32 structHash = keccak256(
            abi.encode(
                INTENT_TYPEHASH, 
                intent.refundBeneficiary, 
                keccak256(abi.encodePacked(encodedInputs)),
                intent.to, 
                intent.outputAmount, 
                orderId
            )
        );
        return keccak256(abi.encodePacked("\x19\x01", DOMAIN_SEPARATOR, structHash));
    }

    /// @inheritdoc IOriginSettler
    function open(OnchainCrossChainOrder calldata order) external override {
        // Check that the order is not expired
        require(order.fillDeadline > block.timestamp, "Order expired");

        // Compute the order ID
        bytes32 orderId = keccak256(abi.encode(order));

        // Decode the intent from the order data
        Intent memory intent = abi.decode(order.orderData, (Intent));

        // Transfer the input tokens from the user to this contract
        disperse(intent.inputs, msg.sender, address(this));

        // Resolve the intent into a cross-chain order for fillers to use when filling the order
        ResolvedCrossChainOrder memory resolvedOrder = _resolve(intent, order.fillDeadline);

        // Store the intent in the orders mapping
        orders[orderId] = keccak256(order.orderData);
        // Store the fill deadline in the fillDeadlines mapping
        fillDeadlines[orderId] = order.fillDeadline;

        // Emit an Open event
        emit Open(orderId, resolvedOrder);
    }

    /// @notice Internal function to open a gasless order
    /// @param order The gasless cross-chain order to open
    function _openFor(GaslessCrossChainOrder calldata order) internal {
        // Validate the order
        require(order.originSettler == address(this), "Invalid origin settler");
        require(order.openDeadline > block.timestamp, "Open deadline expired");
        require(order.fillDeadline > block.timestamp, "Order expired");
        require(order.nonce == gaslessNonces[order.user], "Invalid nonce");

        // Increment the user's gasless nonce
        gaslessNonces[order.user]++;

        // Compute the order ID
        bytes32 orderId = keccak256(abi.encode(order));

        // Decode the intent from the order data
        Intent memory intent = abi.decode(order.orderData, (Intent));

        // Store the intent in the orders mapping
        orders[orderId] = keccak256(order.orderData);
        // Store the fill deadline in the fillDeadlines mapping
        fillDeadlines[orderId] = order.fillDeadline;

        // Resolve the intent into a cross-chain order for fillers to use when filling the order
        ResolvedCrossChainOrder memory resolvedOrder = _resolve(intent, order.fillDeadline);

        // Emit an Open event
        emit Open(orderId, resolvedOrder);
    }

    /// @inheritdoc IOriginSettler
    function openFor(GaslessCrossChainOrder calldata order, bytes calldata signature, bytes calldata) public {
        // Compute the order ID
        bytes32 orderId = keccak256(abi.encode(order));

        // Decode the intent from the order data
        Intent memory intent = abi.decode(order.orderData, (Intent));

        // Compute the message hash
        bytes32 messageHash = _messageHash(orderId, intent);

        // Verify the signature
        signature.verify(messageHash, order.user);

        // Transfer the input tokens from the user to this contract
        disperse(intent.inputs, order.user, address(this));

        // Call the internal openFor function
        _openFor(order);
    }

    /// @inheritdoc IUntronIntents
    function permitAndOpenFor(
        GaslessCrossChainOrder calldata order,
        bytes calldata signature,
        bytes calldata fillerData,
        uint256[] calldata deadlines,
        uint8[] calldata v,
        bytes32[] calldata r,
        bytes32[] calldata s
    ) external {
        // Decode the intent from the order data
        Intent memory intent = abi.decode(order.orderData, (Intent));
        address user = order.user;

        // Process permits for each input token
        for (uint256 i = 0; i < intent.inputs.length; i++) {
            // Call the permit function on the token
            IERC20Permit(intent.inputs[i].token).permit(
                user, address(this), intent.inputs[i].amount, deadlines[i], v[i], r[i], s[i]
            );
        }

        // Call the openFor function
        openFor(order, signature, fillerData);
    }

    /// @inheritdoc IUntronIntents
    function permit2AndOpenFor(
        GaslessCrossChainOrder calldata order,
        bytes calldata,
        bytes calldata permit,
        uint48 deadline
    ) external {
        // Decode the intent from the order data
        Intent memory intent = abi.decode(order.orderData, (Intent));
        bytes32 orderId = keccak256(abi.encode(order));

        // Prepare Permit2 data structures
        ISignatureTransfer.TokenPermissions[] memory permissions =
            new ISignatureTransfer.TokenPermissions[](intent.inputs.length);
        ISignatureTransfer.SignatureTransferDetails[] memory transferDetails =
            new ISignatureTransfer.SignatureTransferDetails[](intent.inputs.length);

        // Populate Permit2 data structures
        for (uint256 i = 0; i < intent.inputs.length; i++) {
            permissions[i] = ISignatureTransfer.TokenPermissions(intent.inputs[i].token, intent.inputs[i].amount);
            transferDetails[i] = ISignatureTransfer.SignatureTransferDetails(address(this), intent.inputs[i].amount);
        }

        // Create Permit2 batch transfer struct
        ISignatureTransfer.PermitBatchTransferFrom memory batch =
            ISignatureTransfer.PermitBatchTransferFrom({permitted: permissions, nonce: 0, deadline: deadline});

        // Compute the message hash
        bytes32 messageHash = _messageHash(orderId, intent);

        // Call Permit2 to transfer tokens
        permit2.permitWitnessTransferFrom(batch, transferDetails, order.user, messageHash, witnessTypeString, permit);

        // Call the internal openFor function
        _openFor(order);
    }

    /// @inheritdoc IUntronIntents
    function reclaim(bytes32 orderId, Intent memory intent, bytes calldata proof) external {
        // Verify the intent matches the stored order
        require(orders[orderId] == keccak256(abi.encode(intent)), "Invalid intent");

        // Get the fill deadline from the fillDeadlines mapping
        uint32 fillDeadline = fillDeadlines[orderId];

        // Validate the fill of the order if not expired
        if (fillDeadline > block.timestamp) {
            require(_validateFill(intent, proof), "Invalid fill");
        }

        // Transfer the input tokens from the contract to the user or refund beneficiary
        disperse(intent.inputs, address(this), fillDeadline > block.timestamp ? msg.sender : intent.refundBeneficiary);

        // Delete the intent from the orders mapping
        delete orders[orderId];
        // Delete the fill deadline from the fillDeadlines mapping
        delete fillDeadlines[orderId];
    }

    /// @notice Validate the fill of the order
    /// @param intent The intent to validate
    /// @param proof The proof of fulfillment
    /// @return bool Whether the fill is valid
    /// @dev This function should be implemented by the inheriting contract
    function _validateFill(Intent memory intent, bytes calldata proof) internal virtual returns (bool);
}
