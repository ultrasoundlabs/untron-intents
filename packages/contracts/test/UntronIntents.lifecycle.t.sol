// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {UntronTestBase} from "./helpers/UntronTestBase.sol";

import {UntronIntents} from "../src/UntronIntents.sol";
import {
    TriggerSmartContract,
    TransferContract,
    DelegateResourceContract
} from "../src/external/interfaces/ITronTxReader.sol";

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
                UntronIntents.TriggerSmartContractIntent({to: makeAddr("tronTarget"), callValueSun: 0, data: hex"abcd"})
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
            tx_.callValueSun = specs.callValueSun;
            tx_.data = specs.data;
            tronReader.setTx(tx_);
        }

        _proveAs(solver, id);

        // Solver gets deposit + escrowed USDC.
        assertEq(usdt.balanceOf(solver), intents.INTENT_CLAIM_DEPOSIT());
        assertEq(usdc.balanceOf(solver), escrow);
    }

    function test_createIntent_triggerSmartContract_checksCallValueSun() public {
        // Maker escrows USDC.
        uint256 escrow = 5_000_000;
        usdc.mint(maker, escrow);
        vm.prank(maker);
        usdc.approve(address(intents), type(uint256).max);

        uint256 callValueSun = 123_456_789;
        UntronIntents.Intent memory intent = UntronIntents.Intent({
            intentType: UntronIntents.IntentType.TRIGGER_SMART_CONTRACT,
            intentSpecs: abi.encode(
                UntronIntents.TriggerSmartContractIntent({
                    to: makeAddr("tronTarget"), callValueSun: callValueSun, data: hex"abcd"
                })
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

        _mintAndApproveSolverDeposit(solver);
        vm.prank(solver);
        intents.claimIntent(id);

        UntronIntents.TriggerSmartContractIntent memory specs =
            abi.decode(intent.intentSpecs, (UntronIntents.TriggerSmartContractIntent));

        // Mock Tron tx that matches the intent, including call value.
        {
            TriggerSmartContract memory tx_;
            tx_.toTron = _tronAddrBytes21(specs.to);
            tx_.callValueSun = specs.callValueSun;
            tx_.data = specs.data;
            tronReader.setTx(tx_);
        }

        _proveAs(solver, id);

        assertEq(usdc.balanceOf(solver), escrow);
        assertEq(usdt.balanceOf(solver), intents.INTENT_CLAIM_DEPOSIT());
    }

    function test_createIntent_triggerSmartContract_revertsOnCallValueMismatch() public {
        uint256 escrow = 5_000_000;
        usdc.mint(maker, escrow);
        vm.prank(maker);
        usdc.approve(address(intents), type(uint256).max);

        UntronIntents.Intent memory intent = UntronIntents.Intent({
            intentType: UntronIntents.IntentType.TRIGGER_SMART_CONTRACT,
            intentSpecs: abi.encode(
                UntronIntents.TriggerSmartContractIntent({to: makeAddr("tronTarget"), callValueSun: 1, data: hex"abcd"})
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

        _mintAndApproveSolverDeposit(solver);
        vm.prank(solver);
        intents.claimIntent(id);

        UntronIntents.TriggerSmartContractIntent memory specs =
            abi.decode(intent.intentSpecs, (UntronIntents.TriggerSmartContractIntent));

        // Mock Tron tx with mismatching call value.
        {
            TriggerSmartContract memory tx_;
            tx_.toTron = _tronAddrBytes21(specs.to);
            tx_.callValueSun = specs.callValueSun + 1;
            tx_.data = specs.data;
            tronReader.setTx(tx_);
        }

        vm.expectRevert(UntronIntents.WrongTxProps.selector);
        _proveAs(solver, id);
    }

    function test_createIntent_trxTransfer_happyPath() public {
        uint256 escrow = 5_000_000;
        usdc.mint(maker, escrow);
        vm.prank(maker);
        usdc.approve(address(intents), type(uint256).max);

        address toTron = makeAddr("trxRecipient");
        uint256 amountSun = 1_234_567;

        UntronIntents.Intent memory intent = UntronIntents.Intent({
            intentType: UntronIntents.IntentType.TRX_TRANSFER,
            intentSpecs: abi.encode(UntronIntents.TRXTransferIntent({to: toTron, amountSun: amountSun})),
            refundBeneficiary: maker,
            token: address(usdc),
            amount: escrow
        });

        uint256 deadline = block.timestamp + 1 days;
        vm.prank(maker);
        intents.createIntent(intent, deadline);

        bytes32 intentHash = keccak256(abi.encode(intent));
        bytes32 id = keccak256(abi.encodePacked(maker, intentHash, deadline));

        _mintAndApproveSolverDeposit(solver);
        vm.prank(solver);
        intents.claimIntent(id);

        // Mock Tron tx that matches the intent.
        {
            TransferContract memory tx_;
            tx_.toTron = _tronAddrBytes21(toTron);
            tx_.amountSun = amountSun;
            tronReader.setTransferTx(tx_);
        }

        _proveAs(solver, id);

        assertEq(usdc.balanceOf(solver), escrow);
        assertEq(usdt.balanceOf(solver), intents.INTENT_CLAIM_DEPOSIT());
    }

    function test_createIntent_trxTransfer_revertsOnMismatch() public {
        uint256 escrow = 5_000_000;
        usdc.mint(maker, escrow);
        vm.prank(maker);
        usdc.approve(address(intents), type(uint256).max);

        address toTron = makeAddr("trxRecipient");
        uint256 amountSun = 1_234_567;

        UntronIntents.Intent memory intent = UntronIntents.Intent({
            intentType: UntronIntents.IntentType.TRX_TRANSFER,
            intentSpecs: abi.encode(UntronIntents.TRXTransferIntent({to: toTron, amountSun: amountSun})),
            refundBeneficiary: maker,
            token: address(usdc),
            amount: escrow
        });

        uint256 deadline = block.timestamp + 1 days;
        vm.prank(maker);
        intents.createIntent(intent, deadline);

        bytes32 intentHash = keccak256(abi.encode(intent));
        bytes32 id = keccak256(abi.encodePacked(maker, intentHash, deadline));

        _mintAndApproveSolverDeposit(solver);
        vm.prank(solver);
        intents.claimIntent(id);

        // Mock Tron tx with mismatching amount.
        {
            TransferContract memory tx_;
            tx_.toTron = _tronAddrBytes21(toTron);
            tx_.amountSun = amountSun + 1;
            tronReader.setTransferTx(tx_);
        }

        vm.expectRevert(UntronIntents.WrongTxProps.selector);
        _proveAs(solver, id);
    }

    function test_createIntent_delegateResource_happyPath() public {
        uint256 escrow = 5_000_000;
        usdc.mint(maker, escrow);
        vm.prank(maker);
        usdc.approve(address(intents), type(uint256).max);

        address receiverTron = makeAddr("tronReceiver");
        uint8 resource = 1; // ENERGY
        uint256 balanceSun = 23_508e6; // 23_508 TRX (sun)
        uint256 lockPeriod = 200; // ~10 minutes if interpreted as blocks

        UntronIntents.Intent memory intent = UntronIntents.Intent({
            intentType: UntronIntents.IntentType.DELEGATE_RESOURCE,
            intentSpecs: abi.encode(
                UntronIntents.DelegateResourceIntent({
                    receiver: receiverTron, resource: resource, balanceSun: balanceSun, lockPeriod: lockPeriod
                })
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

        _mintAndApproveSolverDeposit(solver);
        vm.prank(solver);
        intents.claimIntent(id);

        // Mock Tron tx that matches the intent.
        {
            DelegateResourceContract memory tx_;
            tx_.receiverTron = _tronAddrBytes21(receiverTron);
            tx_.resource = resource;
            tx_.balanceSun = balanceSun;
            tx_.lock = true;
            tx_.lockPeriod = lockPeriod;
            tronReader.setDelegateResourceTx(tx_);
        }

        _proveAs(solver, id);

        assertEq(usdc.balanceOf(solver), escrow);
        assertEq(usdt.balanceOf(solver), intents.INTENT_CLAIM_DEPOSIT());
    }

    function test_createIntent_delegateResource_revertsOnMismatch() public {
        uint256 escrow = 5_000_000;
        usdc.mint(maker, escrow);
        vm.prank(maker);
        usdc.approve(address(intents), type(uint256).max);

        address receiverTron = makeAddr("tronReceiver");
        uint8 resource = 1; // ENERGY
        uint256 balanceSun = 23_508e6;
        uint256 lockPeriod = 200;

        UntronIntents.Intent memory intent = UntronIntents.Intent({
            intentType: UntronIntents.IntentType.DELEGATE_RESOURCE,
            intentSpecs: abi.encode(
                UntronIntents.DelegateResourceIntent({
                    receiver: receiverTron, resource: resource, balanceSun: balanceSun, lockPeriod: lockPeriod
                })
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

        _mintAndApproveSolverDeposit(solver);
        vm.prank(solver);
        intents.claimIntent(id);

        // Mock Tron tx with mismatching lock.
        {
            DelegateResourceContract memory tx_;
            tx_.receiverTron = _tronAddrBytes21(receiverTron);
            tx_.resource = resource;
            tx_.balanceSun = balanceSun;
            tx_.lock = false;
            tx_.lockPeriod = lockPeriod;
            tronReader.setDelegateResourceTx(tx_);
        }

        vm.expectRevert(UntronIntents.WrongTxProps.selector);
        _proveAs(solver, id);
    }

    function test_proveIntentFill_revertsOnReusedTronTxId() public {
        uint256 escrow = 5_000_000;
        usdc.mint(maker, escrow * 2);
        vm.prank(maker);
        usdc.approve(address(intents), type(uint256).max);

        address receiverTron = makeAddr("tronReceiver");
        uint8 resource = 1; // ENERGY
        uint256 balanceSun = 23_508e6;
        uint256 lockPeriod = 200;

        UntronIntents.Intent memory intent = UntronIntents.Intent({
            intentType: UntronIntents.IntentType.DELEGATE_RESOURCE,
            intentSpecs: abi.encode(
                UntronIntents.DelegateResourceIntent({
                    receiver: receiverTron, resource: resource, balanceSun: balanceSun, lockPeriod: lockPeriod
                })
            ),
            refundBeneficiary: maker,
            token: address(usdc),
            amount: escrow
        });

        uint256 deadline1 = block.timestamp + 1 days;
        uint256 deadline2 = block.timestamp + 2 days;
        vm.prank(maker);
        intents.createIntent(intent, deadline1);
        vm.prank(maker);
        intents.createIntent(intent, deadline2);

        bytes32 intentHash = keccak256(abi.encode(intent));
        bytes32 id1 = keccak256(abi.encodePacked(maker, intentHash, deadline1));
        bytes32 id2 = keccak256(abi.encodePacked(maker, intentHash, deadline2));

        _mintAndApproveSolverDeposit(solver);
        vm.prank(solver);
        intents.claimIntent(id1);
        _mintAndApproveSolverDeposit(solver);
        vm.prank(solver);
        intents.claimIntent(id2);

        // Mock a single Tron txid and reuse it for both proofs.
        {
            DelegateResourceContract memory tx_;
            tx_.txId = keccak256("same-txid");
            tx_.tronBlockNumber = 123;
            tx_.receiverTron = _tronAddrBytes21(receiverTron);
            tx_.resource = resource;
            tx_.balanceSun = balanceSun;
            tx_.lock = true;
            tx_.lockPeriod = lockPeriod;
            tronReader.setDelegateResourceTx(tx_);
        }

        _proveAs(solver, id1);

        vm.expectRevert(UntronIntents.WrongTxProps.selector);
        _proveAs(solver, id2);
    }

    function test_createIntent_delegateResource_allowsOverfillAndLongerLock() public {
        uint256 escrow = 5_000_000;
        usdc.mint(maker, escrow);
        vm.prank(maker);
        usdc.approve(address(intents), type(uint256).max);

        address receiverTron = makeAddr("tronReceiver");
        uint8 resource = 1; // ENERGY
        uint256 balanceSun = 1_000e6;
        uint256 lockPeriod = 10;

        UntronIntents.Intent memory intent = UntronIntents.Intent({
            intentType: UntronIntents.IntentType.DELEGATE_RESOURCE,
            intentSpecs: abi.encode(
                UntronIntents.DelegateResourceIntent({
                    receiver: receiverTron, resource: resource, balanceSun: balanceSun, lockPeriod: lockPeriod
                })
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

        _mintAndApproveSolverDeposit(solver);
        vm.prank(solver);
        intents.claimIntent(id);

        // Mock Tron tx that overfills amount and lock period.
        {
            DelegateResourceContract memory tx_;
            tx_.receiverTron = _tronAddrBytes21(receiverTron);
            tx_.resource = resource;
            tx_.balanceSun = balanceSun + 1; // >=
            tx_.lock = true;
            tx_.lockPeriod = lockPeriod + 1; // >=
            tronReader.setDelegateResourceTx(tx_);
        }

        _proveAs(solver, id);

        assertEq(usdc.balanceOf(solver), escrow);
        assertEq(usdt.balanceOf(solver), intents.INTENT_CLAIM_DEPOSIT());
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
