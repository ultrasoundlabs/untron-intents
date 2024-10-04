// SPDX-License-Identifier: BUSL-1.1
pragma solidity ^0.8.20;

import "./IERC7683.sol";

interface IUntronIntents is IOriginSettler {
    /// @dev The intent struct for an order
    struct Intent {
        // The user who initiated the order
        address user;
        // The input token for the order
        address inputToken;
        // The input amount for the order
        uint256 inputAmount;
        // The Tron address to send USDT TRC20 to
        bytes21 to;
        // The output amount of USDT TRC20 for the order
        uint256 outputAmount;
    }

    /// @notice Get the gasless nonce for a user
    /// @param user The user to get the nonce for
    /// @return The gasless nonce for the user
    function gaslessNonces(address user) external view returns (uint256);

    /// @notice Get the intent for an order
    /// @param orderId The ID of the order
    /// @return The intent for the order
    function intents(bytes32 orderId) external view returns (Intent memory);

    /// @notice Reclaim the locked funds for a filled order
    /// @param orderId The ID of the order
    /// @param proof The proof of fulfillment of the order
    function reclaim(bytes32 orderId, bytes calldata proof) external;
}
