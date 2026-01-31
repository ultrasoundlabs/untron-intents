// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {ProtoVarint, ProtoTruncated, ProtoInvalidWireType} from "./ProtoVarint.sol";
import {TronTxReaderErrors} from "../TronTxReaderErrors.sol";
import {
    TriggerSmartContract,
    TransferContract,
    DelegateResourceContract
} from "../external/interfaces/ITronTxReader.sol";

/// @title TronTxParser
/// @notice Narrow protobuf parser for Tron `Transaction` bytes used by the protocol.
/// @author Ultrasound Labs
library TronTxParser {
    struct TriggerHeaders {
        bytes21 ownerTron;
        bytes21 contractTron;
        uint256 callValueSun;
        uint256 dataStart;
        uint256 dataEnd;
    }

    struct TriggerParse {
        bytes32 txId;
        uint256 rawDataEnd;
        bytes21 ownerTron;
        bytes21 contractTron;
        uint256 callValueSun;
        uint256 dataStart;
        uint256 dataEnd;
    }

    // Protobuf wire types
    uint8 internal constant _WIRE_VARINT = 0;
    uint8 internal constant _WIRE_LENGTH_DELIMITED = 2;

    // Tron contract types
    uint64 internal constant _CONTRACT_TRANSFER = 1;
    uint64 internal constant _CONTRACT_TRIGGER_SMART = 31;
    uint64 internal constant _CONTRACT_DELEGATE_RESOURCE = 57;

    uint64 internal constant _MAX_INT64 = 0x7fffffffffffffff;

    // ---------------- Public parsers ----------------

    function parseTriggerSmartContract(bytes calldata encodedTx)
        internal
        pure
        returns (TriggerSmartContract memory callData)
    {
        TriggerParse memory p = _parseTriggerSmartContractMeta(encodedTx);
        if (p.dataStart == 0 && p.dataEnd == 0) revert TronTxReaderErrors.NotTriggerSmartContract();
        if (!_parseTxSuccess(encodedTx, p.rawDataEnd, encodedTx.length)) {
            revert TronTxReaderErrors.TronTxNotSuccessful();
        }

        callData.txId = p.txId;
        callData.senderTron = p.ownerTron;
        callData.toTron = p.contractTron;
        callData.callValueSun = p.callValueSun;
        callData.data = _slice(encodedTx, p.dataStart, p.dataEnd);
    }

    function parseTransferContract(bytes calldata encodedTx) internal pure returns (TransferContract memory transfer) {
        bytes32 txId;
        uint256 rawDataEnd;
        bytes21 ownerTron;
        bytes21 toTron;
        uint256 amountSun;

        {
            uint256 rawDataStart;
            (rawDataStart, rawDataEnd, txId) = _parseRawData(encodedTx);
            // solhint-disable-next-line gas-strict-inequalities
            assert(rawDataStart <= rawDataEnd && rawDataEnd <= encodedTx.length);
            (ownerTron, toTron, amountSun) = _parseTransferFromRawData(encodedTx, rawDataStart, rawDataEnd);
        }

        if (!_parseTxSuccess(encodedTx, rawDataEnd, encodedTx.length)) revert TronTxReaderErrors.TronTxNotSuccessful();

        transfer.txId = txId;
        transfer.senderTron = ownerTron;
        transfer.toTron = toTron;
        transfer.amountSun = amountSun;
    }

    function parseDelegateResourceContract(bytes calldata encodedTx)
        internal
        pure
        returns (DelegateResourceContract memory delegation)
    {
        bytes32 txId;
        uint256 rawDataEnd;
        bytes21 ownerTron;
        bytes21 receiverTron;
        uint256 balanceSun;
        uint8 resource;
        bool lock;
        uint256 lockPeriod;

        {
            uint256 rawDataStart;
            (rawDataStart, rawDataEnd, txId) = _parseRawData(encodedTx);
            // solhint-disable-next-line gas-strict-inequalities
            assert(rawDataStart <= rawDataEnd && rawDataEnd <= encodedTx.length);

            (ownerTron, receiverTron, resource, balanceSun, lock, lockPeriod) =
                _parseDelegateResourceFromRawData(encodedTx, rawDataStart, rawDataEnd);
        }

        if (!_parseTxSuccess(encodedTx, rawDataEnd, encodedTx.length)) revert TronTxReaderErrors.TronTxNotSuccessful();

        delegation.txId = txId;
        delegation.ownerTron = ownerTron;
        delegation.receiverTron = receiverTron;
        delegation.resource = resource;
        delegation.balanceSun = balanceSun;
        delegation.lock = lock;
        delegation.lockPeriod = lockPeriod;
    }

    // ---------------- TriggerSmartContract ----------------

    function _parseTriggerSmartContractMeta(bytes calldata encodedTx) private pure returns (TriggerParse memory p) {
        uint256 rawDataStart;
        (rawDataStart, p.rawDataEnd, p.txId) = _parseRawData(encodedTx);
        // solhint-disable-next-line gas-strict-inequalities
        assert(rawDataStart <= p.rawDataEnd && p.rawDataEnd <= encodedTx.length);

        TriggerHeaders memory h = _parseTriggerFromRawData(encodedTx, rawDataStart, p.rawDataEnd);
        p.ownerTron = h.ownerTron;
        p.contractTron = h.contractTron;
        p.callValueSun = h.callValueSun;
        p.dataStart = h.dataStart;
        p.dataEnd = h.dataEnd;
    }

    function _parseTriggerFromRawData(bytes calldata encodedTx, uint256 rawDataStart, uint256 rawDataEnd)
        private
        pure
        returns (TriggerHeaders memory h)
    {
        (uint256 cStart, uint256 cEnd, uint64 cType) = _readSingleContract(encodedTx, rawDataStart, rawDataEnd);
        // solhint-disable-next-line gas-strict-inequalities
        assert(cStart < cEnd && cEnd <= rawDataEnd);

        if (cType != _CONTRACT_TRIGGER_SMART) revert TronTxReaderErrors.NotTriggerSmartContract();

        (uint256 msgStart, uint256 msgEnd) = _extractContractParamValue(encodedTx, cStart, cEnd);
        if (msgStart == 0 && msgEnd == 0) revert TronTxReaderErrors.NotTriggerSmartContract();

        h = _parseTriggerHeaders(encodedTx, msgStart, msgEnd);
    }

    function _parseTriggerHeaders(bytes calldata encodedTx, uint256 trigStart, uint256 trigEnd)
        private
        pure
        returns (TriggerHeaders memory h)
    {
        uint256 trigCursor = trigStart;
        while (trigCursor < trigEnd) {
            (uint64 tFieldNum, uint64 tWireType, uint256 next) = _readKey(encodedTx, trigCursor, trigEnd);
            trigCursor = next;

            if (tFieldNum == 1 && tWireType == _WIRE_LENGTH_DELIMITED) {
                (uint256 oStart, uint256 oEnd, uint256 p) = _readLength(encodedTx, trigCursor, trigEnd);
                trigCursor = p;
                if (oEnd - oStart != 21) revert TronTxReaderErrors.TronInvalidOwnerLength();
                h.ownerTron = _readBytes21(encodedTx, oStart);
                if (uint8(h.ownerTron[0]) != 0x41) revert TronTxReaderErrors.TronInvalidOwnerPrefix();
            } else if (tFieldNum == 2 && tWireType == _WIRE_LENGTH_DELIMITED) {
                (uint256 cStart, uint256 cEnd, uint256 p) = _readLength(encodedTx, trigCursor, trigEnd);
                trigCursor = p;
                if (cEnd - cStart != 21) revert TronTxReaderErrors.TronInvalidContractLength();
                h.contractTron = _readBytes21(encodedTx, cStart);
                if (uint8(h.contractTron[0]) != 0x41) revert TronTxReaderErrors.TronInvalidContractPrefix();
            } else if (tFieldNum == 3 && tWireType == _WIRE_VARINT) {
                uint256 v;
                (v, trigCursor) = ProtoVarint.read(encodedTx, trigCursor, trigEnd);
                if (v > _MAX_INT64) revert TronTxReaderErrors.TronInvalidCallValue();
                h.callValueSun = v;
            } else if (tFieldNum == 4 && tWireType == _WIRE_LENGTH_DELIMITED) {
                (h.dataStart, h.dataEnd, trigCursor) = _readLength(encodedTx, trigCursor, trigEnd);
            } else {
                trigCursor = _skipField(encodedTx, trigCursor, trigEnd, tWireType);
            }
        }
    }

    // ---------------- TransferContract ----------------

    function _parseTransferFromRawData(bytes calldata encodedTx, uint256 rawDataStart, uint256 rawDataEnd)
        private
        pure
        returns (bytes21 ownerTron, bytes21 toTron, uint256 amountSun)
    {
        (uint256 cStart, uint256 cEnd, uint64 cType) = _readSingleContract(encodedTx, rawDataStart, rawDataEnd);
        // solhint-disable-next-line gas-strict-inequalities
        assert(cStart < cEnd && cEnd <= rawDataEnd);

        if (cType != _CONTRACT_TRANSFER) revert TronTxReaderErrors.NotTransferContract();

        (uint256 msgStart, uint256 msgEnd) = _extractContractParamValue(encodedTx, cStart, cEnd);
        if (msgStart == 0 && msgEnd == 0) revert TronTxReaderErrors.NotTransferContract();

        (ownerTron, toTron, amountSun) = _parseTransferHeaders(encodedTx, msgStart, msgEnd);
    }

    function _parseTransferHeaders(bytes calldata encodedTx, uint256 start, uint256 end)
        private
        pure
        returns (bytes21 ownerTron, bytes21 toTron, uint256 amountSun)
    {
        uint256 cursor = start;
        while (cursor < end) {
            (uint64 fieldNum, uint64 wireType, uint256 next) = _readKey(encodedTx, cursor, end);
            cursor = next;

            if (fieldNum == 1 && wireType == _WIRE_LENGTH_DELIMITED) {
                (uint256 oStart, uint256 oEnd, uint256 p) = _readLength(encodedTx, cursor, end);
                cursor = p;
                if (oEnd - oStart != 21) revert TronTxReaderErrors.TronInvalidOwnerLength();
                ownerTron = _readBytes21(encodedTx, oStart);
                if (uint8(ownerTron[0]) != 0x41) revert TronTxReaderErrors.TronInvalidOwnerPrefix();
            } else if (fieldNum == 2 && wireType == _WIRE_LENGTH_DELIMITED) {
                (uint256 tStart, uint256 tEnd, uint256 p) = _readLength(encodedTx, cursor, end);
                cursor = p;
                if (tEnd - tStart != 21) revert TronTxReaderErrors.TronInvalidReceiverLength();
                toTron = _readBytes21(encodedTx, tStart);
                if (uint8(toTron[0]) != 0x41) revert TronTxReaderErrors.TronInvalidReceiverPrefix();
            } else if (fieldNum == 3 && wireType == _WIRE_VARINT) {
                uint256 v;
                (v, cursor) = ProtoVarint.read(encodedTx, cursor, end);
                if (v > _MAX_INT64) revert TronTxReaderErrors.TronInvalidBalance();
                amountSun = v;
            } else {
                cursor = _skipField(encodedTx, cursor, end, wireType);
            }
        }
    }

    // ---------------- DelegateResourceContract ----------------

    function _parseDelegateResourceFromRawData(bytes calldata encodedTx, uint256 rawDataStart, uint256 rawDataEnd)
        private
        pure
        returns (
            bytes21 ownerTron,
            bytes21 receiverTron,
            uint8 resource,
            uint256 balanceSun,
            bool lock,
            uint256 lockPeriod
        )
    {
        (uint256 cStart, uint256 cEnd, uint64 cType) = _readSingleContract(encodedTx, rawDataStart, rawDataEnd);
        // solhint-disable-next-line gas-strict-inequalities
        assert(cStart < cEnd && cEnd <= rawDataEnd);

        if (cType != _CONTRACT_DELEGATE_RESOURCE) revert TronTxReaderErrors.NotDelegateResourceContract();

        (uint256 msgStart, uint256 msgEnd) = _extractContractParamValue(encodedTx, cStart, cEnd);
        if (msgStart == 0 && msgEnd == 0) revert TronTxReaderErrors.NotDelegateResourceContract();

        (ownerTron, receiverTron, resource, balanceSun, lock, lockPeriod) =
            _parseDelegateResourceHeaders(encodedTx, msgStart, msgEnd);
    }

    function _parseDelegateResourceHeaders(bytes calldata encodedTx, uint256 start, uint256 end)
        private
        pure
        returns (
            bytes21 ownerTron,
            bytes21 receiverTron,
            uint8 resource,
            uint256 balanceSun,
            bool lock,
            uint256 lockPeriod
        )
    {
        uint256 cursor = start;
        while (cursor < end) {
            (uint64 fieldNum, uint64 wireType, uint256 next) = _readKey(encodedTx, cursor, end);
            (cursor, ownerTron, receiverTron, resource, balanceSun, lock, lockPeriod) = _parseDelegateResourceField(
                encodedTx,
                next,
                end,
                fieldNum,
                wireType,
                ownerTron,
                receiverTron,
                resource,
                balanceSun,
                lock,
                lockPeriod
            );
        }
    }

    function _parseDelegateResourceField(
        bytes calldata encodedTx,
        uint256 cursor,
        uint256 end,
        uint64 fieldNum,
        uint64 wireType,
        bytes21 ownerTron,
        bytes21 receiverTron,
        uint8 resource,
        uint256 balanceSun,
        bool lock,
        uint256 lockPeriod
    )
        private
        pure
        returns (
            uint256 nextCursor,
            bytes21 nextOwnerTron,
            bytes21 nextReceiverTron,
            uint8 nextResource,
            uint256 nextBalanceSun,
            bool nextLock,
            uint256 nextLockPeriod
        )
    {
        if (fieldNum == 1 && wireType == _WIRE_LENGTH_DELIMITED) {
            (nextOwnerTron, nextCursor) = _parseTronAddress(encodedTx, cursor, end, true);
            return (nextCursor, nextOwnerTron, receiverTron, resource, balanceSun, lock, lockPeriod);
        }
        if (fieldNum == 2 && wireType == _WIRE_VARINT) {
            (nextResource, nextCursor) = _parseResource(encodedTx, cursor, end);
            return (nextCursor, ownerTron, receiverTron, nextResource, balanceSun, lock, lockPeriod);
        }
        if (fieldNum == 3 && wireType == _WIRE_VARINT) {
            (nextBalanceSun, nextCursor) = _parseBalanceSun(encodedTx, cursor, end);
            return (nextCursor, ownerTron, receiverTron, resource, nextBalanceSun, lock, lockPeriod);
        }
        if (fieldNum == 4 && wireType == _WIRE_LENGTH_DELIMITED) {
            (nextReceiverTron, nextCursor) = _parseTronAddress(encodedTx, cursor, end, false);
            return (nextCursor, ownerTron, nextReceiverTron, resource, balanceSun, lock, lockPeriod);
        }
        if (fieldNum == 5 && wireType == _WIRE_VARINT) {
            (nextLock, nextCursor) = _parseLock(encodedTx, cursor, end);
            return (nextCursor, ownerTron, receiverTron, resource, balanceSun, nextLock, lockPeriod);
        }
        if (fieldNum == 6 && wireType == _WIRE_VARINT) {
            (nextLockPeriod, nextCursor) = _parseLockPeriod(encodedTx, cursor, end);
            return (nextCursor, ownerTron, receiverTron, resource, balanceSun, lock, nextLockPeriod);
        }

        return
            (
                _skipField(encodedTx, cursor, end, wireType),
                ownerTron,
                receiverTron,
                resource,
                balanceSun,
                lock,
                lockPeriod
            );
    }

    function _parseTronAddress(bytes calldata encodedTx, uint256 cursor, uint256 end, bool isOwner)
        private
        pure
        returns (bytes21 tronAddress, uint256 nextCursor)
    {
        (uint256 start, uint256 end_, uint256 p) = _readLength(encodedTx, cursor, end);
        nextCursor = p;

        if (end_ - start != 21) {
            if (isOwner) revert TronTxReaderErrors.TronInvalidOwnerLength();
            revert TronTxReaderErrors.TronInvalidReceiverLength();
        }

        tronAddress = _readBytes21(encodedTx, start);
        if (uint8(tronAddress[0]) != 0x41) {
            if (isOwner) revert TronTxReaderErrors.TronInvalidOwnerPrefix();
            revert TronTxReaderErrors.TronInvalidReceiverPrefix();
        }
    }

    function _parseResource(bytes calldata encodedTx, uint256 cursor, uint256 end)
        private
        pure
        returns (uint8 resource, uint256 nextCursor)
    {
        uint256 v;
        (v, nextCursor) = ProtoVarint.read(encodedTx, cursor, end);
        if (v > type(uint8).max) revert TronTxReaderErrors.TronInvalidResource();
        // forge-lint: disable-next-line(unsafe-typecast)
        resource = uint8(v);
    }

    function _parseBalanceSun(bytes calldata encodedTx, uint256 cursor, uint256 end)
        private
        pure
        returns (uint256 balanceSun, uint256 nextCursor)
    {
        (balanceSun, nextCursor) = ProtoVarint.read(encodedTx, cursor, end);
        if (balanceSun > _MAX_INT64) revert TronTxReaderErrors.TronInvalidBalance();
    }

    function _parseLock(bytes calldata encodedTx, uint256 cursor, uint256 end)
        private
        pure
        returns (bool lock, uint256 nextCursor)
    {
        uint256 v;
        (v, nextCursor) = ProtoVarint.read(encodedTx, cursor, end);
        if (v == 0) return (false, nextCursor);
        if (v == 1) return (true, nextCursor);
        revert TronTxReaderErrors.TronInvalidLock();
    }

    function _parseLockPeriod(bytes calldata encodedTx, uint256 cursor, uint256 end)
        private
        pure
        returns (uint256 lockPeriod, uint256 nextCursor)
    {
        (lockPeriod, nextCursor) = ProtoVarint.read(encodedTx, cursor, end);
        if (lockPeriod > _MAX_INT64) revert TronTxReaderErrors.TronInvalidLockPeriod();
    }

    // ---------------- Transaction envelope parsing ----------------

    function _parseRawData(bytes calldata tx_)
        private
        pure
        returns (uint256 rawDataStart, uint256 rawDataEnd, bytes32 txId)
    {
        uint256 p = 0;
        while (p < tx_.length) {
            (uint64 fieldNum, uint64 wireType, uint256 next) = _readKey(tx_, p, tx_.length);
            p = next;
            if (fieldNum == 1 && wireType == _WIRE_LENGTH_DELIMITED) {
                (rawDataStart, rawDataEnd, p) = _readLength(tx_, p, tx_.length);
                txId = sha256(tx_[rawDataStart:rawDataEnd]);
                return (rawDataStart, rawDataEnd, txId);
            }
            p = _skipField(tx_, p, tx_.length, wireType);
        }
        revert ProtoTruncated();
    }

    function _parseTxSuccess(bytes calldata tx_, uint256 start, uint256 end) private pure returns (bool success) {
        uint256 p = start;
        while (p < end) {
            (uint64 fieldNum, uint64 wireType, uint256 next) = _readKey(tx_, p, end);
            p = next;
            if (fieldNum == 5 && wireType == _WIRE_LENGTH_DELIMITED) {
                (uint256 rStart, uint256 rEnd, uint256 p2) = _readLength(tx_, p, end);
                p = p2;
                if (_parseTxResultSuccess(tx_, rStart, rEnd)) return true;
            } else {
                p = _skipField(tx_, p, end, wireType);
            }
        }
    }

    function _parseTxResultSuccess(bytes calldata tx_, uint256 start, uint256 end) private pure returns (bool success) {
        uint256 p = start;
        while (p < end) {
            (uint64 fieldNum, uint64 wireType, uint256 next) = _readKey(tx_, p, end);
            p = next;
            // Transaction.Result.contractRet is field #3 (varint enum).
            if (fieldNum == 3 && wireType == _WIRE_VARINT) {
                (uint256 v,) = ProtoVarint.read(tx_, p, end);
                return v == 1;
            }
            p = _skipField(tx_, p, end, wireType);
        }
    }

    // ---------------- Contract parsing helpers ----------------

    function _readSingleContract(bytes calldata tx_, uint256 rawDataStart, uint256 rawDataEnd)
        private
        pure
        returns (uint256 contractStart, uint256 contractEnd, uint64 contractType)
    {
        uint256 p = rawDataStart;
        bool seenContract;

        while (p < rawDataEnd) {
            (uint64 fieldNum, uint64 wireType, uint256 next) = _readKey(tx_, p, rawDataEnd);
            p = next;
            if (fieldNum == 11 && wireType == _WIRE_LENGTH_DELIMITED) {
                // Enforce "exactly one" contract at the protobuf level.
                // Historical behavior: revert as NotTriggerSmartContract.
                if (seenContract) revert TronTxReaderErrors.NotTriggerSmartContract();
                (contractStart, contractEnd, p) = _readLength(tx_, p, rawDataEnd);
                seenContract = true;
                bool foundType;
                (contractType, foundType) = _readContractType(tx_, contractStart, contractEnd);
                if (!foundType) revert TronTxReaderErrors.NotTriggerSmartContract();
                break;
            } else {
                p = _skipField(tx_, p, rawDataEnd, wireType);
            }
        }

        if (!seenContract) revert TronTxReaderErrors.NotTriggerSmartContract();
    }

    function _readContractType(bytes calldata tx_, uint256 contractStart, uint256 contractEnd)
        private
        pure
        returns (uint64 contractType, bool foundType)
    {
        uint256 p = contractStart;
        while (p < contractEnd) {
            (uint64 cFieldNum, uint64 cWireType, uint256 next) = _readKey(tx_, p, contractEnd);
            p = next;
            if (cFieldNum == 1 && cWireType == _WIRE_VARINT) {
                (contractType,) = ProtoVarint.read(tx_, p, contractEnd);
                return (contractType, true);
            }
            p = _skipField(tx_, p, contractEnd, cWireType);
        }
    }

    function _extractContractParamValue(bytes calldata tx_, uint256 contractStart, uint256 contractEnd)
        private
        pure
        returns (uint256 msgStart, uint256 msgEnd)
    {
        uint256 p = contractStart;
        while (p < contractEnd) {
            (uint64 cFieldNum, uint64 cWireType, uint256 next) = _readKey(tx_, p, contractEnd);
            p = next;
            if (cFieldNum == 2 && cWireType == _WIRE_LENGTH_DELIMITED) {
                (uint256 paramStart, uint256 paramEnd, uint256 p2) = _readLength(tx_, p, contractEnd);
                p = p2;
                (msgStart, msgEnd) = _parseAnyValueField(tx_, paramStart, paramEnd);
            } else {
                p = _skipField(tx_, p, contractEnd, cWireType);
            }
        }
    }

    function _parseAnyValueField(bytes calldata encodedTx, uint256 paramStart, uint256 paramEnd)
        private
        pure
        returns (uint256 valueStart, uint256 valueEnd)
    {
        uint256 q = paramStart;
        while (q < paramEnd) {
            (uint64 anyFieldNum, uint64 anyWireType, uint256 next) = _readKey(encodedTx, q, paramEnd);
            q = next;
            if (anyFieldNum == 1 && anyWireType == _WIRE_LENGTH_DELIMITED) {
                (,, q) = _readLength(encodedTx, q, paramEnd);
            } else if (anyFieldNum == 2 && anyWireType == _WIRE_LENGTH_DELIMITED) {
                (valueStart, valueEnd, q) = _readLength(encodedTx, q, paramEnd);
            } else {
                q = _skipField(encodedTx, q, paramEnd, anyWireType);
            }
        }
    }

    // ---------------- Proto primitives ----------------

    function _readKey(bytes calldata data, uint256 start, uint256 end)
        private
        pure
        returns (uint64 fieldNum, uint64 wireType, uint256 next)
    {
        (uint256 key, uint256 p) = ProtoVarint.read(data, start, end);
        // fieldNum and wireType are extracted from a protobuf key varint:
        // key = (field_number << 3) | wire_type.
        // We only support keys that fit into 64 bits.
        if (key > type(uint64).max) revert ProtoInvalidWireType();
        // casting to 'uint64' is safe due to the explicit bound check above
        // forge-lint: disable-next-line(unsafe-typecast)
        fieldNum = uint64(key >> 3);
        // wireType is 3 bits (0..7), safe to cast
        // forge-lint: disable-next-line(unsafe-typecast)
        wireType = uint64(key & 7);
        next = p;
    }

    function _readLength(bytes calldata data, uint256 start, uint256 end)
        private
        pure
        returns (uint256 bodyStart, uint256 bodyEnd, uint256 next)
    {
        uint256 len;
        (len, next) = ProtoVarint.read(data, start, end);
        bodyStart = next;
        bodyEnd = bodyStart + len;
        if (bodyEnd > end) revert ProtoTruncated();
        next = bodyEnd;
    }

    function _skipField(bytes calldata data, uint256 start, uint256 end, uint64 wireType)
        private
        pure
        returns (uint256 next)
    {
        if (wireType == _WIRE_VARINT) {
            (, next) = ProtoVarint.read(data, start, end);
            return next;
        }
        if (wireType == _WIRE_LENGTH_DELIMITED) {
            (,, next) = _readLength(data, start, end);
            return next;
        }
        revert ProtoInvalidWireType();
    }

    function _readBytes21(bytes calldata data, uint256 start) private pure returns (bytes21 out) {
        if (start + 21 > data.length) revert ProtoTruncated();
        uint168 v;
        for (uint256 i = 0; i < 21; ++i) {
            v = (v << 8) | uint8(data[start + i]);
        }
        out = bytes21(v);
    }

    // ---------------- Utilities ----------------

    function _slice(bytes calldata data, uint256 start, uint256 end) private pure returns (bytes memory out) {
        if (end < start) revert ProtoTruncated();
        if (end > data.length) revert ProtoTruncated();
        out = data[start:end];
    }
}
