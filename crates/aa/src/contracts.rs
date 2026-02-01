alloy::sol! {
    #[sol(rpc)]
    interface IEntryPointNonces {
        function getNonce(address sender, uint192 key) external view returns (uint256);
    }

    #[sol(rpc)]
    interface IEntryPointDeposits {
        function balanceOf(address account) external view returns (uint256);
    }

    interface Safe4337Module {
        function executeUserOp(address to, uint256 value, bytes data, uint8 operation) external;
    }

    interface ISafe {
        function setup(
            address[] calldata owners,
            uint256 threshold,
            address to,
            bytes calldata data,
            address fallbackHandler,
            address paymentToken,
            uint256 payment,
            address payable paymentReceiver
        ) external;

        function isModuleEnabled(address module) external view returns (bool);
        function getFallbackHandler() external view returns (address);
        function getOwners() external view returns (address[] memory);
        function getThreshold() external view returns (uint256);
    }

    interface ISafeProxyFactory {
        function createProxyWithNonce(
            address singleton,
            bytes memory initializer,
            uint256 saltNonce
        ) external returns (address proxy);

        function createChainSpecificProxyWithNonce(
            address singleton,
            bytes memory initializer,
            uint256 saltNonce
        ) external returns (address proxy);

        // Optional helpers exposed by some SafeProxyFactory deployments. Used for offchain CREATE2 prediction.
        function proxyCreationCode() external pure returns (bytes memory);

        // Some deployments expose this directly (hash of `abi.encodePacked(proxyCreationCode(), uint256(uint160(singleton)))`).
        function proxyCreationCodehash(address singleton) external pure returns (bytes32);
    }

    interface ISafeModuleSetup {
        function enableModules(address[] calldata modules) external;
    }

    /// EIP-712 struct hashed by the module (see `Safe4337Module._getSafeOp`).
    struct SafeOp {
        address safe;
        uint256 nonce;
        bytes initCode;
        bytes callData;
        uint128 verificationGasLimit;
        uint128 callGasLimit;
        uint256 preVerificationGas;
        uint128 maxPriorityFeePerGas;
        uint128 maxFeePerGas;
        bytes paymasterAndData;
        uint48 validAfter;
        uint48 validUntil;
        address entryPoint;
    }
}
