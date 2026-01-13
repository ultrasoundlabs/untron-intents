// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {Test} from "forge-std/Test.sol";

import {IntentsForwarder} from "../../src/IntentsForwarder.sol";

import {MockERC20} from "../../src/mocks/MockERC20.sol";

abstract contract ForwarderTestBase is Test {
    MockERC20 internal usdt;
    MockERC20 internal usdc;

    IntentsForwarder internal forwarder;
    address internal owner;

    function setUp() public virtual {
        owner = makeAddr("owner");
        usdt = new MockERC20("USDT", "USDT", 6);
        usdc = new MockERC20("USDC", "USDC", 6);
        forwarder = new IntentsForwarder(address(usdt), address(usdc), owner);
    }

    function baseSalt(uint256 targetChain, address beneficiary, bool beneficiaryClaimOnly, bytes32 intentHash)
        internal
        pure
        returns (bytes32)
    {
        return keccak256(abi.encodePacked(targetChain, beneficiary, beneficiaryClaimOnly, intentHash));
    }

    function ephemeralSalt(bytes32 receiverSalt, bytes32 forwardSalt, address tokenOut, uint256 balance)
        internal
        pure
        returns (bytes32)
    {
        return keccak256(abi.encodePacked(receiverSalt, forwardSalt, tokenOut, balance));
    }
}
