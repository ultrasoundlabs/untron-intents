// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {UntronTestBase} from "./helpers/UntronTestBase.sol";

import {UntronIntents} from "../src/UntronIntents.sol";
import {TriggerSmartContract} from "../src/external/interfaces/ITronTxReader.sol";

contract UntronIntentsTronDecodingTest is UntronTestBase {
    function setUp() public override {
        super.setUp();
        _setRecommendedFee(0, 0);
    }

    function test_proveIntentFill_reverts_TronInvalidCalldataLength() public {
        address toTron = makeAddr("toTron");
        bytes32 forwardSalt = keccak256("forwardSalt");
        uint256 amount = 1_000_000;

        bytes32 id = _claimVirtual(solver, toTron, forwardSalt, address(usdt), amount);

        TriggerSmartContract memory tx_;
        tx_.toTron = _tronAddrBytes21(v3.tronUsdt());
        tx_.data = hex"";
        tronReader.setTx(tx_);

        bytes[20] memory blocks;
        bytes32[] memory proof;

        vm.prank(solver);
        vm.expectRevert(UntronIntents.TronInvalidCalldataLength.selector);
        intents.proveIntentFill(id, blocks, "", proof, 0);
    }

    function test_proveIntentFill_reverts_NotATrc20Transfer() public {
        address toTron = makeAddr("toTron");
        bytes32 forwardSalt = keccak256("forwardSalt");
        uint256 amount = 1_000_000;

        bytes32 id = _claimVirtual(solver, toTron, forwardSalt, address(usdt), amount);

        TriggerSmartContract memory tx_;
        tx_.toTron = _tronAddrBytes21(v3.tronUsdt());
        tx_.data = hex"12345678";
        tronReader.setTx(tx_);

        bytes[20] memory blocks;
        bytes32[] memory proof;

        vm.prank(solver);
        vm.expectRevert(UntronIntents.NotATrc20Transfer.selector);
        intents.proveIntentFill(id, blocks, "", proof, 0);
    }

    function test_proveIntentFill_reverts_TronInvalidTrc20DataLength_transfer() public {
        address toTron = makeAddr("toTron");
        bytes32 forwardSalt = keccak256("forwardSalt");
        uint256 amount = 1_000_000;

        bytes32 id = _claimVirtual(solver, toTron, forwardSalt, address(usdt), amount);

        bytes4 transferSel = bytes4(keccak256("transfer(address,uint256)"));

        TriggerSmartContract memory tx_;
        tx_.toTron = _tronAddrBytes21(v3.tronUsdt());
        tx_.data = abi.encodePacked(transferSel);
        tronReader.setTx(tx_);

        bytes[20] memory blocks;
        bytes32[] memory proof;

        vm.prank(solver);
        vm.expectRevert(UntronIntents.TronInvalidTrc20DataLength.selector);
        intents.proveIntentFill(id, blocks, "", proof, 0);
    }

    function test_proveIntentFill_reverts_WrongTxProps_whenTransferToMismatch() public {
        address toTron = makeAddr("toTron");
        address wrongTo = makeAddr("wrongTo");
        bytes32 forwardSalt = keccak256("forwardSalt");
        uint256 amount = 1_000_000;

        bytes32 id = _claimVirtual(solver, toTron, forwardSalt, address(usdt), amount);

        TriggerSmartContract memory tx_;
        tx_.toTron = _tronAddrBytes21(v3.tronUsdt());
        tx_.data = abi.encodeWithSelector(bytes4(keccak256("transfer(address,uint256)")), wrongTo, amount);
        tronReader.setTx(tx_);

        bytes[20] memory blocks;
        bytes32[] memory proof;

        vm.prank(solver);
        vm.expectRevert(UntronIntents.WrongTxProps.selector);
        intents.proveIntentFill(id, blocks, "", proof, 0);
    }
}

