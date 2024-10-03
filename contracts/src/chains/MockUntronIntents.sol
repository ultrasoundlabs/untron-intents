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
    /// @dev This is required for upgradeable contracts
    function initialize() public initializer {
        __Ownable_init(msg.sender);
    }

    /// @inheritdoc UntronIntents
    function _determineBeneficiary(Intent memory, bytes calldata) internal view override returns (address) {
        return owner();
    }
}
