// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {IntentsForwarderIndex} from "../index/IntentsForwarderIndex.sol";
import {IndexedOwnable} from "./IndexedOwnable.sol";

/* solhint-disable no-inline-assembly */

/// @title IntentsForwarder Indexed Ownable
/// @notice Ownable variant wired into IntentsForwarder' event-chain index.
/// @author Ultrasound Labs
abstract contract IntentsForwarderIndexedOwnable is IntentsForwarderIndex, IndexedOwnable {
    function _emitOwnershipTransferred(address oldOwner, address newOwner)
        internal
        override(IndexedOwnable, IntentsForwarderIndex)
    {
        IntentsForwarderIndex._emitOwnershipTransferred(oldOwner, newOwner);
    }
}
