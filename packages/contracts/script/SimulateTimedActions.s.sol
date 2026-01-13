// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {console2} from "forge-std/console2.sol";
import {stdJson} from "forge-std/StdJson.sol";

import {AutoCreateXScript} from "./utils/AutoCreateXScript.sol";
import {UntronIntents} from "../src/UntronIntents.sol";

/// @notice Executes time-gated protocol actions (e.g., `closeIntent`, `unclaimIntent`) after time has advanced.
/// @dev Reads state produced by `script/SimulateActivity.s.sol` from `out/activity-state-<chainid>.json`.
contract SimulateTimedActions is AutoCreateXScript {
    using stdJson for string;

    function run() public {
        // Not strictly required here, but keeps the script consistent with other scripts.
        _ensureCreateX();

        string memory path = string.concat("out/activity-state-", vm.toString(block.chainid), ".json");
        string memory json = vm.readFile(path);

        address intentsAddr = json.readAddress(".intents");
        bytes32 closeId = json.readBytes32(".closeIntentId");
        bytes32 unclaimId = json.readBytes32(".unclaimIntentId");

        UntronIntents intents = UntronIntents(intentsAddr);

        console2.log("chainId", block.chainid);
        console2.log("intents", intentsAddr);
        console2.log("closeIntentId");
        console2.logBytes32(closeId);
        console2.log("unclaimIntentId");
        console2.logBytes32(unclaimId);

        uint256 pk = vm.envOr("DEPLOYER_PK", uint256(0));
        if (pk != 0) vm.startBroadcast(pk);
        else vm.startBroadcast();

        // Close if expired.
        (,, uint256 closeDeadline,,,,) = intents.intents(closeId);
        if (closeDeadline != 0 && block.timestamp >= closeDeadline) {
            intents.closeIntent(closeId);
            console2.log("closed intent");
            console2.logBytes32(closeId);
        } else {
            console2.log("skip close (not expired or missing)");
        }

        // Unclaim if TIME_TO_FILL has passed and intent is still unsolved.
        (, uint256 solverClaimedAt,, address solver, bool solved,,) = intents.intents(unclaimId);
        if (solverClaimedAt != 0 && solver != address(0) && !solved) {
            if (block.timestamp >= solverClaimedAt + intents.TIME_TO_FILL()) {
                intents.unclaimIntent(unclaimId);
                console2.log("unclaimed intent");
                console2.logBytes32(unclaimId);
            } else {
                console2.log("skip unclaim (NotExpiredYet)");
            }
        } else {
            console2.log("skip unclaim (missing/not claimed/solved)");
        }

        vm.stopBroadcast();
    }
}
