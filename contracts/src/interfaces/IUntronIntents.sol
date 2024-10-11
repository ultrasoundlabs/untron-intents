// SPDX-License-Identifier: BUSL-1.1
pragma solidity ^0.8.20;

import "./IERC7683.sol";

interface IUntronIntents is IOriginSettler {
    /// @dev The intent struct for an order
    struct Intent {
        // The address that can reclaim the order if it is not filled (usually its creator)
        address refundBeneficiary;
        // The ERC20 inputs for the order
        Input[] inputs;
        // The Tron address to send USDT TRC20 to
        bytes21 to;
        // The output amount of USDT TRC20 for the order
        uint256 outputAmount;
    }

    /// @notice EIP-712 typehash for the intent struct + order ID
    /// @dev Needed to sign gasless orders.
    ///      ABI is equal to Intent struct but with bytes32 (orderId) at the end
    function INTENT_TYPEHASH() external view returns (bytes32);
    /// @notice EIP-712 domain separator
    function DOMAIN_SEPARATOR() external view returns (bytes32);

    function _messageHash(bytes32 orderId, Intent memory intent) external view returns (bytes32);

    /// @notice Get the gasless nonce for a user
    /// @param user The user to get the nonce for
    /// @return The gasless nonce for the user
    function gaslessNonces(address user) external view returns (uint256);

    /// @notice Get if an order was created and not yet reclaimed
    /// @param orderId The ID of the order
    /// @return bool if it's created and not yet reclaimed
    function orders(bytes32 orderId) external view returns (bool);

    /// @notice Reclaim the locked funds for a filled order
    /// @param order The resolved order which was filled
    /// @param proof The proof of fulfillment of the order
    function reclaim(ResolvedCrossChainOrder calldata order, bytes calldata proof) external;

    /// @notice Multicall helper function to multi-permit and openFor in one call
    /// @param order The order to open
    /// @param signature The user's signature for the order
    /// @param fillerData Arbitrary data to pass to openFor
    /// @param deadlines The deadlines for the permits
    /// @param v The v values for the permits
    /// @param r The r values for the permits
    /// @param s The s values for the permits
    /// @dev Calls ERC20 permit on each input token before running openFor
    function permitAndOpenFor(
        GaslessCrossChainOrder calldata order,
        bytes calldata signature,
        bytes calldata fillerData,
        uint256[] calldata deadlines,
        uint8[] calldata v,
        bytes32[] calldata r,
        bytes32[] calldata s
    ) external;

    /// @notice Multicall helper function to permit2 and openFor in one call
    /// @param order The order to open
    /// @param fillerData Arbitrary data to pass to openFor
    /// @param permit Permit2 signature for the order
    /// @param deadline The deadline for the permit
    /// @dev permitWitnessTransferFrom is used. The witness must be equal to the EIP-712 message hash of the order.
    function permit2AndOpenFor(
        GaslessCrossChainOrder calldata order,
        bytes calldata fillerData,
        bytes calldata permit,
        uint48 deadline
    ) external;
}
