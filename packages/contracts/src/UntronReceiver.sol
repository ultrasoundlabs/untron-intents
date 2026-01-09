// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {TokenUtils} from "./utils/TokenUtils.sol";

/// @title UntronReceiver
/// @notice Simple smart contract controlled by the deployer that holds ERC-20 tokens
///         and native ETH and lets the deployer transfer them to a specified recipient.
/// @author Ultrasound Labs
contract UntronReceiver {
    /*//////////////////////////////////////////////////////////////
                                STORAGE
    //////////////////////////////////////////////////////////////*/

    address payable public immutable OWNER;

    /*//////////////////////////////////////////////////////////////
                                  ERRORS
    //////////////////////////////////////////////////////////////*/

    error NotOwner();

    /*//////////////////////////////////////////////////////////////
                                 CONSTRUCTOR
    //////////////////////////////////////////////////////////////*/

    /// @notice Initializes the contract with the owner address.
    constructor() {
        OWNER = payable(msg.sender);
    }

    /*//////////////////////////////////////////////////////////////
                                 FUNCTIONS
    //////////////////////////////////////////////////////////////*/

    /// @notice Called by the owner to move `amount` of `token` held by this contract to owner.
    /// @param token The address of the token to transfer.
    /// @param amount The amount of tokens to transfer.
    function pull(address token, uint256 amount) external {
        if (msg.sender != OWNER) revert NotOwner();
        if (amount != 0) {
            TokenUtils.transfer(token, OWNER, amount);
        }
    }

    /// @notice Receive ETH to this contract.
    receive() external payable {}
}
