// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {ForwarderTestBase} from "./ForwarderTestBase.sol";

import {UntronIntents} from "../../src/UntronIntents.sol";
import {TriggerSmartContract} from "../../src/external/interfaces/ITronTxReader.sol";

import {MockTronTxReader} from "../../src/mocks/MockTronTxReader.sol";
import {MockUntronV3} from "../../src/mocks/MockUntronV3.sol";
import {MockERC20} from "../../src/mocks/MockERC20.sol";

abstract contract UntronTestBase is ForwarderTestBase {
    address internal solver;
    address internal maker;
    address internal untronOwner;

    MockTronTxReader internal tronReader;
    MockUntronV3 internal v3;
    UntronIntents internal intents;

    function setUp() public virtual override {
        super.setUp();

        solver = makeAddr("solver");
        maker = makeAddr("maker");
        untronOwner = makeAddr("untronOwner");

        tronReader = new MockTronTxReader();
        v3 = new MockUntronV3(tronReader, makeAddr("tronController"), makeAddr("tronUsdt"));
        intents = new UntronIntents(untronOwner, v3, address(usdt));
    }

    function _setRecommendedFee(uint256 ppm, uint256 flat) internal {
        vm.prank(untronOwner);
        intents.setRecommendedIntentFee(ppm, flat);
    }

    function _tronAddrBytes21(address tronAddr) internal pure returns (bytes21 out) {
        uint168 packed = (uint168(uint8(0x41)) << 160) | uint168(uint160(tronAddr));
        return bytes21(packed);
    }

    function _receiverIntentHash(address toTron) internal view returns (bytes32) {
        return keccak256(abi.encode(forwarder, toTron));
    }

    function _receiverId(address toTron, bytes32 forwardSalt, address token, uint256 amount)
        internal
        view
        returns (bytes32)
    {
        return intents.receiverIntentId(forwarder, toTron, forwardSalt, token, amount);
    }

    function _predictEphemeralReceiver(address toTron, bytes32 forwardSalt, address token, uint256 amount)
        internal
        view
        returns (address receiver)
    {
        return _predictEphemeralReceiverFor(block.chainid, address(intents), true, toTron, forwardSalt, token, amount);
    }

    function _predictEphemeralReceiverFor(
        uint256 targetChain,
        address beneficiary,
        bool beneficiaryClaimOnly,
        address toTron,
        bytes32 forwardSalt,
        address token,
        uint256 amount
    ) internal view returns (address receiver) {
        bytes32 intentHash = _receiverIntentHash(toTron);
        bytes32 baseReceiverSalt = baseSalt(targetChain, beneficiary, beneficiaryClaimOnly, intentHash);
        bytes32 epSalt = ephemeralSalt(baseReceiverSalt, forwardSalt, token, amount);
        return forwarder.predictReceiverAddress(epSalt);
    }

    function _mintToEphemeralReceiver(address toTron, bytes32 forwardSalt, MockERC20 token, uint256 amount)
        internal
        returns (address receiver)
    {
        receiver = _predictEphemeralReceiver(toTron, forwardSalt, address(token), amount);
        token.mint(receiver, amount);
    }

    function _mintAndApproveSolverDeposit(address solver_) internal {
        uint256 deposit = intents.INTENT_CLAIM_DEPOSIT();
        usdt.mint(solver_, deposit);
        vm.prank(solver_);
        usdt.approve(address(intents), type(uint256).max);
    }

    function _claimVirtual(address solver_, address toTron, bytes32 forwardSalt, address token, uint256 amount)
        internal
        returns (bytes32 id)
    {
        _mintAndApproveSolverDeposit(solver_);
        id = _receiverId(toTron, forwardSalt, token, amount);
        vm.prank(solver_);
        intents.claimVirtualReceiverIntent(forwarder, toTron, forwardSalt, token, amount);
    }

    function _setTronTx_UsdtTransfer(address toTron, uint256 amount) internal {
        TriggerSmartContract memory tx_;
        tx_.toTron = _tronAddrBytes21(v3.tronUsdt());
        tx_.data = abi.encodeWithSelector(bytes4(keccak256("transfer(address,uint256)")), toTron, amount);
        tronReader.setTx(tx_);
    }

    function _setTronTx_UsdtTransferWithBadCalldataLength() internal {
        TriggerSmartContract memory tx_;
        tx_.toTron = _tronAddrBytes21(v3.tronUsdt());
        tx_.data = hex"12345678";
        tronReader.setTx(tx_);
    }

    function _proveAs(address solver_, bytes32 id) internal {
        bytes[20] memory blocks;
        bytes32[] memory proof;
        vm.prank(solver_);
        intents.proveIntentFill(id, blocks, "", proof, 0);
    }

    function _fundReceiver(address toTron, bytes32 forwardSalt, address token, uint256 amount) internal {
        intents.fundReceiverIntent(forwarder, toTron, forwardSalt, token, amount);
    }
}
