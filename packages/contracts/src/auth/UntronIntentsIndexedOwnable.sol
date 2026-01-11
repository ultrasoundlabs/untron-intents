// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {UntronIntentsIndex} from "../index/UntronIntentsIndex.sol";
import {IndexedOwnable} from "./IndexedOwnable.sol";

/// @title UntronIntents Indexed Ownable
/// @notice Ownable variant wired into UntronIntents' event-chain index.
/// @author Ultrasound Labs
abstract contract UntronIntentsIndexedOwnable is UntronIntentsIndex, IndexedOwnable {
    function _emitOwnershipTransferred(address oldOwner, address newOwner)
        internal
        override(IndexedOwnable, UntronIntentsIndex)
    {
        UntronIntentsIndex._emitOwnershipTransferred(oldOwner, newOwner);
    }
}
