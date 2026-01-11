// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {ReentrancyGuard} from "solady/utils/ReentrancyGuard.sol";
import {TokenUtils} from "./utils/TokenUtils.sol";

/// @notice Thrown when a caller other than the owner attempts to call a restricted function.
error NotOwner();
/// @notice Thrown when one of the low-level calls in `execute` fails.
/// @param callIndex Index of the call in the `calls` array that failed.
error CallFailed(uint256 callIndex);
/// @notice Thrown when the resulting token balance is less than the expected output amount.
/// @dev This guards against incomplete or unfavorable swaps.
error InsufficientOutput();

// things below are defined outside the contract so parents can use them

/// @notice Represents a low-level contract call used during swap execution.
/// @dev The call is executed with `call.value` native tokens and `call.data`
/// forwarded to the target address `to`.
/// @author Ultrasound Labs
struct Call {
    /// @notice Address of the contract to call.
    address to;
    /// @notice Native token amount (e.g. ETH) to forward with the call, or 0.
    uint256 value;
    /// @notice Calldata to send to the target contract.
    bytes data;
}

/// @title SwapExecutor
/// @notice Executes a sequence of arbitrary contract calls and settles the resulting token balance.
/// @dev
/// - Intended to be deployed and called via the main UntronV3 contract.
/// - Executes calls using `call` and enforces a minimum output amount for a single ERC-20 token.
/// Inspired by Daimo Pay's `DaimoPayExecutor`.
/// @author Ultrasound Labs
contract SwapExecutor is ReentrancyGuard {
    /// @notice Immutable address representing the owner/controller of this executor.
    /// @dev Only this address is allowed to call `execute` directly.
    address public immutable OWNER;

    /// @notice Initializes the SwapExecutor with an immutable owner address (msg.sender).
    /// @dev In this repo, msg.sender is the {IntentsForwarder} deploying the executor.
    constructor() {
        OWNER = msg.sender;
    }

    /// @notice Accepts native token (e.g. ETH) deposits used by swap calls.
    /// @dev This function enables the executor to receive ETH for subsequent low-level calls.
    receive() external payable {}

    /// @notice Executes a batch of arbitrary calls and settles token outputs.
    /// @dev
    /// - Reverts with {NotOwner} if `msg.sender` is not `OWNER`.
    /// - Reverts with {CallFailed} if any underlying call fails.
    /// - Reverts with {InsufficientOutput} if the post-call token balance is less than `expectedAmount`.
    /// - Uses `TokenUtils` to transfer all output to `recipient`.
    /// @param calls Array of low-level calls that will be executed in order.
    /// @param token Address of the ERC-20 token whose balance is checked and distributed.
    /// @param expectedAmount Minimum amount of `token` that must be present after executing `calls`.
    /// @param recipient Address to which the token output will be transferred.
    /// @return actualOut The total amount of `token` produced by the swap calls.
    function execute(Call[] calldata calls, address token, uint256 expectedAmount, address payable recipient)
        external
        nonReentrant
        returns (uint256 actualOut)
    {
        if (msg.sender != OWNER) revert NotOwner();

        // Execute provided calls.
        uint256 callsLength = calls.length;
        for (uint256 i = 0; i < callsLength; ++i) {
            Call calldata call = calls[i];
            // The executor intentionally does not attempt to interpret calldata or enforce allowlists.
            // Correctness and safety is enforced by restricting access to OWNER and by the
            // post-call balance check on `token`.
            (bool success,) = call.to.call{value: call.value}(call.data);
            if (!success) revert CallFailed(i);
        }

        // Determine actual output and enforce minimums.
        actualOut = TokenUtils.getBalanceOf(token, address(this));
        if (actualOut < expectedAmount) revert InsufficientOutput();

        // Transfer full output to the recipient (protocol-owned).
        if (actualOut != 0) {
            TokenUtils.transfer({token: token, recipient: recipient, amount: actualOut});
        }
    }
}
