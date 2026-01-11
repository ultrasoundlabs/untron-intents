// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {UntronTestBase} from "./helpers/UntronTestBase.sol";

import {UntronIntents} from "../src/UntronIntents.sol";
import {TriggerSmartContract} from "../src/external/interfaces/ITronTxReader.sol";

contract UntronIntentsLifecycleTest is UntronTestBase {
    function setUp() public override {
        super.setUp();
        _setRecommendedFee(10_000, 0);
    }

    function test_createIntent_triggerSmartContract_happyPath() public {
        // Maker escrows USDC.
        uint256 escrow = 5_000_000;
        usdc.mint(maker, escrow);
        vm.prank(maker);
        usdc.approve(address(intents), type(uint256).max);

        UntronIntents.Intent memory intent = UntronIntents.Intent({
            intentType: UntronIntents.IntentType.TRIGGER_SMART_CONTRACT,
            intentSpecs: abi.encode(
                UntronIntents.TriggerSmartContractIntent({to: makeAddr("tronTarget"), data: hex"abcd"})
            ),
            refundBeneficiary: maker,
            token: address(usdc),
            amount: escrow
        });

        uint256 deadline = block.timestamp + 1 days;
        vm.prank(maker);
        intents.createIntent(intent, deadline);

        bytes32 intentHash = keccak256(abi.encode(intent));
        bytes32 id = keccak256(abi.encodePacked(maker, intentHash, deadline));

        // Solver claims and proves.
        _mintAndApproveSolverDeposit(solver);
        vm.prank(solver);
        intents.claimIntent(id);

        UntronIntents.TriggerSmartContractIntent memory specs =
            abi.decode(intent.intentSpecs, (UntronIntents.TriggerSmartContractIntent));

        // Mock Tron tx that matches the intent.
        {
            TriggerSmartContract memory tx_;
            tx_.toTron = _tronAddrBytes21(specs.to);
            tx_.data = specs.data;
            tronReader.setTx(tx_);
        }

        _proveAs(solver, id);

        // Solver gets deposit + escrowed USDC.
        assertEq(usdt.balanceOf(solver), intents.INTENT_CLAIM_DEPOSIT());
        assertEq(usdc.balanceOf(solver), escrow);
    }

    function test_unclaimIntent_unfunded_refundsSolverDeposit() public {
        address toTron = makeAddr("toTron");
        bytes32 forwardSalt = keccak256("forwardSalt");
        uint256 amount = 1_000_000;

        bytes32 id = _claimVirtual(solver, toTron, forwardSalt, address(usdt), amount);

        vm.warp(block.timestamp + intents.TIME_TO_FILL() + 1);
        vm.prank(makeAddr("anyone"));
        intents.unclaimIntent(id);

        assertEq(usdt.balanceOf(solver), intents.INTENT_CLAIM_DEPOSIT());
        (, uint256 claimedAt, uint256 deadline, address solverAddr, bool solved, bool funded, bool settled) =
            intents.intents(id);
        assertTrue(deadline != 0);
        assertFalse(solved);
        assertFalse(settled);
        assertEq(solverAddr, address(0));
        assertEq(claimedAt, 0);
        assertFalse(funded);
    }

    function test_unclaimIntent_funded_splitsDeposit() public {
        address toTron = makeAddr("toTron");
        bytes32 forwardSalt = keccak256("forwardSalt");
        uint256 amount = 2_000_000;

        _mintToEphemeralReceiver(toTron, forwardSalt, usdt, amount);
        bytes32 id = _claimVirtual(solver, toTron, forwardSalt, address(usdt), amount);

        _fundReceiver(toTron, forwardSalt, address(usdt), amount);

        address caller = makeAddr("caller");
        vm.warp(block.timestamp + intents.TIME_TO_FILL() + 1);
        vm.prank(caller);
        intents.unclaimIntent(id);

        assertEq(usdt.balanceOf(caller), intents.INTENT_CLAIM_DEPOSIT() / 2);
        assertEq(usdt.balanceOf(untronOwner), intents.INTENT_CLAIM_DEPOSIT() / 2);
        assertEq(usdt.balanceOf(solver), 0);
    }

    function test_closeIntent_revertsBeforeDeadline() public {
        address toTron = makeAddr("toTron");
        bytes32 forwardSalt = keccak256("forwardSalt");
        uint256 amount = 1_000_000;

        bytes32 id = _claimVirtual(solver, toTron, forwardSalt, address(usdt), amount);
        vm.expectRevert(UntronIntents.NotExpiredYet.selector);
        intents.closeIntent(id);
    }

    function test_closeIntent_afterDeadline_funded_unSolved_refundsEscrowAndSplitsDeposit() public {
        address toTron = makeAddr("toTron");
        bytes32 forwardSalt = keccak256("forwardSalt");
        uint256 amount = 3_000_000;

        _mintToEphemeralReceiver(toTron, forwardSalt, usdt, amount);
        bytes32 id = _claimVirtual(solver, toTron, forwardSalt, address(usdt), amount);
        _fundReceiver(toTron, forwardSalt, address(usdt), amount);

        address caller = makeAddr("caller");
        vm.warp(block.timestamp + intents.RECEIVER_INTENT_DURATION() + 1);
        vm.prank(caller);
        intents.closeIntent(id);

        // Escrow refunded to refundBeneficiary (= owner()).
        assertEq(usdt.balanceOf(untronOwner), amount + intents.INTENT_CLAIM_DEPOSIT() / 2);
        // Deposit split between caller and refund beneficiary.
        assertEq(usdt.balanceOf(caller), intents.INTENT_CLAIM_DEPOSIT() / 2);
        assertEq(usdt.balanceOf(solver), 0);
    }

    function test_settleIntent_revertsIfNotReady() public {
        address toTron = makeAddr("toTron");
        bytes32 forwardSalt = keccak256("forwardSalt");
        uint256 amount = 1_000_000;

        bytes32 id = _claimVirtual(solver, toTron, forwardSalt, address(usdt), amount);
        vm.expectRevert(UntronIntents.NothingToSettle.selector);
        intents.settleIntent(id);
    }
}
