// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {UntronTestBase} from "./helpers/UntronTestBase.sol";
import {UntronIntents} from "../src/UntronIntents.sol";

contract UntronIntentsReceiverVirtualTest is UntronTestBase {
    event ReceiverIntentParams(
        bytes32 indexed id,
        address indexed forwarder,
        address indexed toTron,
        bytes32 forwardSalt,
        address token,
        uint256 amount
    );
    event ReceiverIntentFeeSnap(bytes32 indexed id, uint256 feePpm, uint256 feeFlat, uint256 tronPaymentAmount);
    event IntentCreated(
        bytes32 indexed id,
        address indexed creator,
        uint8 intentType,
        address token,
        uint256 amount,
        address refundBeneficiary,
        uint256 deadline,
        bytes intentSpecs
    );
    event IntentClaimed(bytes32 indexed id, address indexed solver, uint256 depositAmount);
    event IntentSolved(bytes32 indexed id, address indexed solver, bytes32 tronTxId, uint256 tronBlockNumber);
    event IntentFunded(bytes32 indexed id, address indexed funder, address token, uint256 amount);
    event IntentSettled(
        bytes32 indexed id,
        address indexed solver,
        address escrowToken,
        uint256 escrowAmount,
        address depositToken,
        uint256 depositAmount
    );

    function setUp() public override {
        super.setUp();
        _setRecommendedFee(10_000, 123); // 1% + flat, snapshotted on claim
    }

    function test_VirtualReceiverIntent_CanProveBeforeFund_ThenSettleOnFund() public {
        address toTron = makeAddr("toTron");
        bytes32 forwardSalt = keccak256("forwardSalt");
        uint256 amount = 10_000_000;

        _mintToEphemeralReceiver(toTron, forwardSalt, usdt, amount);

        bytes32 id = intents.receiverIntentId(forwarder, toTron, forwardSalt, address(usdt), amount);
        uint256 deadline = block.timestamp + intents.RECEIVER_INTENT_DURATION();

        uint256 feeAtClaim = intents.recommendedIntentFee(amount);
        uint256 tronPayAmount = amount - feeAtClaim;
        bytes memory specs = abi.encode(UntronIntents.USDTTransferIntent({to: toTron, amount: tronPayAmount}));

        _mintAndApproveSolverDeposit(solver);

        vm.expectEmit(true, true, true, true, address(intents));
        emit ReceiverIntentParams(id, address(forwarder), toTron, forwardSalt, address(usdt), amount);
        vm.expectEmit(true, true, true, true, address(intents));
        emit ReceiverIntentFeeSnap(id, 10_000, 123, tronPayAmount);
        vm.expectEmit(true, true, true, true, address(intents));
        emit IntentCreated(
            id,
            solver,
            uint8(UntronIntents.IntentType.USDT_TRANSFER),
            address(usdt),
            amount,
            untronOwner,
            deadline,
            specs
        );
        vm.expectEmit(true, true, true, true, address(intents));
        emit IntentClaimed(id, solver, intents.INTENT_CLAIM_DEPOSIT());

        vm.prank(solver);
        intents.claimVirtualReceiverIntent(forwarder, toTron, forwardSalt, address(usdt), amount);

        _setTronTx_UsdtTransfer(toTron, tronPayAmount);

        vm.expectEmit(true, true, true, true, address(intents));
        emit IntentSolved(id, solver, bytes32(0), 0);
        _proveAs(solver, id);

        // Not funded yet => cannot settle; deposit stays locked.
        assertEq(usdt.balanceOf(solver), 0);

        // Fee changes after the claim should not affect required Tron payment.
        _setRecommendedFee(123_456, 789);

        vm.expectEmit(true, true, true, true, address(intents));
        emit IntentFunded(id, address(this), address(usdt), amount);
        vm.expectEmit(true, true, true, true, address(intents));
        emit IntentSettled(id, solver, address(usdt), amount, address(usdt), intents.INTENT_CLAIM_DEPOSIT());
        _fundReceiver(toTron, forwardSalt, address(usdt), amount);

        // Solver gets escrow + claim deposit back.
        assertEq(usdt.balanceOf(solver), intents.INTENT_CLAIM_DEPOSIT() + amount);
    }
}
