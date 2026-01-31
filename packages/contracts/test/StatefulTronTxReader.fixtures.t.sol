// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {Test} from "forge-std/Test.sol";
import {stdJson} from "forge-std/StdJson.sol";

import {StatefulTronTxReader} from "../src/StatefulTronTxReader.sol";
import {
    ITronTxReader,
    TriggerSmartContract,
    TransferContract,
    DelegateResourceContract
} from "../src/external/interfaces/ITronTxReader.sol";

import {FixtureLoader} from "./tron/FixtureLoader.sol";

contract StatefulTronTxReaderFixturesTest is Test {
    using stdJson for string;

    function _runFixture(string memory path) private {
        if (!vm.exists(path)) fail(string.concat("missing fixture: ", path));

        string memory json = vm.readFile(path);

        // Deploy reader with SR set from fixture.
        address[] memory srsAddr = json.readAddressArray("$.srs");
        address[] memory delegateesAddr = json.readAddressArray("$.witnessDelegatees");
        assertEq(srsAddr.length, 27, "fixture srs length");
        assertEq(delegateesAddr.length, 27, "fixture witnessDelegatees length");

        bytes20[27] memory srs;
        bytes20[27] memory delegatees;
        for (uint256 i = 0; i < 27; i++) {
            srs[i] = bytes20(srsAddr[i]);
            delegatees[i] = bytes20(delegateesAddr[i]);
        }

        ITronTxReader reader = new StatefulTronTxReader(srs, delegatees);

        bytes[] memory blocksDyn = json.readBytesArray("$.blocks");
        assertEq(blocksDyn.length, 20, "fixture blocks length");
        bytes[20] memory blocks;
        for (uint256 i = 0; i < 20; i++) {
            blocks[i] = blocksDyn[i];
        }

        bytes memory encodedTx = json.readBytes("$.encodedTx");
        bytes32[] memory proof = json.readBytes32Array("$.proof");
        uint256 indexBits = json.readUint("$.indexBits");

        string memory contractType = json.readString("$.expected.contractType");
        bytes32 expectedTxId = json.readBytes32("$.txIdFromRawData");

        if (keccak256(bytes(contractType)) == keccak256(bytes("TriggerSmartContract"))) {
            bytes21 expectedSender = FixtureLoader.bytes21FromJson(json, "$.expected.senderTron");
            bytes21 expectedTo = FixtureLoader.bytes21FromJson(json, "$.expected.toTron");
            uint256 expectedCallValueSun = FixtureLoader.uint256FromBytes32Json(json, "$.expected.callValueSun");
            bytes memory expectedData = json.readBytes("$.expected.data");

            TriggerSmartContract memory tx_ = reader.readTriggerSmartContract(blocks, encodedTx, proof, indexBits);

            assertEq(tx_.txId, expectedTxId, "txId");
            assertEq(tx_.senderTron, expectedSender, "senderTron");
            assertEq(tx_.toTron, expectedTo, "toTron");
            assertEq(tx_.callValueSun, expectedCallValueSun, "callValueSun");
            assertEq(tx_.data, expectedData, "data");
        } else if (keccak256(bytes(contractType)) == keccak256(bytes("TransferContract"))) {
            bytes21 expectedSender = FixtureLoader.bytes21FromJson(json, "$.expected.senderTron");
            bytes21 expectedTo = FixtureLoader.bytes21FromJson(json, "$.expected.toTron");
            uint256 expectedAmountSun = FixtureLoader.uint256FromBytes32Json(json, "$.expected.amountSun");

            TransferContract memory t = reader.readTransferContract(blocks, encodedTx, proof, indexBits);

            assertEq(t.txId, expectedTxId, "txId");
            assertEq(t.senderTron, expectedSender, "senderTron");
            assertEq(t.toTron, expectedTo, "toTron");
            assertEq(t.amountSun, expectedAmountSun, "amountSun");
        } else if (keccak256(bytes(contractType)) == keccak256(bytes("DelegateResourceContract"))) {
            bytes21 expectedOwner = FixtureLoader.bytes21FromJson(json, "$.expected.ownerTron");
            bytes21 expectedReceiver = FixtureLoader.bytes21FromJson(json, "$.expected.receiverTron");
            uint256 expectedResource = json.readUint("$.expected.resource");
            uint256 expectedBalanceSun = FixtureLoader.uint256FromBytes32Json(json, "$.expected.balanceSun");
            bool expectedLock = json.readBool("$.expected.lock");
            uint256 expectedLockPeriod = FixtureLoader.uint256FromBytes32Json(json, "$.expected.lockPeriod");

            DelegateResourceContract memory d = reader.readDelegateResourceContract(blocks, encodedTx, proof, indexBits);

            assertEq(d.txId, expectedTxId, "txId");
            assertEq(d.ownerTron, expectedOwner, "ownerTron");
            assertEq(d.receiverTron, expectedReceiver, "receiverTron");
            assertEq(uint256(d.resource), expectedResource, "resource");
            assertEq(d.balanceSun, expectedBalanceSun, "balanceSun");
            assertEq(d.lock, expectedLock, "lock");
            assertEq(d.lockPeriod, expectedLockPeriod, "lockPeriod");
        } else {
            fail("unsupported fixture expected.contractType");
        }
    }

    function test_fixture_decodesRealTx_transferContract() public {
        _runFixture("test/tron/fixtures/transfer_79729218_eceb1b.json");
    }

    function test_fixture_decodesRealTx_triggerSmartContract() public {
        _runFixture("test/tron/fixtures/trigger_79729218_caf54b.json");
    }

    function test_fixture_decodesRealTx_delegateResourceContract() public {
        _runFixture("test/tron/fixtures/delegate_79707076_7fed5a.json");
    }

    function test_fixture_decodesRealTx_customFixturePath() public {
        string memory path = vm.envOr("TRON_TX_READER_FIXTURE", string(""));
        if (bytes(path).length == 0) return;
        _runFixture(path);
    }
}
