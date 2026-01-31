// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

// Generic protobuf parsing errors (file-scoped so the library can use them)
error ProtoTruncated();
error ProtoInvalidWireType();

/// @title ProtoVarint
/// @notice Minimal protobuf varint reader/skip utilities for parsing Tron protobuf-encoded transactions.
/// @dev
/// - Protobuf "varint" encoding is used for both keys (fieldNum + wireType) and integer values.
/// - This library is intentionally tiny and opinionated: it supports only 64-bit varints and
///   reverts (instead of returning error codes) on malformed/truncated input.
/// @author Ultrasound Labs
library ProtoVarint {
    /// @notice Reads a protobuf varint from `data` starting at `pos`, bounded by `limit`.
    /// @dev Reverts with `ProtoTruncated()` if the varint is malformed or extends past `limit`.
    /// @param data The calldata byte array being parsed.
    /// @param pos The starting offset within `data` (inclusive).
    /// @param limit The maximum offset within `data` that may be read (exclusive).
    /// @return value The decoded varint value.
    /// @return newPos The first position in `data` after the varint.
    function read(bytes calldata data, uint256 pos, uint256 limit)
        internal
        pure
        returns (uint64 value, uint256 newPos)
    {
        uint64 v;
        uint64 shift;

        // Max 10 bytes for a 64-bit varint.
        for (uint256 i = 0; i < 10; ++i) {
            // solhint-disable-next-line gas-strict-inequalities
            if (pos >= limit) revert ProtoTruncated();
            // solhint-disable-next-line gas-increment-by-one
            uint8 b = uint8(data[pos++]);
            v |= uint64(b & 0x7F) << shift;
            if ((b & 0x80) == 0) {
                return (v, pos);
            }
            shift += 7;
        }

        // If we exit without returning, it’s malformed.
        revert ProtoTruncated();
    }

    /// @notice Skips over a protobuf varint at `pos`, bounded by `limit`.
    /// @dev Reverts with `ProtoTruncated()` if the varint is malformed or extends past `limit`.
    /// @param data The calldata byte array being parsed.
    /// @param pos The starting offset within `data` (inclusive).
    /// @param limit The maximum offset within `data` that may be read (exclusive).
    /// @return newPos The first position in `data` after the varint.
    function skip(bytes calldata data, uint256 pos, uint256 limit) internal pure returns (uint256 newPos) {
        // Max 10 bytes
        for (uint256 i = 0; i < 10; ++i) {
            // solhint-disable-next-line gas-strict-inequalities
            if (pos >= limit) revert ProtoTruncated();
            // solhint-disable-next-line gas-increment-by-one
            uint8 b = uint8(data[pos++]);
            if ((b & 0x80) == 0) {
                return pos;
            }
        }
        revert ProtoTruncated();
    }
}
