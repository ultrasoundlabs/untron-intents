// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {UntronTestBase} from "./helpers/UntronTestBase.sol";

contract UntronIntentsReceiverVirtualTest is UntronTestBase {
    function setUp() public override {
        super.setUp();
        _setRecommendedFee(10_000, 123); // 1% + flat, snapshotted on claim
    }

    function test_VirtualReceiverIntent_CanProveBeforeFund_ThenSettleOnFund() public {
        address toTron = makeAddr("toTron");
        bytes32 forwardSalt = keccak256("forwardSalt");
        uint256 amount = 10_000_000;

        _mintToEphemeralReceiver(toTron, forwardSalt, usdt, amount);

        bytes32 id = _claimVirtual(solver, toTron, forwardSalt, address(usdt), amount);

        uint256 feeAtClaim = intents.recommendedIntentFee(amount);
        uint256 tronPayAmount = amount - feeAtClaim;
        _setTronTx_UsdtTransfer(toTron, tronPayAmount);

        _proveAs(solver, id);

        // Not funded yet => cannot settle; deposit stays locked.
        assertEq(usdt.balanceOf(solver), 0);

        // Fee changes after the claim should not affect required Tron payment.
        _setRecommendedFee(123_456, 789);

        _fundReceiver(toTron, forwardSalt, address(usdt), amount);

        // Solver gets escrow + claim deposit back.
        assertEq(usdt.balanceOf(solver), intents.INTENT_CLAIM_DEPOSIT() + amount);
    }
}

