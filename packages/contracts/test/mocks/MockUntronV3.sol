// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {IUntronV3} from "../../src/external/interfaces/IUntronV3.sol";
import {ITronTxReader} from "../../src/external/interfaces/ITronTxReader.sol";

contract MockUntronV3 is IUntronV3 {
    ITronTxReader internal _reader;
    address internal _controller;
    address internal _tronUsdt;

    constructor(ITronTxReader reader_, address controller_, address tronUsdt_) {
        _reader = reader_;
        _controller = controller_;
        _tronUsdt = tronUsdt_;
    }

    function tronReader() external view returns (ITronTxReader) {
        return _reader;
    }

    function CONTROLLER_ADDRESS() external view returns (address) {
        return _controller;
    }

    function tronUsdt() external view returns (address) {
        return _tronUsdt;
    }
}

