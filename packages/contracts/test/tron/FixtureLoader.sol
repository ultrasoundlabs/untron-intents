// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {stdJson} from "forge-std/StdJson.sol";

library FixtureLoader {
    using stdJson for string;

    function _bytes21(bytes memory b) private pure returns (bytes21 out) {
        require(b.length == 21, "FixtureLoader: expected 21 bytes");
        // solhint-disable-next-line no-inline-assembly
        assembly {
            out := mload(add(b, 0x20))
        }
    }

    function bytes21FromJson(string memory json, string memory key) internal pure returns (bytes21) {
        return _bytes21(json.readBytes(key));
    }

    function uint256FromBytes32Json(string memory json, string memory key) internal pure returns (uint256) {
        return uint256(json.readBytes32(key));
    }
}

