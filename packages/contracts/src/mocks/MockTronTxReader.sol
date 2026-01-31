// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {
    ITronTxReader,
    TriggerSmartContract,
    TransferContract,
    DelegateResourceContract
} from "../external/interfaces/ITronTxReader.sol";

contract MockTronTxReader is ITronTxReader {
    TriggerSmartContract internal _tx;
    TransferContract internal _transfer;
    DelegateResourceContract internal _delegation;

    function setTx(TriggerSmartContract calldata tx_) external {
        _tx = tx_;
    }

    function setTransferTx(TransferContract calldata tx_) external {
        _transfer = tx_;
    }

    function setDelegateResourceTx(DelegateResourceContract calldata tx_) external {
        _delegation = tx_;
    }

    function readTriggerSmartContract(bytes[20] calldata, bytes calldata, bytes32[] calldata, uint256)
        external
        view
        returns (TriggerSmartContract memory callData)
    {
        return _tx;
    }

    function readTransferContract(bytes[20] calldata, bytes calldata, bytes32[] calldata, uint256)
        external
        view
        returns (TransferContract memory transfer)
    {
        return _transfer;
    }

    function readDelegateResourceContract(bytes[20] calldata, bytes calldata, bytes32[] calldata, uint256)
        external
        view
        returns (DelegateResourceContract memory delegation)
    {
        return _delegation;
    }
}
