// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {TokenUtils} from "./utils/TokenUtils.sol";

/// @title UntronReceiver
/// @notice Minimal “holding” contract that can custody tokens/ETH and let its owner pull them out.
/// @dev
/// - Designed to be deployed as a CREATE2 deterministic address by {IntentsForwarder}.
/// - Ownership is immutable: the owner is the deployer (msg.sender) at construction time.
/// - The only privileged action is {pull}, which transfers funds to the immutable owner.
///
/// This contract is intentionally tiny:
/// - It can safely act as a counterfactual deposit address.
/// - It has no upgrade hooks, no arbitrary call functionality, and no external transfer methods
///   besides “pull to owner”.
/// @author Ultrasound Labs
contract UntronReceiver {
    /*//////////////////////////////////////////////////////////////
                                STORAGE
    //////////////////////////////////////////////////////////////*/

    /// @notice Immutable owner allowed to pull funds.
    /// @dev In this system, OWNER is expected to be the {IntentsForwarder} that deployed the receiver.
    address payable public immutable OWNER;

    /*//////////////////////////////////////////////////////////////
                                  ERRORS
    //////////////////////////////////////////////////////////////*/

    /// @notice Reverts when a non-owner attempts to pull funds.
    error NotOwner();

    /*//////////////////////////////////////////////////////////////
                                 CONSTRUCTOR
    //////////////////////////////////////////////////////////////*/

    /// @notice Sets the immutable {OWNER} to the deployer.
    /// @dev When deployed by {IntentsForwarder}, msg.sender is the forwarder and thus becomes OWNER.
    constructor() {
        OWNER = payable(msg.sender);
    }

    /// @notice Accepts native token (e.g. ETH) deposits.
    /// @dev Enables receiver addresses to custody ETH as well as ERC-20 tokens.
    receive() external payable {}

    /*//////////////////////////////////////////////////////////////
                                 FUNCTIONS
    //////////////////////////////////////////////////////////////*/

    /// @notice Pulls funds held by this receiver to {OWNER}.
    /// @dev Only callable by {OWNER}. For ERC-20 tokens, this uses {TokenUtils.transfer}.
    /// @param token Token to pull (address(0) = native ETH).
    /// @param amount Amount to pull. If 0, no transfer is performed.
    function pull(address token, uint256 amount) external {
        if (msg.sender != OWNER) revert NotOwner();
        if (amount != 0) {
            TokenUtils.transfer(token, OWNER, amount);
        }
    }
}
