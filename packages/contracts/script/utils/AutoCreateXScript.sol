// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {console2} from "forge-std/console2.sol";
import {CreateXScript} from "createx-forge/script/CreateXScript.sol";
import {CREATEX_ADDRESS, CREATEX_BYTECODE} from "createx-forge/script/CreateX.d.sol";

/// @notice Extends `createx-forge`'s `CreateXScript` with an Anvil-aware auto-etch flow.
/// @dev `createx-forge` only etches CreateX for chainId=31337 (Forge internal VM). For
///      multi-chain Anvil setups we want non-31337 chain IDs, so we additionally etch via
///      `anvil_setCode` when connected to an Anvil RPC endpoint.
abstract contract AutoCreateXScript is CreateXScript {
    function setUp() public virtual {
        _ensureCreateX();
    }

    function _ensureCreateX() internal {
        if (isCreateXDeployed()) {
            return;
        }

        console2.log("CreateX missing at", CREATEX_ADDRESS, "chainId", block.chainid);

        // Forge internal VM default.
        if (block.chainid == 31337) {
            console2.log("Etching CreateX via vm.etch on chainId=31337");
            vm.etch(CREATEX_ADDRESS, CREATEX_BYTECODE);
            vm.label(CREATEX_ADDRESS, "CreateX");
            require(isCreateXDeployed(), "CreateX missing after vm.etch");
            return;
        }

        // If we're on an Anvil RPC, etch via JSON-RPC so it persists on the node.
        if (_isAnvilRpc()) {
            console2.log("Etching CreateX via anvil_setCode on chainId", block.chainid);
            _rpc(
                "anvil_setCode",
                string.concat("[\"", vm.toString(CREATEX_ADDRESS), "\",\"", vm.toString(CREATEX_BYTECODE), "\"]")
            );

            // `forge script` first runs a local simulation on a fork of the RPC state. Out-of-band JSON-RPC
            // state changes (like `anvil_setCode`) do not necessarily reflect in that local EVM instance,
            // so we also `vm.etch` to keep simulation and broadcast consistent.
            vm.etch(CREATEX_ADDRESS, CREATEX_BYTECODE);

            vm.label(CREATEX_ADDRESS, "CreateX");
            require(isCreateXDeployed(), "CreateX missing after anvil_setCode");
            return;
        }

        revert("CreateX not deployed on this chain; deploy CreateX or use Anvil");
    }

    function _isAnvilRpc() internal returns (bool) {
        // `web3_clientVersion` is supported by all clients; we just substring-match for "anvil".
        bytes memory resp = _rpc("web3_clientVersion", "[]");
        return _bytesContains(resp, "anvil") || _bytesContains(resp, "Anvil");
    }

    function _rpc(string memory method, string memory params) internal returns (bytes memory resp) {
        // Prefer an explicit RPC URL when running `forge script --rpc-url ...`; Forge does not always
        // treat that as the "current fork URL" for the 2-arg `vm.rpc` overload.
        string memory rpcUrl = vm.envOr("RPC_URL", string(""));
        if (bytes(rpcUrl).length != 0) {
            try vm.rpc(rpcUrl, method, params) returns (bytes memory r) {
                return r;
            } catch {
                // fall through to try the default overload
            }
        }

        try vm.rpc(method, params) returns (bytes memory r2) {
            return r2;
        } catch {
            revert("vm.rpc failed");
        }
    }

    function _bytesContains(bytes memory haystack, bytes memory needle) internal pure returns (bool) {
        if (needle.length == 0 || haystack.length < needle.length) return false;
        for (uint256 i = 0; i <= haystack.length - needle.length; i++) {
            bool ok = true;
            for (uint256 j = 0; j < needle.length; j++) {
                if (haystack[i + j] != needle[j]) {
                    ok = false;
                    break;
                }
            }
            if (ok) return true;
        }
        return false;
    }
}
