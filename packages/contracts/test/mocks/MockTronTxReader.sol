// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {ITronTxReader, TriggerSmartContract} from "../../src/external/interfaces/ITronTxReader.sol";

contract MockTronTxReader is ITronTxReader {
    TriggerSmartContract internal _tx;

    function setTx(TriggerSmartContract calldata tx_) external {
        _tx = tx_;
    }

    function readTriggerSmartContract(bytes[20] calldata, bytes calldata, bytes32[] calldata, uint256)
        external
        view
        returns (TriggerSmartContract memory callData)
    {
        return _tx;
    }
}

