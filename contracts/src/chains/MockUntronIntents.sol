// SPDX-License-Identifier: BUSL-1.1
pragma solidity ^0.8.20;

import "@openzeppelin/contracts-upgradeable/access/OwnableUpgradeable.sol";
import "@openzeppelin/contracts-upgradeable/proxy/utils/Initializable.sol";
import "../UntronIntents.sol";

/// @title Chain-agnotic mock contract for Untron Intents
/// @author Ultrasound Labs
/// @dev This contract is intended to be used for testing purposes only.
///      On a deployed mock, all intents are considered filled by the owner.
///      It does not verify Tron blockchain state, and the owner is a source of truth.
contract MockUntronIntents is Initializable, OwnableUpgradeable, UntronIntents {
    /// @custom:oz-upgrades-unsafe-allow constructor
    constructor() {
        _disableInitializers();
    }

    /// @notice Initialize the contract
    /// @param _permit2 The address of the Permit2 contract
    /// @dev This is required for upgradeable contracts
    function initialize(address _permit2) public initializer {
        __Ownable_init(msg.sender);
        __UntronIntents_init(_permit2);
    }

    /// @inheritdoc UntronIntents
    function _validateFills(FillInstruction[] calldata, bytes calldata) internal view override returns (bool) {
        return msg.sender == owner();
    }
}
