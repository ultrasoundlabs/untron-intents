//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
// AutoCreateXScript
//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

export const autoCreateXScriptAbi = [
  {
    type: 'function',
    inputs: [],
    name: 'IS_SCRIPT',
    outputs: [{ name: '', internalType: 'bool', type: 'bool' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [
      { name: 'salt', internalType: 'bytes32', type: 'bytes32' },
      { name: 'deployer', internalType: 'address', type: 'address' },
    ],
    name: 'computeCreate3Address',
    outputs: [{ name: '', internalType: 'address', type: 'address' }],
    stateMutability: 'pure',
  },
  {
    type: 'function',
    inputs: [
      { name: 'salt', internalType: 'bytes32', type: 'bytes32' },
      { name: 'initCode', internalType: 'bytes', type: 'bytes' },
    ],
    name: 'create3',
    outputs: [{ name: '', internalType: 'address', type: 'address' }],
    stateMutability: 'nonpayable',
  },
  {
    type: 'function',
    inputs: [],
    name: 'setUp',
    outputs: [],
    stateMutability: 'nonpayable',
  },
] as const

//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
// CCTPV2Bridger
//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

export const cctpv2BridgerAbi = [
  {
    type: 'constructor',
    inputs: [
      { name: 'owner', internalType: 'address', type: 'address' },
      { name: 'authorizedCaller', internalType: 'address', type: 'address' },
      { name: 'tokenMessengerV2', internalType: 'address', type: 'address' },
      { name: 'usdc', internalType: 'address', type: 'address' },
      {
        name: 'supportedChainIds',
        internalType: 'uint256[]',
        type: 'uint256[]',
      },
      { name: 'circleDomains', internalType: 'uint32[]', type: 'uint32[]' },
    ],
    stateMutability: 'nonpayable',
  },
  { type: 'receive', stateMutability: 'payable' },
  {
    type: 'function',
    inputs: [],
    name: 'AUTHORIZED_CALLER',
    outputs: [{ name: '', internalType: 'address', type: 'address' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'TOKEN_MESSENGER_V2',
    outputs: [
      { name: '', internalType: 'contract ITokenMessengerV2', type: 'address' },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'USDC',
    outputs: [{ name: '', internalType: 'contract IERC20', type: 'address' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [
      { name: 'inputToken', internalType: 'address', type: 'address' },
      { name: 'inputAmount', internalType: 'uint256', type: 'uint256' },
      { name: 'outputAddress', internalType: 'address', type: 'address' },
      { name: 'outputChainId', internalType: 'uint256', type: 'uint256' },
      { name: 'extraData', internalType: 'bytes', type: 'bytes' },
    ],
    name: 'bridge',
    outputs: [
      { name: 'expectedAmountOut', internalType: 'uint256', type: 'uint256' },
    ],
    stateMutability: 'payable',
  },
  {
    type: 'function',
    inputs: [],
    name: 'cancelOwnershipHandover',
    outputs: [],
    stateMutability: 'payable',
  },
  {
    type: 'function',
    inputs: [{ name: '', internalType: 'uint256', type: 'uint256' }],
    name: 'circleDomainByChainId',
    outputs: [{ name: '', internalType: 'uint32', type: 'uint32' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [
      { name: 'pendingOwner', internalType: 'address', type: 'address' },
    ],
    name: 'completeOwnershipHandover',
    outputs: [],
    stateMutability: 'payable',
  },
  {
    type: 'function',
    inputs: [{ name: '', internalType: 'uint256', type: 'uint256' }],
    name: 'isSupportedChainId',
    outputs: [{ name: '', internalType: 'bool', type: 'bool' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'owner',
    outputs: [{ name: 'result', internalType: 'address', type: 'address' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [
      { name: 'pendingOwner', internalType: 'address', type: 'address' },
    ],
    name: 'ownershipHandoverExpiresAt',
    outputs: [{ name: 'result', internalType: 'uint256', type: 'uint256' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'renounceOwnership',
    outputs: [],
    stateMutability: 'payable',
  },
  {
    type: 'function',
    inputs: [],
    name: 'requestOwnershipHandover',
    outputs: [],
    stateMutability: 'payable',
  },
  {
    type: 'function',
    inputs: [{ name: 'newOwner', internalType: 'address', type: 'address' }],
    name: 'transferOwnership',
    outputs: [],
    stateMutability: 'payable',
  },
  {
    type: 'function',
    inputs: [
      { name: 'token', internalType: 'address', type: 'address' },
      { name: 'amount', internalType: 'uint256', type: 'uint256' },
    ],
    name: 'withdraw',
    outputs: [],
    stateMutability: 'nonpayable',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'pendingOwner',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
    ],
    name: 'OwnershipHandoverCanceled',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'pendingOwner',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
    ],
    name: 'OwnershipHandoverRequested',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'oldOwner',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      {
        name: 'newOwner',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
    ],
    name: 'OwnershipTransferred',
  },
  { type: 'error', inputs: [], name: 'AlreadyInitialized' },
  { type: 'error', inputs: [], name: 'AmountZero' },
  {
    type: 'error',
    inputs: [
      { name: 'a', internalType: 'uint256', type: 'uint256' },
      { name: 'b', internalType: 'uint256', type: 'uint256' },
    ],
    name: 'ArrayLengthMismatch',
  },
  {
    type: 'error',
    inputs: [{ name: 'chainId', internalType: 'uint256', type: 'uint256' }],
    name: 'DuplicateChainId',
  },
  { type: 'error', inputs: [], name: 'InvalidExtraData' },
  { type: 'error', inputs: [], name: 'NewOwnerIsZeroAddress' },
  { type: 'error', inputs: [], name: 'NoHandoverRequest' },
  { type: 'error', inputs: [], name: 'NotAuthorizedCaller' },
  { type: 'error', inputs: [], name: 'Unauthorized' },
  {
    type: 'error',
    inputs: [{ name: 'chainId', internalType: 'uint256', type: 'uint256' }],
    name: 'UnsupportedChainId',
  },
  {
    type: 'error',
    inputs: [{ name: 'token', internalType: 'address', type: 'address' }],
    name: 'UnsupportedToken',
  },
  { type: 'error', inputs: [], name: 'ZeroAddress' },
  { type: 'error', inputs: [], name: 'ZeroOutputAddress' },
] as const

//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
// CreateXScript
//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

export const createXScriptAbi = [
  {
    type: 'function',
    inputs: [],
    name: 'IS_SCRIPT',
    outputs: [{ name: '', internalType: 'bool', type: 'bool' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [
      { name: 'salt', internalType: 'bytes32', type: 'bytes32' },
      { name: 'deployer', internalType: 'address', type: 'address' },
    ],
    name: 'computeCreate3Address',
    outputs: [{ name: '', internalType: 'address', type: 'address' }],
    stateMutability: 'pure',
  },
  {
    type: 'function',
    inputs: [
      { name: 'salt', internalType: 'bytes32', type: 'bytes32' },
      { name: 'initCode', internalType: 'bytes', type: 'bytes' },
    ],
    name: 'create3',
    outputs: [{ name: '', internalType: 'address', type: 'address' }],
    stateMutability: 'nonpayable',
  },
] as const

//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
// ExactBridger
//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

export const exactBridgerAbi = [
  { type: 'receive', stateMutability: 'payable' },
  {
    type: 'function',
    inputs: [
      { name: 'inputToken', internalType: 'address', type: 'address' },
      { name: 'inputAmount', internalType: 'uint256', type: 'uint256' },
      { name: 'outputAddress', internalType: 'address', type: 'address' },
      { name: 'outputChainId', internalType: 'uint256', type: 'uint256' },
      { name: 'extraData', internalType: 'bytes', type: 'bytes' },
    ],
    name: 'bridge',
    outputs: [
      { name: 'expectedAmountOut', internalType: 'uint256', type: 'uint256' },
    ],
    stateMutability: 'payable',
  },
  {
    type: 'function',
    inputs: [],
    name: 'lastExtraData',
    outputs: [{ name: '', internalType: 'bytes', type: 'bytes' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'lastInputAmount',
    outputs: [{ name: '', internalType: 'uint256', type: 'uint256' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'lastInputToken',
    outputs: [{ name: '', internalType: 'address', type: 'address' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'lastMsgValue',
    outputs: [{ name: '', internalType: 'uint256', type: 'uint256' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'lastOutputAddress',
    outputs: [{ name: '', internalType: 'address', type: 'address' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'lastOutputChainId',
    outputs: [{ name: '', internalType: 'uint256', type: 'uint256' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'refundToCaller',
    outputs: [{ name: '', internalType: 'uint256', type: 'uint256' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [
      { name: 'refundToCaller_', internalType: 'uint256', type: 'uint256' },
    ],
    name: 'setRefundToCaller',
    outputs: [],
    stateMutability: 'nonpayable',
  },
] as const

//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
// FeeBridger
//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

export const feeBridgerAbi = [
  {
    type: 'function',
    inputs: [
      { name: '', internalType: 'address', type: 'address' },
      { name: 'inputAmount', internalType: 'uint256', type: 'uint256' },
      { name: '', internalType: 'address', type: 'address' },
      { name: '', internalType: 'uint256', type: 'uint256' },
      { name: '', internalType: 'bytes', type: 'bytes' },
    ],
    name: 'bridge',
    outputs: [{ name: '', internalType: 'uint256', type: 'uint256' }],
    stateMutability: 'payable',
  },
  {
    type: 'function',
    inputs: [],
    name: 'fee',
    outputs: [{ name: '', internalType: 'uint256', type: 'uint256' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [{ name: 'fee_', internalType: 'uint256', type: 'uint256' }],
    name: 'setFee',
    outputs: [],
    stateMutability: 'nonpayable',
  },
] as const

//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
// ForwarderTestBase
//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

export const forwarderTestBaseAbi = [
  {
    type: 'function',
    inputs: [],
    name: 'IS_TEST',
    outputs: [{ name: '', internalType: 'bool', type: 'bool' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'excludeArtifacts',
    outputs: [
      {
        name: 'excludedArtifacts_',
        internalType: 'string[]',
        type: 'string[]',
      },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'excludeContracts',
    outputs: [
      {
        name: 'excludedContracts_',
        internalType: 'address[]',
        type: 'address[]',
      },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'excludeSelectors',
    outputs: [
      {
        name: 'excludedSelectors_',
        internalType: 'struct StdInvariant.FuzzSelector[]',
        type: 'tuple[]',
        components: [
          { name: 'addr', internalType: 'address', type: 'address' },
          { name: 'selectors', internalType: 'bytes4[]', type: 'bytes4[]' },
        ],
      },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'excludeSenders',
    outputs: [
      {
        name: 'excludedSenders_',
        internalType: 'address[]',
        type: 'address[]',
      },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'failed',
    outputs: [{ name: '', internalType: 'bool', type: 'bool' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'setUp',
    outputs: [],
    stateMutability: 'nonpayable',
  },
  {
    type: 'function',
    inputs: [],
    name: 'targetArtifactSelectors',
    outputs: [
      {
        name: 'targetedArtifactSelectors_',
        internalType: 'struct StdInvariant.FuzzArtifactSelector[]',
        type: 'tuple[]',
        components: [
          { name: 'artifact', internalType: 'string', type: 'string' },
          { name: 'selectors', internalType: 'bytes4[]', type: 'bytes4[]' },
        ],
      },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'targetArtifacts',
    outputs: [
      {
        name: 'targetedArtifacts_',
        internalType: 'string[]',
        type: 'string[]',
      },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'targetContracts',
    outputs: [
      {
        name: 'targetedContracts_',
        internalType: 'address[]',
        type: 'address[]',
      },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'targetInterfaces',
    outputs: [
      {
        name: 'targetedInterfaces_',
        internalType: 'struct StdInvariant.FuzzInterface[]',
        type: 'tuple[]',
        components: [
          { name: 'addr', internalType: 'address', type: 'address' },
          { name: 'artifacts', internalType: 'string[]', type: 'string[]' },
        ],
      },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'targetSelectors',
    outputs: [
      {
        name: 'targetedSelectors_',
        internalType: 'struct StdInvariant.FuzzSelector[]',
        type: 'tuple[]',
        components: [
          { name: 'addr', internalType: 'address', type: 'address' },
          { name: 'selectors', internalType: 'bytes4[]', type: 'bytes4[]' },
        ],
      },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'targetSenders',
    outputs: [
      {
        name: 'targetedSenders_',
        internalType: 'address[]',
        type: 'address[]',
      },
    ],
    stateMutability: 'view',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: '', internalType: 'string', type: 'string', indexed: false },
    ],
    name: 'log',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: '', internalType: 'address', type: 'address', indexed: false },
    ],
    name: 'log_address',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'val',
        internalType: 'uint256[]',
        type: 'uint256[]',
        indexed: false,
      },
    ],
    name: 'log_array',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'val',
        internalType: 'int256[]',
        type: 'int256[]',
        indexed: false,
      },
    ],
    name: 'log_array',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'val',
        internalType: 'address[]',
        type: 'address[]',
        indexed: false,
      },
    ],
    name: 'log_array',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: '', internalType: 'bytes', type: 'bytes', indexed: false },
    ],
    name: 'log_bytes',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: '', internalType: 'bytes32', type: 'bytes32', indexed: false },
    ],
    name: 'log_bytes32',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: '', internalType: 'int256', type: 'int256', indexed: false },
    ],
    name: 'log_int',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'key', internalType: 'string', type: 'string', indexed: false },
      { name: 'val', internalType: 'address', type: 'address', indexed: false },
    ],
    name: 'log_named_address',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'key', internalType: 'string', type: 'string', indexed: false },
      {
        name: 'val',
        internalType: 'uint256[]',
        type: 'uint256[]',
        indexed: false,
      },
    ],
    name: 'log_named_array',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'key', internalType: 'string', type: 'string', indexed: false },
      {
        name: 'val',
        internalType: 'int256[]',
        type: 'int256[]',
        indexed: false,
      },
    ],
    name: 'log_named_array',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'key', internalType: 'string', type: 'string', indexed: false },
      {
        name: 'val',
        internalType: 'address[]',
        type: 'address[]',
        indexed: false,
      },
    ],
    name: 'log_named_array',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'key', internalType: 'string', type: 'string', indexed: false },
      { name: 'val', internalType: 'bytes', type: 'bytes', indexed: false },
    ],
    name: 'log_named_bytes',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'key', internalType: 'string', type: 'string', indexed: false },
      { name: 'val', internalType: 'bytes32', type: 'bytes32', indexed: false },
    ],
    name: 'log_named_bytes32',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'key', internalType: 'string', type: 'string', indexed: false },
      { name: 'val', internalType: 'int256', type: 'int256', indexed: false },
      {
        name: 'decimals',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
    ],
    name: 'log_named_decimal_int',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'key', internalType: 'string', type: 'string', indexed: false },
      { name: 'val', internalType: 'uint256', type: 'uint256', indexed: false },
      {
        name: 'decimals',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
    ],
    name: 'log_named_decimal_uint',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'key', internalType: 'string', type: 'string', indexed: false },
      { name: 'val', internalType: 'int256', type: 'int256', indexed: false },
    ],
    name: 'log_named_int',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'key', internalType: 'string', type: 'string', indexed: false },
      { name: 'val', internalType: 'string', type: 'string', indexed: false },
    ],
    name: 'log_named_string',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'key', internalType: 'string', type: 'string', indexed: false },
      { name: 'val', internalType: 'uint256', type: 'uint256', indexed: false },
    ],
    name: 'log_named_uint',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: '', internalType: 'string', type: 'string', indexed: false },
    ],
    name: 'log_string',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: '', internalType: 'uint256', type: 'uint256', indexed: false },
    ],
    name: 'log_uint',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: '', internalType: 'bytes', type: 'bytes', indexed: false },
    ],
    name: 'logs',
  },
] as const

//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
// ICreateX
//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

export const iCreateXAbi = [
  {
    type: 'function',
    inputs: [
      { name: 'salt', internalType: 'bytes32', type: 'bytes32' },
      { name: 'initCodeHash', internalType: 'bytes32', type: 'bytes32' },
    ],
    name: 'computeCreate2Address',
    outputs: [
      { name: 'computedAddress', internalType: 'address', type: 'address' },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [
      { name: 'salt', internalType: 'bytes32', type: 'bytes32' },
      { name: 'initCodeHash', internalType: 'bytes32', type: 'bytes32' },
      { name: 'deployer', internalType: 'address', type: 'address' },
    ],
    name: 'computeCreate2Address',
    outputs: [
      { name: 'computedAddress', internalType: 'address', type: 'address' },
    ],
    stateMutability: 'pure',
  },
  {
    type: 'function',
    inputs: [
      { name: 'salt', internalType: 'bytes32', type: 'bytes32' },
      { name: 'deployer', internalType: 'address', type: 'address' },
    ],
    name: 'computeCreate3Address',
    outputs: [
      { name: 'computedAddress', internalType: 'address', type: 'address' },
    ],
    stateMutability: 'pure',
  },
  {
    type: 'function',
    inputs: [{ name: 'salt', internalType: 'bytes32', type: 'bytes32' }],
    name: 'computeCreate3Address',
    outputs: [
      { name: 'computedAddress', internalType: 'address', type: 'address' },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [{ name: 'nonce', internalType: 'uint256', type: 'uint256' }],
    name: 'computeCreateAddress',
    outputs: [
      { name: 'computedAddress', internalType: 'address', type: 'address' },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [
      { name: 'deployer', internalType: 'address', type: 'address' },
      { name: 'nonce', internalType: 'uint256', type: 'uint256' },
    ],
    name: 'computeCreateAddress',
    outputs: [
      { name: 'computedAddress', internalType: 'address', type: 'address' },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [{ name: 'initCode', internalType: 'bytes', type: 'bytes' }],
    name: 'deployCreate',
    outputs: [
      { name: 'newContract', internalType: 'address', type: 'address' },
    ],
    stateMutability: 'payable',
  },
  {
    type: 'function',
    inputs: [
      { name: 'salt', internalType: 'bytes32', type: 'bytes32' },
      { name: 'initCode', internalType: 'bytes', type: 'bytes' },
    ],
    name: 'deployCreate2',
    outputs: [
      { name: 'newContract', internalType: 'address', type: 'address' },
    ],
    stateMutability: 'payable',
  },
  {
    type: 'function',
    inputs: [{ name: 'initCode', internalType: 'bytes', type: 'bytes' }],
    name: 'deployCreate2',
    outputs: [
      { name: 'newContract', internalType: 'address', type: 'address' },
    ],
    stateMutability: 'payable',
  },
  {
    type: 'function',
    inputs: [
      { name: 'salt', internalType: 'bytes32', type: 'bytes32' },
      { name: 'initCode', internalType: 'bytes', type: 'bytes' },
      { name: 'data', internalType: 'bytes', type: 'bytes' },
      {
        name: 'values',
        internalType: 'struct ICreateX.Values',
        type: 'tuple',
        components: [
          {
            name: 'constructorAmount',
            internalType: 'uint256',
            type: 'uint256',
          },
          { name: 'initCallAmount', internalType: 'uint256', type: 'uint256' },
        ],
      },
      { name: 'refundAddress', internalType: 'address', type: 'address' },
    ],
    name: 'deployCreate2AndInit',
    outputs: [
      { name: 'newContract', internalType: 'address', type: 'address' },
    ],
    stateMutability: 'payable',
  },
  {
    type: 'function',
    inputs: [
      { name: 'initCode', internalType: 'bytes', type: 'bytes' },
      { name: 'data', internalType: 'bytes', type: 'bytes' },
      {
        name: 'values',
        internalType: 'struct ICreateX.Values',
        type: 'tuple',
        components: [
          {
            name: 'constructorAmount',
            internalType: 'uint256',
            type: 'uint256',
          },
          { name: 'initCallAmount', internalType: 'uint256', type: 'uint256' },
        ],
      },
    ],
    name: 'deployCreate2AndInit',
    outputs: [
      { name: 'newContract', internalType: 'address', type: 'address' },
    ],
    stateMutability: 'payable',
  },
  {
    type: 'function',
    inputs: [
      { name: 'initCode', internalType: 'bytes', type: 'bytes' },
      { name: 'data', internalType: 'bytes', type: 'bytes' },
      {
        name: 'values',
        internalType: 'struct ICreateX.Values',
        type: 'tuple',
        components: [
          {
            name: 'constructorAmount',
            internalType: 'uint256',
            type: 'uint256',
          },
          { name: 'initCallAmount', internalType: 'uint256', type: 'uint256' },
        ],
      },
      { name: 'refundAddress', internalType: 'address', type: 'address' },
    ],
    name: 'deployCreate2AndInit',
    outputs: [
      { name: 'newContract', internalType: 'address', type: 'address' },
    ],
    stateMutability: 'payable',
  },
  {
    type: 'function',
    inputs: [
      { name: 'salt', internalType: 'bytes32', type: 'bytes32' },
      { name: 'initCode', internalType: 'bytes', type: 'bytes' },
      { name: 'data', internalType: 'bytes', type: 'bytes' },
      {
        name: 'values',
        internalType: 'struct ICreateX.Values',
        type: 'tuple',
        components: [
          {
            name: 'constructorAmount',
            internalType: 'uint256',
            type: 'uint256',
          },
          { name: 'initCallAmount', internalType: 'uint256', type: 'uint256' },
        ],
      },
    ],
    name: 'deployCreate2AndInit',
    outputs: [
      { name: 'newContract', internalType: 'address', type: 'address' },
    ],
    stateMutability: 'payable',
  },
  {
    type: 'function',
    inputs: [
      { name: 'salt', internalType: 'bytes32', type: 'bytes32' },
      { name: 'implementation', internalType: 'address', type: 'address' },
      { name: 'data', internalType: 'bytes', type: 'bytes' },
    ],
    name: 'deployCreate2Clone',
    outputs: [{ name: 'proxy', internalType: 'address', type: 'address' }],
    stateMutability: 'payable',
  },
  {
    type: 'function',
    inputs: [
      { name: 'implementation', internalType: 'address', type: 'address' },
      { name: 'data', internalType: 'bytes', type: 'bytes' },
    ],
    name: 'deployCreate2Clone',
    outputs: [{ name: 'proxy', internalType: 'address', type: 'address' }],
    stateMutability: 'payable',
  },
  {
    type: 'function',
    inputs: [{ name: 'initCode', internalType: 'bytes', type: 'bytes' }],
    name: 'deployCreate3',
    outputs: [
      { name: 'newContract', internalType: 'address', type: 'address' },
    ],
    stateMutability: 'payable',
  },
  {
    type: 'function',
    inputs: [
      { name: 'salt', internalType: 'bytes32', type: 'bytes32' },
      { name: 'initCode', internalType: 'bytes', type: 'bytes' },
    ],
    name: 'deployCreate3',
    outputs: [
      { name: 'newContract', internalType: 'address', type: 'address' },
    ],
    stateMutability: 'payable',
  },
  {
    type: 'function',
    inputs: [
      { name: 'salt', internalType: 'bytes32', type: 'bytes32' },
      { name: 'initCode', internalType: 'bytes', type: 'bytes' },
      { name: 'data', internalType: 'bytes', type: 'bytes' },
      {
        name: 'values',
        internalType: 'struct ICreateX.Values',
        type: 'tuple',
        components: [
          {
            name: 'constructorAmount',
            internalType: 'uint256',
            type: 'uint256',
          },
          { name: 'initCallAmount', internalType: 'uint256', type: 'uint256' },
        ],
      },
    ],
    name: 'deployCreate3AndInit',
    outputs: [
      { name: 'newContract', internalType: 'address', type: 'address' },
    ],
    stateMutability: 'payable',
  },
  {
    type: 'function',
    inputs: [
      { name: 'initCode', internalType: 'bytes', type: 'bytes' },
      { name: 'data', internalType: 'bytes', type: 'bytes' },
      {
        name: 'values',
        internalType: 'struct ICreateX.Values',
        type: 'tuple',
        components: [
          {
            name: 'constructorAmount',
            internalType: 'uint256',
            type: 'uint256',
          },
          { name: 'initCallAmount', internalType: 'uint256', type: 'uint256' },
        ],
      },
    ],
    name: 'deployCreate3AndInit',
    outputs: [
      { name: 'newContract', internalType: 'address', type: 'address' },
    ],
    stateMutability: 'payable',
  },
  {
    type: 'function',
    inputs: [
      { name: 'salt', internalType: 'bytes32', type: 'bytes32' },
      { name: 'initCode', internalType: 'bytes', type: 'bytes' },
      { name: 'data', internalType: 'bytes', type: 'bytes' },
      {
        name: 'values',
        internalType: 'struct ICreateX.Values',
        type: 'tuple',
        components: [
          {
            name: 'constructorAmount',
            internalType: 'uint256',
            type: 'uint256',
          },
          { name: 'initCallAmount', internalType: 'uint256', type: 'uint256' },
        ],
      },
      { name: 'refundAddress', internalType: 'address', type: 'address' },
    ],
    name: 'deployCreate3AndInit',
    outputs: [
      { name: 'newContract', internalType: 'address', type: 'address' },
    ],
    stateMutability: 'payable',
  },
  {
    type: 'function',
    inputs: [
      { name: 'initCode', internalType: 'bytes', type: 'bytes' },
      { name: 'data', internalType: 'bytes', type: 'bytes' },
      {
        name: 'values',
        internalType: 'struct ICreateX.Values',
        type: 'tuple',
        components: [
          {
            name: 'constructorAmount',
            internalType: 'uint256',
            type: 'uint256',
          },
          { name: 'initCallAmount', internalType: 'uint256', type: 'uint256' },
        ],
      },
      { name: 'refundAddress', internalType: 'address', type: 'address' },
    ],
    name: 'deployCreate3AndInit',
    outputs: [
      { name: 'newContract', internalType: 'address', type: 'address' },
    ],
    stateMutability: 'payable',
  },
  {
    type: 'function',
    inputs: [
      { name: 'initCode', internalType: 'bytes', type: 'bytes' },
      { name: 'data', internalType: 'bytes', type: 'bytes' },
      {
        name: 'values',
        internalType: 'struct ICreateX.Values',
        type: 'tuple',
        components: [
          {
            name: 'constructorAmount',
            internalType: 'uint256',
            type: 'uint256',
          },
          { name: 'initCallAmount', internalType: 'uint256', type: 'uint256' },
        ],
      },
    ],
    name: 'deployCreateAndInit',
    outputs: [
      { name: 'newContract', internalType: 'address', type: 'address' },
    ],
    stateMutability: 'payable',
  },
  {
    type: 'function',
    inputs: [
      { name: 'initCode', internalType: 'bytes', type: 'bytes' },
      { name: 'data', internalType: 'bytes', type: 'bytes' },
      {
        name: 'values',
        internalType: 'struct ICreateX.Values',
        type: 'tuple',
        components: [
          {
            name: 'constructorAmount',
            internalType: 'uint256',
            type: 'uint256',
          },
          { name: 'initCallAmount', internalType: 'uint256', type: 'uint256' },
        ],
      },
      { name: 'refundAddress', internalType: 'address', type: 'address' },
    ],
    name: 'deployCreateAndInit',
    outputs: [
      { name: 'newContract', internalType: 'address', type: 'address' },
    ],
    stateMutability: 'payable',
  },
  {
    type: 'function',
    inputs: [
      { name: 'implementation', internalType: 'address', type: 'address' },
      { name: 'data', internalType: 'bytes', type: 'bytes' },
    ],
    name: 'deployCreateClone',
    outputs: [{ name: 'proxy', internalType: 'address', type: 'address' }],
    stateMutability: 'payable',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'newContract',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      { name: 'salt', internalType: 'bytes32', type: 'bytes32', indexed: true },
    ],
    name: 'ContractCreation',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'newContract',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
    ],
    name: 'ContractCreation',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'newContract',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      { name: 'salt', internalType: 'bytes32', type: 'bytes32', indexed: true },
    ],
    name: 'Create3ProxyContractCreation',
  },
  {
    type: 'error',
    inputs: [{ name: 'emitter', internalType: 'address', type: 'address' }],
    name: 'FailedContractCreation',
  },
  {
    type: 'error',
    inputs: [
      { name: 'emitter', internalType: 'address', type: 'address' },
      { name: 'revertData', internalType: 'bytes', type: 'bytes' },
    ],
    name: 'FailedContractInitialisation',
  },
  {
    type: 'error',
    inputs: [
      { name: 'emitter', internalType: 'address', type: 'address' },
      { name: 'revertData', internalType: 'bytes', type: 'bytes' },
    ],
    name: 'FailedEtherTransfer',
  },
  {
    type: 'error',
    inputs: [{ name: 'emitter', internalType: 'address', type: 'address' }],
    name: 'InvalidNonceValue',
  },
  {
    type: 'error',
    inputs: [{ name: 'emitter', internalType: 'address', type: 'address' }],
    name: 'InvalidSalt',
  },
] as const

//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
// IERC20
//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

export const ierc20Abi = [
  {
    type: 'function',
    inputs: [
      { name: 'owner', internalType: 'address', type: 'address' },
      { name: 'spender', internalType: 'address', type: 'address' },
    ],
    name: 'allowance',
    outputs: [{ name: '', internalType: 'uint256', type: 'uint256' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [
      { name: 'spender', internalType: 'address', type: 'address' },
      { name: 'value', internalType: 'uint256', type: 'uint256' },
    ],
    name: 'approve',
    outputs: [{ name: '', internalType: 'bool', type: 'bool' }],
    stateMutability: 'nonpayable',
  },
  {
    type: 'function',
    inputs: [{ name: 'account', internalType: 'address', type: 'address' }],
    name: 'balanceOf',
    outputs: [{ name: '', internalType: 'uint256', type: 'uint256' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'totalSupply',
    outputs: [{ name: '', internalType: 'uint256', type: 'uint256' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [
      { name: 'to', internalType: 'address', type: 'address' },
      { name: 'value', internalType: 'uint256', type: 'uint256' },
    ],
    name: 'transfer',
    outputs: [{ name: '', internalType: 'bool', type: 'bool' }],
    stateMutability: 'nonpayable',
  },
  {
    type: 'function',
    inputs: [
      { name: 'from', internalType: 'address', type: 'address' },
      { name: 'to', internalType: 'address', type: 'address' },
      { name: 'value', internalType: 'uint256', type: 'uint256' },
    ],
    name: 'transferFrom',
    outputs: [{ name: '', internalType: 'bool', type: 'bool' }],
    stateMutability: 'nonpayable',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'owner',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      {
        name: 'spender',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      {
        name: 'value',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
    ],
    name: 'Approval',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'from', internalType: 'address', type: 'address', indexed: true },
      { name: 'to', internalType: 'address', type: 'address', indexed: true },
      {
        name: 'value',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
    ],
    name: 'Transfer',
  },
] as const

//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
// IMintable
//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

export const iMintableAbi = [
  {
    type: 'function',
    inputs: [
      { name: 'to', internalType: 'address', type: 'address' },
      { name: 'amount', internalType: 'uint256', type: 'uint256' },
    ],
    name: 'mint',
    outputs: [],
    stateMutability: 'nonpayable',
  },
] as const

//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
// ITokenMessengerV2
//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

export const iTokenMessengerV2Abi = [
  {
    type: 'function',
    inputs: [
      { name: 'amount', internalType: 'uint256', type: 'uint256' },
      { name: 'destinationDomain', internalType: 'uint32', type: 'uint32' },
      { name: 'mintRecipient', internalType: 'bytes32', type: 'bytes32' },
      { name: 'burnToken', internalType: 'address', type: 'address' },
      { name: 'destinationCaller', internalType: 'bytes32', type: 'bytes32' },
      { name: 'maxFee', internalType: 'uint256', type: 'uint256' },
      { name: 'minFinalityThreshold', internalType: 'uint32', type: 'uint32' },
    ],
    name: 'depositForBurn',
    outputs: [],
    stateMutability: 'nonpayable',
  },
] as const

//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
// IntentsForwarder
//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

export const intentsForwarderAbi = [
  {
    type: 'constructor',
    inputs: [
      { name: '_usdt', internalType: 'address', type: 'address' },
      { name: '_usdc', internalType: 'address', type: 'address' },
      { name: '_owner', internalType: 'address', type: 'address' },
    ],
    stateMutability: 'nonpayable',
  },
  { type: 'receive', stateMutability: 'payable' },
  {
    type: 'function',
    inputs: [],
    name: 'RECEIVER_BYTECODE_HASH',
    outputs: [{ name: '', internalType: 'bytes32', type: 'bytes32' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'RECEIVER_IMPLEMENTATION',
    outputs: [{ name: '', internalType: 'address', type: 'address' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'SWAP_EXECUTOR',
    outputs: [
      { name: '', internalType: 'contract SwapExecutor', type: 'address' },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'USDC',
    outputs: [{ name: '', internalType: 'address', type: 'address' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'USDT',
    outputs: [{ name: '', internalType: 'address', type: 'address' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'eventChainTip',
    outputs: [{ name: '', internalType: 'bytes32', type: 'bytes32' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'eventSeq',
    outputs: [{ name: '', internalType: 'uint256', type: 'uint256' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [{ name: 'salt', internalType: 'bytes32', type: 'bytes32' }],
    name: 'getReceiver',
    outputs: [
      {
        name: 'receiver',
        internalType: 'contract UntronReceiver',
        type: 'address',
      },
    ],
    stateMutability: 'nonpayable',
  },
  {
    type: 'function',
    inputs: [],
    name: 'owner',
    outputs: [{ name: 'result', internalType: 'address', type: 'address' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [{ name: 'salt', internalType: 'bytes32', type: 'bytes32' }],
    name: 'predictReceiverAddress',
    outputs: [
      { name: 'predicted', internalType: 'address payable', type: 'address' },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [
      {
        name: 'req',
        internalType: 'struct IntentsForwarder.PullRequest',
        type: 'tuple',
        components: [
          { name: 'targetChain', internalType: 'uint256', type: 'uint256' },
          {
            name: 'beneficiary',
            internalType: 'address payable',
            type: 'address',
          },
          { name: 'beneficiaryClaimOnly', internalType: 'bool', type: 'bool' },
          { name: 'intentHash', internalType: 'bytes32', type: 'bytes32' },
          { name: 'forwardSalt', internalType: 'bytes32', type: 'bytes32' },
          { name: 'balance', internalType: 'uint256', type: 'uint256' },
          { name: 'tokenIn', internalType: 'address', type: 'address' },
          { name: 'tokenOut', internalType: 'address', type: 'address' },
          {
            name: 'swapData',
            internalType: 'struct Call[]',
            type: 'tuple[]',
            components: [
              { name: 'to', internalType: 'address', type: 'address' },
              { name: 'value', internalType: 'uint256', type: 'uint256' },
              { name: 'data', internalType: 'bytes', type: 'bytes' },
            ],
          },
          { name: 'bridgeData', internalType: 'bytes', type: 'bytes' },
        ],
      },
    ],
    name: 'pullFromReceiver',
    outputs: [{ name: 'amountOut', internalType: 'uint256', type: 'uint256' }],
    stateMutability: 'payable',
  },
  {
    type: 'function',
    inputs: [{ name: '', internalType: 'address', type: 'address' }],
    name: 'quoterByToken',
    outputs: [{ name: '', internalType: 'contract IQuoter', type: 'address' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'renounceOwnership',
    outputs: [],
    stateMutability: 'payable',
  },
  {
    type: 'function',
    inputs: [
      {
        name: '_usdtBridger',
        internalType: 'contract IBridger',
        type: 'address',
      },
      {
        name: '_usdcBridger',
        internalType: 'contract IBridger',
        type: 'address',
      },
    ],
    name: 'setBridgers',
    outputs: [],
    stateMutability: 'nonpayable',
  },
  {
    type: 'function',
    inputs: [
      { name: 'targetToken', internalType: 'address', type: 'address' },
      { name: 'quoter', internalType: 'contract IQuoter', type: 'address' },
    ],
    name: 'setQuoter',
    outputs: [],
    stateMutability: 'nonpayable',
  },
  {
    type: 'function',
    inputs: [{ name: 'newOwner', internalType: 'address', type: 'address' }],
    name: 'transferOwnership',
    outputs: [],
    stateMutability: 'payable',
  },
  {
    type: 'function',
    inputs: [],
    name: 'usdcBridger',
    outputs: [{ name: '', internalType: 'contract IBridger', type: 'address' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'usdtBridger',
    outputs: [{ name: '', internalType: 'contract IBridger', type: 'address' }],
    stateMutability: 'view',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'forwardId',
        internalType: 'bytes32',
        type: 'bytes32',
        indexed: true,
      },
      {
        name: 'bridger',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      {
        name: 'tokenOut',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      {
        name: 'amountIn',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
      {
        name: 'targetChain',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
    ],
    name: 'BridgeInitiated',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'usdtBridger',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      {
        name: 'usdcBridger',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
    ],
    name: 'BridgersSet',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'eventSeq',
        internalType: 'uint256',
        type: 'uint256',
        indexed: true,
      },
      {
        name: 'prevTip',
        internalType: 'bytes32',
        type: 'bytes32',
        indexed: true,
      },
      {
        name: 'newTip',
        internalType: 'bytes32',
        type: 'bytes32',
        indexed: true,
      },
      {
        name: 'eventSignature',
        internalType: 'bytes32',
        type: 'bytes32',
        indexed: false,
      },
      {
        name: 'abiEncodedEventData',
        internalType: 'bytes',
        type: 'bytes',
        indexed: false,
      },
    ],
    name: 'EventAppended',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'forwardId',
        internalType: 'bytes32',
        type: 'bytes32',
        indexed: true,
      },
      { name: 'ephemeral', internalType: 'bool', type: 'bool', indexed: false },
      {
        name: 'amountPulled',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
      {
        name: 'amountForwarded',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
      {
        name: 'relayerRebate',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
      {
        name: 'msgValueRefunded',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
      {
        name: 'settledLocally',
        internalType: 'bool',
        type: 'bool',
        indexed: false,
      },
      {
        name: 'bridger',
        internalType: 'address',
        type: 'address',
        indexed: false,
      },
      {
        name: 'expectedBridgeOut',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
      {
        name: 'bridgeDataHash',
        internalType: 'bytes32',
        type: 'bytes32',
        indexed: false,
      },
    ],
    name: 'ForwardCompleted',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'forwardId',
        internalType: 'bytes32',
        type: 'bytes32',
        indexed: true,
      },
      {
        name: 'baseReceiverSalt',
        internalType: 'bytes32',
        type: 'bytes32',
        indexed: true,
      },
      {
        name: 'forwardSalt',
        internalType: 'bytes32',
        type: 'bytes32',
        indexed: true,
      },
      {
        name: 'intentHash',
        internalType: 'bytes32',
        type: 'bytes32',
        indexed: false,
      },
      {
        name: 'targetChain',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
      {
        name: 'beneficiary',
        internalType: 'address',
        type: 'address',
        indexed: false,
      },
      {
        name: 'beneficiaryClaimOnly',
        internalType: 'bool',
        type: 'bool',
        indexed: false,
      },
      {
        name: 'balanceParam',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
      {
        name: 'tokenIn',
        internalType: 'address',
        type: 'address',
        indexed: false,
      },
      {
        name: 'tokenOut',
        internalType: 'address',
        type: 'address',
        indexed: false,
      },
      {
        name: 'receiverUsed',
        internalType: 'address',
        type: 'address',
        indexed: false,
      },
      {
        name: 'ephemeralReceiver',
        internalType: 'address',
        type: 'address',
        indexed: false,
      },
    ],
    name: 'ForwardStarted',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'oldOwner',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      {
        name: 'newOwner',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
    ],
    name: 'OwnershipTransferred',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'tokenIn',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      {
        name: 'quoter',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
    ],
    name: 'QuoterSet',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'receiverSalt',
        internalType: 'bytes32',
        type: 'bytes32',
        indexed: true,
      },
      {
        name: 'receiver',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      {
        name: 'implementation',
        internalType: 'address',
        type: 'address',
        indexed: false,
      },
    ],
    name: 'ReceiverDeployed',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'forwardId',
        internalType: 'bytes32',
        type: 'bytes32',
        indexed: true,
      },
      {
        name: 'tokenIn',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      {
        name: 'tokenOut',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      {
        name: 'minOut',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
      {
        name: 'actualOut',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
    ],
    name: 'SwapExecuted',
  },
  { type: 'error', inputs: [], name: 'AlreadyInitialized' },
  { type: 'error', inputs: [], name: 'InsufficientOutputAmount' },
  { type: 'error', inputs: [], name: 'NewOwnerIsZeroAddress' },
  { type: 'error', inputs: [], name: 'PullerUnauthorized' },
  { type: 'error', inputs: [], name: 'SwapOnEphemeralReceiversNotAllowed' },
  { type: 'error', inputs: [], name: 'Unauthorized' },
  { type: 'error', inputs: [], name: 'UnsupportedOutputToken' },
] as const

//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
// IntentsForwarderIndex
//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

export const intentsForwarderIndexAbi = [
  {
    type: 'function',
    inputs: [],
    name: 'eventChainTip',
    outputs: [{ name: '', internalType: 'bytes32', type: 'bytes32' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'eventSeq',
    outputs: [{ name: '', internalType: 'uint256', type: 'uint256' }],
    stateMutability: 'view',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'forwardId',
        internalType: 'bytes32',
        type: 'bytes32',
        indexed: true,
      },
      {
        name: 'bridger',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      {
        name: 'tokenOut',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      {
        name: 'amountIn',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
      {
        name: 'targetChain',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
    ],
    name: 'BridgeInitiated',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'usdtBridger',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      {
        name: 'usdcBridger',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
    ],
    name: 'BridgersSet',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'eventSeq',
        internalType: 'uint256',
        type: 'uint256',
        indexed: true,
      },
      {
        name: 'prevTip',
        internalType: 'bytes32',
        type: 'bytes32',
        indexed: true,
      },
      {
        name: 'newTip',
        internalType: 'bytes32',
        type: 'bytes32',
        indexed: true,
      },
      {
        name: 'eventSignature',
        internalType: 'bytes32',
        type: 'bytes32',
        indexed: false,
      },
      {
        name: 'abiEncodedEventData',
        internalType: 'bytes',
        type: 'bytes',
        indexed: false,
      },
    ],
    name: 'EventAppended',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'forwardId',
        internalType: 'bytes32',
        type: 'bytes32',
        indexed: true,
      },
      { name: 'ephemeral', internalType: 'bool', type: 'bool', indexed: false },
      {
        name: 'amountPulled',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
      {
        name: 'amountForwarded',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
      {
        name: 'relayerRebate',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
      {
        name: 'msgValueRefunded',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
      {
        name: 'settledLocally',
        internalType: 'bool',
        type: 'bool',
        indexed: false,
      },
      {
        name: 'bridger',
        internalType: 'address',
        type: 'address',
        indexed: false,
      },
      {
        name: 'expectedBridgeOut',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
      {
        name: 'bridgeDataHash',
        internalType: 'bytes32',
        type: 'bytes32',
        indexed: false,
      },
    ],
    name: 'ForwardCompleted',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'forwardId',
        internalType: 'bytes32',
        type: 'bytes32',
        indexed: true,
      },
      {
        name: 'baseReceiverSalt',
        internalType: 'bytes32',
        type: 'bytes32',
        indexed: true,
      },
      {
        name: 'forwardSalt',
        internalType: 'bytes32',
        type: 'bytes32',
        indexed: true,
      },
      {
        name: 'intentHash',
        internalType: 'bytes32',
        type: 'bytes32',
        indexed: false,
      },
      {
        name: 'targetChain',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
      {
        name: 'beneficiary',
        internalType: 'address',
        type: 'address',
        indexed: false,
      },
      {
        name: 'beneficiaryClaimOnly',
        internalType: 'bool',
        type: 'bool',
        indexed: false,
      },
      {
        name: 'balanceParam',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
      {
        name: 'tokenIn',
        internalType: 'address',
        type: 'address',
        indexed: false,
      },
      {
        name: 'tokenOut',
        internalType: 'address',
        type: 'address',
        indexed: false,
      },
      {
        name: 'receiverUsed',
        internalType: 'address',
        type: 'address',
        indexed: false,
      },
      {
        name: 'ephemeralReceiver',
        internalType: 'address',
        type: 'address',
        indexed: false,
      },
    ],
    name: 'ForwardStarted',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'oldOwner',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      {
        name: 'newOwner',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
    ],
    name: 'OwnershipTransferred',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'tokenIn',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      {
        name: 'quoter',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
    ],
    name: 'QuoterSet',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'receiverSalt',
        internalType: 'bytes32',
        type: 'bytes32',
        indexed: true,
      },
      {
        name: 'receiver',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      {
        name: 'implementation',
        internalType: 'address',
        type: 'address',
        indexed: false,
      },
    ],
    name: 'ReceiverDeployed',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'forwardId',
        internalType: 'bytes32',
        type: 'bytes32',
        indexed: true,
      },
      {
        name: 'tokenIn',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      {
        name: 'tokenOut',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      {
        name: 'minOut',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
      {
        name: 'actualOut',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
    ],
    name: 'SwapExecuted',
  },
] as const

//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
// MockForwarderPuller
//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

export const mockForwarderPullerAbi = [
  {
    type: 'function',
    inputs: [],
    name: 'lastCall',
    outputs: [
      { name: 'targetChain', internalType: 'uint256', type: 'uint256' },
      { name: 'beneficiary', internalType: 'address', type: 'address' },
      { name: 'beneficiaryClaimOnly', internalType: 'bool', type: 'bool' },
      { name: 'intentHash', internalType: 'bytes32', type: 'bytes32' },
      { name: 'forwardSalt', internalType: 'bytes32', type: 'bytes32' },
      { name: 'balance', internalType: 'uint256', type: 'uint256' },
      { name: 'tokenIn', internalType: 'address', type: 'address' },
      { name: 'tokenOut', internalType: 'address', type: 'address' },
      { name: 'swapDataHash', internalType: 'bytes32', type: 'bytes32' },
      { name: 'bridgeDataHash', internalType: 'bytes32', type: 'bytes32' },
      { name: 'msgValue', internalType: 'uint256', type: 'uint256' },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'nextPull',
    outputs: [
      { name: 'token', internalType: 'address', type: 'address' },
      { name: 'amount', internalType: 'uint256', type: 'uint256' },
      { name: 'shouldRevert', internalType: 'bool', type: 'bool' },
      { name: 'enforceBalanceParam', internalType: 'bool', type: 'bool' },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [
      {
        name: 'req',
        internalType: 'struct IntentsForwarder.PullRequest',
        type: 'tuple',
        components: [
          { name: 'targetChain', internalType: 'uint256', type: 'uint256' },
          {
            name: 'beneficiary',
            internalType: 'address payable',
            type: 'address',
          },
          { name: 'beneficiaryClaimOnly', internalType: 'bool', type: 'bool' },
          { name: 'intentHash', internalType: 'bytes32', type: 'bytes32' },
          { name: 'forwardSalt', internalType: 'bytes32', type: 'bytes32' },
          { name: 'balance', internalType: 'uint256', type: 'uint256' },
          { name: 'tokenIn', internalType: 'address', type: 'address' },
          { name: 'tokenOut', internalType: 'address', type: 'address' },
          {
            name: 'swapData',
            internalType: 'struct Call[]',
            type: 'tuple[]',
            components: [
              { name: 'to', internalType: 'address', type: 'address' },
              { name: 'value', internalType: 'uint256', type: 'uint256' },
              { name: 'data', internalType: 'bytes', type: 'bytes' },
            ],
          },
          { name: 'bridgeData', internalType: 'bytes', type: 'bytes' },
        ],
      },
    ],
    name: 'pullFromReceiver',
    outputs: [{ name: 'amountOut', internalType: 'uint256', type: 'uint256' }],
    stateMutability: 'payable',
  },
  {
    type: 'function',
    inputs: [
      { name: 'enforceBalanceParam_', internalType: 'bool', type: 'bool' },
    ],
    name: 'setEnforceBalanceParam',
    outputs: [],
    stateMutability: 'nonpayable',
  },
  {
    type: 'function',
    inputs: [
      { name: 'token', internalType: 'address', type: 'address' },
      { name: 'amount', internalType: 'uint256', type: 'uint256' },
    ],
    name: 'setNextPull',
    outputs: [],
    stateMutability: 'nonpayable',
  },
  {
    type: 'function',
    inputs: [{ name: 'shouldRevert_', internalType: 'bool', type: 'bool' }],
    name: 'setShouldRevert',
    outputs: [],
    stateMutability: 'nonpayable',
  },
  {
    type: 'error',
    inputs: [
      { name: 'expected', internalType: 'uint256', type: 'uint256' },
      { name: 'got', internalType: 'uint256', type: 'uint256' },
    ],
    name: 'BalanceMismatch',
  },
  { type: 'error', inputs: [], name: 'RevertPull' },
] as const

//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
// MockQuoter
//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

export const mockQuoterAbi = [
  {
    type: 'function',
    inputs: [],
    name: 'amountOut',
    outputs: [{ name: '', internalType: 'uint256', type: 'uint256' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [
      { name: '', internalType: 'address', type: 'address' },
      { name: '', internalType: 'address', type: 'address' },
      { name: '', internalType: 'uint256', type: 'uint256' },
      { name: '', internalType: 'uint256', type: 'uint256' },
    ],
    name: 'quote',
    outputs: [{ name: '', internalType: 'uint256', type: 'uint256' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [{ name: 'amountOut_', internalType: 'uint256', type: 'uint256' }],
    name: 'setAmountOut',
    outputs: [],
    stateMutability: 'nonpayable',
  },
  {
    type: 'function',
    inputs: [{ name: 'shouldRevert_', internalType: 'bool', type: 'bool' }],
    name: 'setShouldRevert',
    outputs: [],
    stateMutability: 'nonpayable',
  },
  {
    type: 'function',
    inputs: [],
    name: 'shouldRevert',
    outputs: [{ name: '', internalType: 'bool', type: 'bool' }],
    stateMutability: 'view',
  },
  { type: 'error', inputs: [], name: 'RevertQuote' },
] as const

//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
// MockReverter
//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

export const mockReverterAbi = [
  {
    type: 'function',
    inputs: [],
    name: 'boom',
    outputs: [],
    stateMutability: 'pure',
  },
] as const

//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
// MockTransportBridger
//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

export const mockTransportBridgerAbi = [
  {
    type: 'function',
    inputs: [
      { name: 'inputToken', internalType: 'address', type: 'address' },
      { name: 'inputAmount', internalType: 'uint256', type: 'uint256' },
      { name: 'outputAddress', internalType: 'address', type: 'address' },
      { name: 'outputChainId', internalType: 'uint256', type: 'uint256' },
      { name: 'extraData', internalType: 'bytes', type: 'bytes' },
    ],
    name: 'bridge',
    outputs: [
      { name: 'expectedAmountOut', internalType: 'uint256', type: 'uint256' },
    ],
    stateMutability: 'payable',
  },
  {
    type: 'function',
    inputs: [],
    name: 'deliverLast',
    outputs: [],
    stateMutability: 'nonpayable',
  },
  {
    type: 'function',
    inputs: [],
    name: 'last',
    outputs: [
      { name: 'inputToken', internalType: 'address', type: 'address' },
      { name: 'inputAmount', internalType: 'uint256', type: 'uint256' },
      { name: 'outputAddress', internalType: 'address', type: 'address' },
      { name: 'outputChainId', internalType: 'uint256', type: 'uint256' },
      { name: 'extraData', internalType: 'bytes', type: 'bytes' },
      { name: 'delivered', internalType: 'bool', type: 'bool' },
    ],
    stateMutability: 'view',
  },
] as const

//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
// MockTronTxReader
//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

export const mockTronTxReaderAbi = [
  {
    type: 'function',
    inputs: [
      { name: '', internalType: 'bytes[20]', type: 'bytes[20]' },
      { name: '', internalType: 'bytes', type: 'bytes' },
      { name: '', internalType: 'bytes32[]', type: 'bytes32[]' },
      { name: '', internalType: 'uint256', type: 'uint256' },
    ],
    name: 'readDelegateResourceContract',
    outputs: [
      {
        name: 'delegation',
        internalType: 'struct DelegateResourceContract',
        type: 'tuple',
        components: [
          { name: 'txId', internalType: 'bytes32', type: 'bytes32' },
          { name: 'tronBlockNumber', internalType: 'uint256', type: 'uint256' },
          { name: 'balanceSun', internalType: 'uint256', type: 'uint256' },
          { name: 'lockPeriod', internalType: 'uint256', type: 'uint256' },
          { name: 'ownerTron', internalType: 'bytes21', type: 'bytes21' },
          { name: 'receiverTron', internalType: 'bytes21', type: 'bytes21' },
          {
            name: 'tronBlockTimestamp',
            internalType: 'uint32',
            type: 'uint32',
          },
          { name: 'resource', internalType: 'uint8', type: 'uint8' },
          { name: 'lock', internalType: 'bool', type: 'bool' },
        ],
      },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [
      { name: '', internalType: 'bytes[20]', type: 'bytes[20]' },
      { name: '', internalType: 'bytes', type: 'bytes' },
      { name: '', internalType: 'bytes32[]', type: 'bytes32[]' },
      { name: '', internalType: 'uint256', type: 'uint256' },
    ],
    name: 'readTransferContract',
    outputs: [
      {
        name: 'transfer',
        internalType: 'struct TransferContract',
        type: 'tuple',
        components: [
          { name: 'txId', internalType: 'bytes32', type: 'bytes32' },
          { name: 'tronBlockNumber', internalType: 'uint256', type: 'uint256' },
          {
            name: 'tronBlockTimestamp',
            internalType: 'uint32',
            type: 'uint32',
          },
          { name: 'senderTron', internalType: 'bytes21', type: 'bytes21' },
          { name: 'toTron', internalType: 'bytes21', type: 'bytes21' },
          { name: 'amountSun', internalType: 'uint256', type: 'uint256' },
        ],
      },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [
      { name: '', internalType: 'bytes[20]', type: 'bytes[20]' },
      { name: '', internalType: 'bytes', type: 'bytes' },
      { name: '', internalType: 'bytes32[]', type: 'bytes32[]' },
      { name: '', internalType: 'uint256', type: 'uint256' },
    ],
    name: 'readTriggerSmartContract',
    outputs: [
      {
        name: 'callData',
        internalType: 'struct TriggerSmartContract',
        type: 'tuple',
        components: [
          { name: 'txId', internalType: 'bytes32', type: 'bytes32' },
          { name: 'tronBlockNumber', internalType: 'uint256', type: 'uint256' },
          {
            name: 'tronBlockTimestamp',
            internalType: 'uint32',
            type: 'uint32',
          },
          { name: 'senderTron', internalType: 'bytes21', type: 'bytes21' },
          { name: 'toTron', internalType: 'bytes21', type: 'bytes21' },
          { name: 'callValueSun', internalType: 'uint256', type: 'uint256' },
          { name: 'data', internalType: 'bytes', type: 'bytes' },
        ],
      },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [
      {
        name: 'tx_',
        internalType: 'struct DelegateResourceContract',
        type: 'tuple',
        components: [
          { name: 'txId', internalType: 'bytes32', type: 'bytes32' },
          { name: 'tronBlockNumber', internalType: 'uint256', type: 'uint256' },
          { name: 'balanceSun', internalType: 'uint256', type: 'uint256' },
          { name: 'lockPeriod', internalType: 'uint256', type: 'uint256' },
          { name: 'ownerTron', internalType: 'bytes21', type: 'bytes21' },
          { name: 'receiverTron', internalType: 'bytes21', type: 'bytes21' },
          {
            name: 'tronBlockTimestamp',
            internalType: 'uint32',
            type: 'uint32',
          },
          { name: 'resource', internalType: 'uint8', type: 'uint8' },
          { name: 'lock', internalType: 'bool', type: 'bool' },
        ],
      },
    ],
    name: 'setDelegateResourceTx',
    outputs: [],
    stateMutability: 'nonpayable',
  },
  {
    type: 'function',
    inputs: [
      {
        name: 'tx_',
        internalType: 'struct TransferContract',
        type: 'tuple',
        components: [
          { name: 'txId', internalType: 'bytes32', type: 'bytes32' },
          { name: 'tronBlockNumber', internalType: 'uint256', type: 'uint256' },
          {
            name: 'tronBlockTimestamp',
            internalType: 'uint32',
            type: 'uint32',
          },
          { name: 'senderTron', internalType: 'bytes21', type: 'bytes21' },
          { name: 'toTron', internalType: 'bytes21', type: 'bytes21' },
          { name: 'amountSun', internalType: 'uint256', type: 'uint256' },
        ],
      },
    ],
    name: 'setTransferTx',
    outputs: [],
    stateMutability: 'nonpayable',
  },
  {
    type: 'function',
    inputs: [
      {
        name: 'tx_',
        internalType: 'struct TriggerSmartContract',
        type: 'tuple',
        components: [
          { name: 'txId', internalType: 'bytes32', type: 'bytes32' },
          { name: 'tronBlockNumber', internalType: 'uint256', type: 'uint256' },
          {
            name: 'tronBlockTimestamp',
            internalType: 'uint32',
            type: 'uint32',
          },
          { name: 'senderTron', internalType: 'bytes21', type: 'bytes21' },
          { name: 'toTron', internalType: 'bytes21', type: 'bytes21' },
          { name: 'callValueSun', internalType: 'uint256', type: 'uint256' },
          { name: 'data', internalType: 'bytes', type: 'bytes' },
        ],
      },
    ],
    name: 'setTx',
    outputs: [],
    stateMutability: 'nonpayable',
  },
] as const

//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
// MockUntronV3
//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

export const mockUntronV3Abi = [
  {
    type: 'constructor',
    inputs: [
      {
        name: 'reader_',
        internalType: 'contract ITronTxReader',
        type: 'address',
      },
      { name: 'controller_', internalType: 'address', type: 'address' },
      { name: 'tronUsdt_', internalType: 'address', type: 'address' },
    ],
    stateMutability: 'nonpayable',
  },
  {
    type: 'function',
    inputs: [],
    name: 'CONTROLLER_ADDRESS',
    outputs: [{ name: '', internalType: 'address', type: 'address' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [{ name: 'controller_', internalType: 'address', type: 'address' }],
    name: 'setController',
    outputs: [],
    stateMutability: 'nonpayable',
  },
  {
    type: 'function',
    inputs: [
      {
        name: 'reader_',
        internalType: 'contract ITronTxReader',
        type: 'address',
      },
    ],
    name: 'setTronReader',
    outputs: [],
    stateMutability: 'nonpayable',
  },
  {
    type: 'function',
    inputs: [{ name: 'tronUsdt_', internalType: 'address', type: 'address' }],
    name: 'setTronUsdt',
    outputs: [],
    stateMutability: 'nonpayable',
  },
  {
    type: 'function',
    inputs: [],
    name: 'tronReader',
    outputs: [
      { name: '', internalType: 'contract ITronTxReader', type: 'address' },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'tronUsdt',
    outputs: [{ name: '', internalType: 'address', type: 'address' }],
    stateMutability: 'view',
  },
] as const

//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
// OAppCore
//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

export const oAppCoreAbi = [
  {
    type: 'function',
    inputs: [],
    name: 'endpoint',
    outputs: [
      {
        name: '',
        internalType: 'contract ILayerZeroEndpointV2',
        type: 'address',
      },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'oAppVersion',
    outputs: [
      { name: 'senderVersion', internalType: 'uint64', type: 'uint64' },
      { name: 'receiverVersion', internalType: 'uint64', type: 'uint64' },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'owner',
    outputs: [{ name: '', internalType: 'address', type: 'address' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [{ name: 'eid', internalType: 'uint32', type: 'uint32' }],
    name: 'peers',
    outputs: [{ name: 'peer', internalType: 'bytes32', type: 'bytes32' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'renounceOwnership',
    outputs: [],
    stateMutability: 'nonpayable',
  },
  {
    type: 'function',
    inputs: [{ name: '_delegate', internalType: 'address', type: 'address' }],
    name: 'setDelegate',
    outputs: [],
    stateMutability: 'nonpayable',
  },
  {
    type: 'function',
    inputs: [
      { name: '_eid', internalType: 'uint32', type: 'uint32' },
      { name: '_peer', internalType: 'bytes32', type: 'bytes32' },
    ],
    name: 'setPeer',
    outputs: [],
    stateMutability: 'nonpayable',
  },
  {
    type: 'function',
    inputs: [{ name: 'newOwner', internalType: 'address', type: 'address' }],
    name: 'transferOwnership',
    outputs: [],
    stateMutability: 'nonpayable',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'previousOwner',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      {
        name: 'newOwner',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
    ],
    name: 'OwnershipTransferred',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'eid', internalType: 'uint32', type: 'uint32', indexed: false },
      {
        name: 'peer',
        internalType: 'bytes32',
        type: 'bytes32',
        indexed: false,
      },
    ],
    name: 'PeerSet',
  },
  { type: 'error', inputs: [], name: 'InvalidDelegate' },
  { type: 'error', inputs: [], name: 'InvalidEndpointCall' },
  {
    type: 'error',
    inputs: [{ name: 'eid', internalType: 'uint32', type: 'uint32' }],
    name: 'NoPeer',
  },
  {
    type: 'error',
    inputs: [
      { name: 'eid', internalType: 'uint32', type: 'uint32' },
      { name: 'sender', internalType: 'bytes32', type: 'bytes32' },
    ],
    name: 'OnlyPeer',
  },
  {
    type: 'error',
    inputs: [{ name: 'owner', internalType: 'address', type: 'address' }],
    name: 'OwnableInvalidOwner',
  },
  {
    type: 'error',
    inputs: [{ name: 'account', internalType: 'address', type: 'address' }],
    name: 'OwnableUnauthorizedAccount',
  },
] as const

//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
// OAppSender
//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

export const oAppSenderAbi = [
  {
    type: 'function',
    inputs: [],
    name: 'endpoint',
    outputs: [
      {
        name: '',
        internalType: 'contract ILayerZeroEndpointV2',
        type: 'address',
      },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'oAppVersion',
    outputs: [
      { name: 'senderVersion', internalType: 'uint64', type: 'uint64' },
      { name: 'receiverVersion', internalType: 'uint64', type: 'uint64' },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'owner',
    outputs: [{ name: '', internalType: 'address', type: 'address' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [{ name: 'eid', internalType: 'uint32', type: 'uint32' }],
    name: 'peers',
    outputs: [{ name: 'peer', internalType: 'bytes32', type: 'bytes32' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'renounceOwnership',
    outputs: [],
    stateMutability: 'nonpayable',
  },
  {
    type: 'function',
    inputs: [{ name: '_delegate', internalType: 'address', type: 'address' }],
    name: 'setDelegate',
    outputs: [],
    stateMutability: 'nonpayable',
  },
  {
    type: 'function',
    inputs: [
      { name: '_eid', internalType: 'uint32', type: 'uint32' },
      { name: '_peer', internalType: 'bytes32', type: 'bytes32' },
    ],
    name: 'setPeer',
    outputs: [],
    stateMutability: 'nonpayable',
  },
  {
    type: 'function',
    inputs: [{ name: 'newOwner', internalType: 'address', type: 'address' }],
    name: 'transferOwnership',
    outputs: [],
    stateMutability: 'nonpayable',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'previousOwner',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      {
        name: 'newOwner',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
    ],
    name: 'OwnershipTransferred',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'eid', internalType: 'uint32', type: 'uint32', indexed: false },
      {
        name: 'peer',
        internalType: 'bytes32',
        type: 'bytes32',
        indexed: false,
      },
    ],
    name: 'PeerSet',
  },
  { type: 'error', inputs: [], name: 'InvalidDelegate' },
  { type: 'error', inputs: [], name: 'InvalidEndpointCall' },
  { type: 'error', inputs: [], name: 'LzTokenUnavailable' },
  {
    type: 'error',
    inputs: [{ name: 'eid', internalType: 'uint32', type: 'uint32' }],
    name: 'NoPeer',
  },
  {
    type: 'error',
    inputs: [{ name: 'msgValue', internalType: 'uint256', type: 'uint256' }],
    name: 'NotEnoughNative',
  },
  {
    type: 'error',
    inputs: [
      { name: 'eid', internalType: 'uint32', type: 'uint32' },
      { name: 'sender', internalType: 'bytes32', type: 'bytes32' },
    ],
    name: 'OnlyPeer',
  },
  {
    type: 'error',
    inputs: [{ name: 'owner', internalType: 'address', type: 'address' }],
    name: 'OwnableInvalidOwner',
  },
  {
    type: 'error',
    inputs: [{ name: 'account', internalType: 'address', type: 'address' }],
    name: 'OwnableUnauthorizedAccount',
  },
] as const

//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
// Ownable
//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

export const ownableAbi = [
  {
    type: 'function',
    inputs: [],
    name: 'owner',
    outputs: [{ name: '', internalType: 'address', type: 'address' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'renounceOwnership',
    outputs: [],
    stateMutability: 'nonpayable',
  },
  {
    type: 'function',
    inputs: [{ name: 'newOwner', internalType: 'address', type: 'address' }],
    name: 'transferOwnership',
    outputs: [],
    stateMutability: 'nonpayable',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'previousOwner',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      {
        name: 'newOwner',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
    ],
    name: 'OwnershipTransferred',
  },
  {
    type: 'error',
    inputs: [{ name: 'owner', internalType: 'address', type: 'address' }],
    name: 'OwnableInvalidOwner',
  },
  {
    type: 'error',
    inputs: [{ name: 'account', internalType: 'address', type: 'address' }],
    name: 'OwnableUnauthorizedAccount',
  },
] as const

//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
// ReentrancyGuard
//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

export const reentrancyGuardAbi = [
  { type: 'error', inputs: [], name: 'Reentrancy' },
] as const

//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
// RevertingBridger
//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

export const revertingBridgerAbi = [
  {
    type: 'function',
    inputs: [
      { name: '', internalType: 'address', type: 'address' },
      { name: '', internalType: 'uint256', type: 'uint256' },
      { name: '', internalType: 'address', type: 'address' },
      { name: '', internalType: 'uint256', type: 'uint256' },
      { name: '', internalType: 'bytes', type: 'bytes' },
    ],
    name: 'bridge',
    outputs: [{ name: '', internalType: 'uint256', type: 'uint256' }],
    stateMutability: 'payable',
  },
  { type: 'error', inputs: [], name: 'RevertBridge' },
] as const

//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
// SafeCast
//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

export const safeCastAbi = [
  {
    type: 'error',
    inputs: [
      { name: 'bits', internalType: 'uint8', type: 'uint8' },
      { name: 'value', internalType: 'int256', type: 'int256' },
    ],
    name: 'SafeCastOverflowedIntDowncast',
  },
  {
    type: 'error',
    inputs: [{ name: 'value', internalType: 'int256', type: 'int256' }],
    name: 'SafeCastOverflowedIntToUint',
  },
  {
    type: 'error',
    inputs: [
      { name: 'bits', internalType: 'uint8', type: 'uint8' },
      { name: 'value', internalType: 'uint256', type: 'uint256' },
    ],
    name: 'SafeCastOverflowedUintDowncast',
  },
  {
    type: 'error',
    inputs: [{ name: 'value', internalType: 'uint256', type: 'uint256' }],
    name: 'SafeCastOverflowedUintToInt',
  },
] as const

//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
// SafeERC20
//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

export const safeErc20Abi = [
  {
    type: 'error',
    inputs: [
      { name: 'spender', internalType: 'address', type: 'address' },
      { name: 'currentAllowance', internalType: 'uint256', type: 'uint256' },
      { name: 'requestedDecrease', internalType: 'uint256', type: 'uint256' },
    ],
    name: 'SafeERC20FailedDecreaseAllowance',
  },
  {
    type: 'error',
    inputs: [{ name: 'token', internalType: 'address', type: 'address' }],
    name: 'SafeERC20FailedOperation',
  },
] as const

//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
// SafeTransferLib
//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

export const safeTransferLibAbi = [
  { type: 'error', inputs: [], name: 'ApproveFailed' },
  { type: 'error', inputs: [], name: 'ETHTransferFailed' },
  { type: 'error', inputs: [], name: 'Permit2AmountOverflow' },
  { type: 'error', inputs: [], name: 'Permit2ApproveFailed' },
  { type: 'error', inputs: [], name: 'Permit2Failed' },
  { type: 'error', inputs: [], name: 'Permit2LockdownFailed' },
  { type: 'error', inputs: [], name: 'TotalSupplyQueryFailed' },
  { type: 'error', inputs: [], name: 'TransferFailed' },
  { type: 'error', inputs: [], name: 'TransferFromFailed' },
] as const

//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
// StablecoinQuoter
//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

export const stablecoinQuoterAbi = [
  {
    type: 'function',
    inputs: [
      { name: '', internalType: 'address', type: 'address' },
      { name: '', internalType: 'address', type: 'address' },
      { name: 'amountIn', internalType: 'uint256', type: 'uint256' },
      { name: '', internalType: 'uint256', type: 'uint256' },
    ],
    name: 'quote',
    outputs: [{ name: 'amountOut', internalType: 'uint256', type: 'uint256' }],
    stateMutability: 'pure',
  },
] as const

//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
// StatefulTronTxReader
//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

export const statefulTronTxReaderAbi = [
  {
    type: 'constructor',
    inputs: [
      { name: '_srs', internalType: 'bytes20[27]', type: 'bytes20[27]' },
      {
        name: '_witnessDelegatees',
        internalType: 'bytes20[27]',
        type: 'bytes20[27]',
      },
    ],
    stateMutability: 'nonpayable',
  },
  {
    type: 'function',
    inputs: [
      { name: 'blocks', internalType: 'bytes[20]', type: 'bytes[20]' },
      { name: 'encodedTx', internalType: 'bytes', type: 'bytes' },
      { name: 'proof', internalType: 'bytes32[]', type: 'bytes32[]' },
      { name: 'index', internalType: 'uint256', type: 'uint256' },
    ],
    name: 'readDelegateResourceContract',
    outputs: [
      {
        name: 'delegation',
        internalType: 'struct DelegateResourceContract',
        type: 'tuple',
        components: [
          { name: 'txId', internalType: 'bytes32', type: 'bytes32' },
          { name: 'tronBlockNumber', internalType: 'uint256', type: 'uint256' },
          { name: 'balanceSun', internalType: 'uint256', type: 'uint256' },
          { name: 'lockPeriod', internalType: 'uint256', type: 'uint256' },
          { name: 'ownerTron', internalType: 'bytes21', type: 'bytes21' },
          { name: 'receiverTron', internalType: 'bytes21', type: 'bytes21' },
          {
            name: 'tronBlockTimestamp',
            internalType: 'uint32',
            type: 'uint32',
          },
          { name: 'resource', internalType: 'uint8', type: 'uint8' },
          { name: 'lock', internalType: 'bool', type: 'bool' },
        ],
      },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [
      { name: 'blocks', internalType: 'bytes[20]', type: 'bytes[20]' },
      { name: 'encodedTx', internalType: 'bytes', type: 'bytes' },
      { name: 'proof', internalType: 'bytes32[]', type: 'bytes32[]' },
      { name: 'index', internalType: 'uint256', type: 'uint256' },
    ],
    name: 'readTransferContract',
    outputs: [
      {
        name: 'transfer',
        internalType: 'struct TransferContract',
        type: 'tuple',
        components: [
          { name: 'txId', internalType: 'bytes32', type: 'bytes32' },
          { name: 'tronBlockNumber', internalType: 'uint256', type: 'uint256' },
          {
            name: 'tronBlockTimestamp',
            internalType: 'uint32',
            type: 'uint32',
          },
          { name: 'senderTron', internalType: 'bytes21', type: 'bytes21' },
          { name: 'toTron', internalType: 'bytes21', type: 'bytes21' },
          { name: 'amountSun', internalType: 'uint256', type: 'uint256' },
        ],
      },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [
      { name: 'blocks', internalType: 'bytes[20]', type: 'bytes[20]' },
      { name: 'encodedTx', internalType: 'bytes', type: 'bytes' },
      { name: 'proof', internalType: 'bytes32[]', type: 'bytes32[]' },
      { name: 'index', internalType: 'uint256', type: 'uint256' },
    ],
    name: 'readTriggerSmartContract',
    outputs: [
      {
        name: 'callData',
        internalType: 'struct TriggerSmartContract',
        type: 'tuple',
        components: [
          { name: 'txId', internalType: 'bytes32', type: 'bytes32' },
          { name: 'tronBlockNumber', internalType: 'uint256', type: 'uint256' },
          {
            name: 'tronBlockTimestamp',
            internalType: 'uint32',
            type: 'uint32',
          },
          { name: 'senderTron', internalType: 'bytes21', type: 'bytes21' },
          { name: 'toTron', internalType: 'bytes21', type: 'bytes21' },
          { name: 'callValueSun', internalType: 'uint256', type: 'uint256' },
          { name: 'data', internalType: 'bytes', type: 'bytes' },
        ],
      },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [{ name: '', internalType: 'bytes20', type: 'bytes20' }],
    name: 'srIndexPlus1',
    outputs: [{ name: '', internalType: 'uint8', type: 'uint8' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [{ name: '', internalType: 'uint256', type: 'uint256' }],
    name: 'srs',
    outputs: [{ name: '', internalType: 'bytes20', type: 'bytes20' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [{ name: '', internalType: 'uint256', type: 'uint256' }],
    name: 'witnessDelegatees',
    outputs: [{ name: '', internalType: 'bytes20', type: 'bytes20' }],
    stateMutability: 'view',
  },
  {
    type: 'error',
    inputs: [{ name: 'sr', internalType: 'bytes20', type: 'bytes20' }],
    name: 'DuplicateSr',
  },
  { type: 'error', inputs: [], name: 'InvalidBlockSequence' },
  {
    type: 'error',
    inputs: [{ name: 'got', internalType: 'uint256', type: 'uint256' }],
    name: 'InvalidEncodedBlockLength',
  },
  { type: 'error', inputs: [], name: 'InvalidHeaderPrefix' },
  { type: 'error', inputs: [], name: 'InvalidTxMerkleProof' },
  {
    type: 'error',
    inputs: [{ name: 'got', internalType: 'uint8', type: 'uint8' }],
    name: 'InvalidWitnessAddressPrefix',
  },
  { type: 'error', inputs: [], name: 'InvalidWitnessSignature' },
  { type: 'error', inputs: [], name: 'NotDelegateResourceContract' },
  { type: 'error', inputs: [], name: 'NotTransferContract' },
  { type: 'error', inputs: [], name: 'NotTriggerSmartContract' },
  { type: 'error', inputs: [], name: 'ProtoInvalidWireType' },
  { type: 'error', inputs: [], name: 'ProtoTruncated' },
  {
    type: 'error',
    inputs: [
      { name: 'index', internalType: 'uint256', type: 'uint256' },
      { name: 'prev', internalType: 'bytes20', type: 'bytes20' },
      { name: 'next', internalType: 'bytes20', type: 'bytes20' },
    ],
    name: 'SrSetNotSorted',
  },
  { type: 'error', inputs: [], name: 'TimestampOverflow' },
  { type: 'error', inputs: [], name: 'TronInvalidBalance' },
  { type: 'error', inputs: [], name: 'TronInvalidCallValue' },
  { type: 'error', inputs: [], name: 'TronInvalidContractLength' },
  { type: 'error', inputs: [], name: 'TronInvalidContractPrefix' },
  { type: 'error', inputs: [], name: 'TronInvalidLock' },
  { type: 'error', inputs: [], name: 'TronInvalidLockPeriod' },
  { type: 'error', inputs: [], name: 'TronInvalidOwnerLength' },
  { type: 'error', inputs: [], name: 'TronInvalidOwnerPrefix' },
  { type: 'error', inputs: [], name: 'TronInvalidReceiverLength' },
  { type: 'error', inputs: [], name: 'TronInvalidReceiverPrefix' },
  { type: 'error', inputs: [], name: 'TronInvalidResource' },
  { type: 'error', inputs: [], name: 'TronTxNotSuccessful' },
  {
    type: 'error',
    inputs: [{ name: 'sr', internalType: 'bytes20', type: 'bytes20' }],
    name: 'UnknownSr',
  },
] as const

//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
// SwapExecutor
//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

export const swapExecutorAbi = [
  { type: 'constructor', inputs: [], stateMutability: 'nonpayable' },
  { type: 'receive', stateMutability: 'payable' },
  {
    type: 'function',
    inputs: [],
    name: 'OWNER',
    outputs: [{ name: '', internalType: 'address', type: 'address' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [
      {
        name: 'calls',
        internalType: 'struct Call[]',
        type: 'tuple[]',
        components: [
          { name: 'to', internalType: 'address', type: 'address' },
          { name: 'value', internalType: 'uint256', type: 'uint256' },
          { name: 'data', internalType: 'bytes', type: 'bytes' },
        ],
      },
      { name: 'token', internalType: 'address', type: 'address' },
      { name: 'expectedAmount', internalType: 'uint256', type: 'uint256' },
      { name: 'recipient', internalType: 'address payable', type: 'address' },
    ],
    name: 'execute',
    outputs: [{ name: 'actualOut', internalType: 'uint256', type: 'uint256' }],
    stateMutability: 'nonpayable',
  },
  {
    type: 'error',
    inputs: [{ name: 'callIndex', internalType: 'uint256', type: 'uint256' }],
    name: 'CallFailed',
  },
  { type: 'error', inputs: [], name: 'InsufficientOutput' },
  { type: 'error', inputs: [], name: 'NotOwner' },
  { type: 'error', inputs: [], name: 'Reentrancy' },
] as const

//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
// TestTronTxReaderNoSig
//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

export const testTronTxReaderNoSigAbi = [
  {
    type: 'function',
    inputs: [
      { name: 'blocks', internalType: 'bytes[20]', type: 'bytes[20]' },
      { name: 'encodedTx', internalType: 'bytes', type: 'bytes' },
      { name: 'proof', internalType: 'bytes32[]', type: 'bytes32[]' },
      { name: 'index', internalType: 'uint256', type: 'uint256' },
    ],
    name: 'readDelegateResourceContract',
    outputs: [
      {
        name: 'delegation',
        internalType: 'struct DelegateResourceContract',
        type: 'tuple',
        components: [
          { name: 'txId', internalType: 'bytes32', type: 'bytes32' },
          { name: 'tronBlockNumber', internalType: 'uint256', type: 'uint256' },
          { name: 'balanceSun', internalType: 'uint256', type: 'uint256' },
          { name: 'lockPeriod', internalType: 'uint256', type: 'uint256' },
          { name: 'ownerTron', internalType: 'bytes21', type: 'bytes21' },
          { name: 'receiverTron', internalType: 'bytes21', type: 'bytes21' },
          {
            name: 'tronBlockTimestamp',
            internalType: 'uint32',
            type: 'uint32',
          },
          { name: 'resource', internalType: 'uint8', type: 'uint8' },
          { name: 'lock', internalType: 'bool', type: 'bool' },
        ],
      },
    ],
    stateMutability: 'pure',
  },
  {
    type: 'function',
    inputs: [
      { name: 'blocks', internalType: 'bytes[20]', type: 'bytes[20]' },
      { name: 'encodedTx', internalType: 'bytes', type: 'bytes' },
      { name: 'proof', internalType: 'bytes32[]', type: 'bytes32[]' },
      { name: 'index', internalType: 'uint256', type: 'uint256' },
    ],
    name: 'readTransferContract',
    outputs: [
      {
        name: 'transfer',
        internalType: 'struct TransferContract',
        type: 'tuple',
        components: [
          { name: 'txId', internalType: 'bytes32', type: 'bytes32' },
          { name: 'tronBlockNumber', internalType: 'uint256', type: 'uint256' },
          {
            name: 'tronBlockTimestamp',
            internalType: 'uint32',
            type: 'uint32',
          },
          { name: 'senderTron', internalType: 'bytes21', type: 'bytes21' },
          { name: 'toTron', internalType: 'bytes21', type: 'bytes21' },
          { name: 'amountSun', internalType: 'uint256', type: 'uint256' },
        ],
      },
    ],
    stateMutability: 'pure',
  },
  {
    type: 'function',
    inputs: [
      { name: 'blocks', internalType: 'bytes[20]', type: 'bytes[20]' },
      { name: 'encodedTx', internalType: 'bytes', type: 'bytes' },
      { name: 'proof', internalType: 'bytes32[]', type: 'bytes32[]' },
      { name: 'index', internalType: 'uint256', type: 'uint256' },
    ],
    name: 'readTriggerSmartContract',
    outputs: [
      {
        name: 'callData',
        internalType: 'struct TriggerSmartContract',
        type: 'tuple',
        components: [
          { name: 'txId', internalType: 'bytes32', type: 'bytes32' },
          { name: 'tronBlockNumber', internalType: 'uint256', type: 'uint256' },
          {
            name: 'tronBlockTimestamp',
            internalType: 'uint32',
            type: 'uint32',
          },
          { name: 'senderTron', internalType: 'bytes21', type: 'bytes21' },
          { name: 'toTron', internalType: 'bytes21', type: 'bytes21' },
          { name: 'callValueSun', internalType: 'uint256', type: 'uint256' },
          { name: 'data', internalType: 'bytes', type: 'bytes' },
        ],
      },
    ],
    stateMutability: 'pure',
  },
  {
    type: 'error',
    inputs: [{ name: 'got', internalType: 'uint256', type: 'uint256' }],
    name: 'InvalidEncodedBlockLength',
  },
  { type: 'error', inputs: [], name: 'InvalidHeaderPrefix' },
  { type: 'error', inputs: [], name: 'InvalidTxMerkleProof' },
  { type: 'error', inputs: [], name: 'NotDelegateResourceContract' },
  { type: 'error', inputs: [], name: 'NotTransferContract' },
  { type: 'error', inputs: [], name: 'NotTriggerSmartContract' },
  { type: 'error', inputs: [], name: 'ProtoInvalidWireType' },
  { type: 'error', inputs: [], name: 'ProtoTruncated' },
  { type: 'error', inputs: [], name: 'TimestampOverflow' },
  { type: 'error', inputs: [], name: 'TronInvalidBalance' },
  { type: 'error', inputs: [], name: 'TronInvalidCallValue' },
  { type: 'error', inputs: [], name: 'TronInvalidContractLength' },
  { type: 'error', inputs: [], name: 'TronInvalidContractPrefix' },
  { type: 'error', inputs: [], name: 'TronInvalidLock' },
  { type: 'error', inputs: [], name: 'TronInvalidLockPeriod' },
  { type: 'error', inputs: [], name: 'TronInvalidOwnerLength' },
  { type: 'error', inputs: [], name: 'TronInvalidOwnerPrefix' },
  { type: 'error', inputs: [], name: 'TronInvalidReceiverLength' },
  { type: 'error', inputs: [], name: 'TronInvalidReceiverPrefix' },
  { type: 'error', inputs: [], name: 'TronInvalidResource' },
  { type: 'error', inputs: [], name: 'TronTxNotSuccessful' },
] as const

//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
// TokenUtils
//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

export const tokenUtilsAbi = [
  { type: 'error', inputs: [], name: 'InsufficientETH' },
] as const

//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
// TronTxReaderErrors
//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

export const tronTxReaderErrorsAbi = [
  {
    type: 'error',
    inputs: [{ name: 'sr', internalType: 'bytes20', type: 'bytes20' }],
    name: 'DuplicateSr',
  },
  { type: 'error', inputs: [], name: 'InvalidBlockSequence' },
  {
    type: 'error',
    inputs: [{ name: 'got', internalType: 'uint256', type: 'uint256' }],
    name: 'InvalidEncodedBlockLength',
  },
  { type: 'error', inputs: [], name: 'InvalidHeaderPrefix' },
  { type: 'error', inputs: [], name: 'InvalidTxMerkleProof' },
  {
    type: 'error',
    inputs: [{ name: 'got', internalType: 'uint8', type: 'uint8' }],
    name: 'InvalidWitnessAddressPrefix',
  },
  { type: 'error', inputs: [], name: 'InvalidWitnessSignature' },
  { type: 'error', inputs: [], name: 'NotDelegateResourceContract' },
  { type: 'error', inputs: [], name: 'NotTransferContract' },
  { type: 'error', inputs: [], name: 'NotTriggerSmartContract' },
  {
    type: 'error',
    inputs: [
      { name: 'index', internalType: 'uint256', type: 'uint256' },
      { name: 'prev', internalType: 'bytes20', type: 'bytes20' },
      { name: 'next', internalType: 'bytes20', type: 'bytes20' },
    ],
    name: 'SrSetNotSorted',
  },
  { type: 'error', inputs: [], name: 'TimestampOverflow' },
  { type: 'error', inputs: [], name: 'TronInvalidBalance' },
  { type: 'error', inputs: [], name: 'TronInvalidCallValue' },
  { type: 'error', inputs: [], name: 'TronInvalidContractLength' },
  { type: 'error', inputs: [], name: 'TronInvalidContractPrefix' },
  { type: 'error', inputs: [], name: 'TronInvalidLock' },
  { type: 'error', inputs: [], name: 'TronInvalidLockPeriod' },
  { type: 'error', inputs: [], name: 'TronInvalidOwnerLength' },
  { type: 'error', inputs: [], name: 'TronInvalidOwnerPrefix' },
  { type: 'error', inputs: [], name: 'TronInvalidReceiverLength' },
  { type: 'error', inputs: [], name: 'TronInvalidReceiverPrefix' },
  { type: 'error', inputs: [], name: 'TronInvalidResource' },
  { type: 'error', inputs: [], name: 'TronTxNotSuccessful' },
  {
    type: 'error',
    inputs: [{ name: 'sr', internalType: 'bytes20', type: 'bytes20' }],
    name: 'UnknownSr',
  },
] as const

//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
// USDT0Bridger
//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

export const usdt0BridgerAbi = [
  {
    type: 'constructor',
    inputs: [
      { name: 'owner', internalType: 'address', type: 'address' },
      { name: 'authorizedCaller', internalType: 'address', type: 'address' },
      { name: 'usdt0', internalType: 'address', type: 'address' },
      { name: 'oft', internalType: 'address', type: 'address' },
      {
        name: 'supportedChainIds',
        internalType: 'uint256[]',
        type: 'uint256[]',
      },
      { name: 'eids', internalType: 'uint32[]', type: 'uint32[]' },
    ],
    stateMutability: 'nonpayable',
  },
  { type: 'receive', stateMutability: 'payable' },
  {
    type: 'function',
    inputs: [],
    name: 'AUTHORIZED_CALLER',
    outputs: [{ name: '', internalType: 'address', type: 'address' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'OFT',
    outputs: [{ name: '', internalType: 'contract IOFT', type: 'address' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'USDT0',
    outputs: [{ name: '', internalType: 'contract IERC20', type: 'address' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [
      { name: 'inputToken', internalType: 'address', type: 'address' },
      { name: 'inputAmount', internalType: 'uint256', type: 'uint256' },
      { name: 'outputAddress', internalType: 'address', type: 'address' },
      { name: 'outputChainId', internalType: 'uint256', type: 'uint256' },
      { name: '', internalType: 'bytes', type: 'bytes' },
    ],
    name: 'bridge',
    outputs: [
      { name: 'expectedAmountOut', internalType: 'uint256', type: 'uint256' },
    ],
    stateMutability: 'payable',
  },
  {
    type: 'function',
    inputs: [],
    name: 'cancelOwnershipHandover',
    outputs: [],
    stateMutability: 'payable',
  },
  {
    type: 'function',
    inputs: [
      { name: 'pendingOwner', internalType: 'address', type: 'address' },
    ],
    name: 'completeOwnershipHandover',
    outputs: [],
    stateMutability: 'payable',
  },
  {
    type: 'function',
    inputs: [{ name: '', internalType: 'uint256', type: 'uint256' }],
    name: 'eidByChainId',
    outputs: [{ name: '', internalType: 'uint32', type: 'uint32' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'owner',
    outputs: [{ name: 'result', internalType: 'address', type: 'address' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [
      { name: 'pendingOwner', internalType: 'address', type: 'address' },
    ],
    name: 'ownershipHandoverExpiresAt',
    outputs: [{ name: 'result', internalType: 'uint256', type: 'uint256' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'renounceOwnership',
    outputs: [],
    stateMutability: 'payable',
  },
  {
    type: 'function',
    inputs: [],
    name: 'requestOwnershipHandover',
    outputs: [],
    stateMutability: 'payable',
  },
  {
    type: 'function',
    inputs: [{ name: 'newOwner', internalType: 'address', type: 'address' }],
    name: 'transferOwnership',
    outputs: [],
    stateMutability: 'payable',
  },
  {
    type: 'function',
    inputs: [
      { name: 'token', internalType: 'address', type: 'address' },
      { name: 'amount', internalType: 'uint256', type: 'uint256' },
    ],
    name: 'withdraw',
    outputs: [],
    stateMutability: 'nonpayable',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'pendingOwner',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
    ],
    name: 'OwnershipHandoverCanceled',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'pendingOwner',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
    ],
    name: 'OwnershipHandoverRequested',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'oldOwner',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      {
        name: 'newOwner',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
    ],
    name: 'OwnershipTransferred',
  },
  { type: 'error', inputs: [], name: 'AlreadyInitialized' },
  { type: 'error', inputs: [], name: 'AmountZero' },
  {
    type: 'error',
    inputs: [
      { name: 'a', internalType: 'uint256', type: 'uint256' },
      { name: 'b', internalType: 'uint256', type: 'uint256' },
    ],
    name: 'ArrayLengthMismatch',
  },
  {
    type: 'error',
    inputs: [{ name: 'chainId', internalType: 'uint256', type: 'uint256' }],
    name: 'DuplicateChainId',
  },
  {
    type: 'error',
    inputs: [
      { name: 'have', internalType: 'uint256', type: 'uint256' },
      { name: 'need', internalType: 'uint256', type: 'uint256' },
    ],
    name: 'InsufficientNativeValue',
  },
  { type: 'error', inputs: [], name: 'NewOwnerIsZeroAddress' },
  { type: 'error', inputs: [], name: 'NoHandoverRequest' },
  { type: 'error', inputs: [], name: 'NotAuthorizedCaller' },
  { type: 'error', inputs: [], name: 'Unauthorized' },
  {
    type: 'error',
    inputs: [{ name: 'chainId', internalType: 'uint256', type: 'uint256' }],
    name: 'UnsupportedChainId',
  },
  {
    type: 'error',
    inputs: [{ name: 'token', internalType: 'address', type: 'address' }],
    name: 'UnsupportedToken',
  },
  { type: 'error', inputs: [], name: 'ZeroAddress' },
  { type: 'error', inputs: [], name: 'ZeroOutputAddress' },
] as const

//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
// UntronIntents
//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

export const untronIntentsAbi = [
  {
    type: 'constructor',
    inputs: [
      { name: '_owner', internalType: 'address', type: 'address' },
      { name: 'v3', internalType: 'contract IUntronV3', type: 'address' },
      { name: 'usdt', internalType: 'address', type: 'address' },
    ],
    stateMutability: 'nonpayable',
  },
  {
    type: 'function',
    inputs: [],
    name: 'INTENT_CLAIM_DEPOSIT',
    outputs: [{ name: '', internalType: 'uint256', type: 'uint256' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'RECEIVER_INTENT_DURATION',
    outputs: [{ name: '', internalType: 'uint256', type: 'uint256' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'TIME_TO_FILL',
    outputs: [{ name: '', internalType: 'uint256', type: 'uint256' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'USDT',
    outputs: [{ name: '', internalType: 'address', type: 'address' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'V3',
    outputs: [
      { name: '', internalType: 'contract IUntronV3', type: 'address' },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [{ name: 'id', internalType: 'bytes32', type: 'bytes32' }],
    name: 'claimIntent',
    outputs: [],
    stateMutability: 'nonpayable',
  },
  {
    type: 'function',
    inputs: [
      {
        name: 'forwarder',
        internalType: 'contract IntentsForwarder',
        type: 'address',
      },
      { name: 'toTron', internalType: 'address', type: 'address' },
      { name: 'forwardSalt', internalType: 'bytes32', type: 'bytes32' },
      { name: 'token', internalType: 'address', type: 'address' },
      { name: 'amount', internalType: 'uint256', type: 'uint256' },
    ],
    name: 'claimVirtualReceiverIntent',
    outputs: [],
    stateMutability: 'nonpayable',
  },
  {
    type: 'function',
    inputs: [{ name: 'id', internalType: 'bytes32', type: 'bytes32' }],
    name: 'closeIntent',
    outputs: [],
    stateMutability: 'nonpayable',
  },
  {
    type: 'function',
    inputs: [
      {
        name: 'intent',
        internalType: 'struct UntronIntents.Intent',
        type: 'tuple',
        components: [
          {
            name: 'intentType',
            internalType: 'enum UntronIntents.IntentType',
            type: 'uint8',
          },
          { name: 'intentSpecs', internalType: 'bytes', type: 'bytes' },
          {
            name: 'refundBeneficiary',
            internalType: 'address',
            type: 'address',
          },
          { name: 'token', internalType: 'address', type: 'address' },
          { name: 'amount', internalType: 'uint256', type: 'uint256' },
        ],
      },
      { name: 'deadline', internalType: 'uint256', type: 'uint256' },
    ],
    name: 'createIntent',
    outputs: [],
    stateMutability: 'payable',
  },
  {
    type: 'function',
    inputs: [
      {
        name: 'forwarder',
        internalType: 'contract IntentsForwarder',
        type: 'address',
      },
      { name: 'toTron', internalType: 'address', type: 'address' },
      { name: 'forwardSalt', internalType: 'bytes32', type: 'bytes32' },
      { name: 'token', internalType: 'address', type: 'address' },
      { name: 'amount', internalType: 'uint256', type: 'uint256' },
    ],
    name: 'createIntentFromReceiver',
    outputs: [],
    stateMutability: 'payable',
  },
  {
    type: 'function',
    inputs: [],
    name: 'eventChainTip',
    outputs: [{ name: '', internalType: 'bytes32', type: 'bytes32' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'eventSeq',
    outputs: [{ name: '', internalType: 'uint256', type: 'uint256' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [
      {
        name: 'forwarder',
        internalType: 'contract IntentsForwarder',
        type: 'address',
      },
      { name: 'toTron', internalType: 'address', type: 'address' },
      { name: 'forwardSalt', internalType: 'bytes32', type: 'bytes32' },
      { name: 'token', internalType: 'address', type: 'address' },
      { name: 'amount', internalType: 'uint256', type: 'uint256' },
    ],
    name: 'fundReceiverIntent',
    outputs: [],
    stateMutability: 'payable',
  },
  {
    type: 'function',
    inputs: [{ name: '', internalType: 'bytes32', type: 'bytes32' }],
    name: 'intents',
    outputs: [
      {
        name: 'intent',
        internalType: 'struct UntronIntents.Intent',
        type: 'tuple',
        components: [
          {
            name: 'intentType',
            internalType: 'enum UntronIntents.IntentType',
            type: 'uint8',
          },
          { name: 'intentSpecs', internalType: 'bytes', type: 'bytes' },
          {
            name: 'refundBeneficiary',
            internalType: 'address',
            type: 'address',
          },
          { name: 'token', internalType: 'address', type: 'address' },
          { name: 'amount', internalType: 'uint256', type: 'uint256' },
        ],
      },
      { name: 'solverClaimedAt', internalType: 'uint256', type: 'uint256' },
      { name: 'deadline', internalType: 'uint256', type: 'uint256' },
      { name: 'solver', internalType: 'address', type: 'address' },
      { name: 'solved', internalType: 'bool', type: 'bool' },
      { name: 'funded', internalType: 'bool', type: 'bool' },
      { name: 'settled', internalType: 'bool', type: 'bool' },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'owner',
    outputs: [{ name: 'result', internalType: 'address', type: 'address' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [
      { name: 'id', internalType: 'bytes32', type: 'bytes32' },
      { name: 'blocks', internalType: 'bytes[20]', type: 'bytes[20]' },
      { name: 'encodedTx', internalType: 'bytes', type: 'bytes' },
      { name: 'proof', internalType: 'bytes32[]', type: 'bytes32[]' },
      { name: 'index', internalType: 'uint256', type: 'uint256' },
    ],
    name: 'proveIntentFill',
    outputs: [],
    stateMutability: 'nonpayable',
  },
  {
    type: 'function',
    inputs: [
      {
        name: 'forwarder',
        internalType: 'contract IntentsForwarder',
        type: 'address',
      },
      { name: 'toTron', internalType: 'address', type: 'address' },
      { name: 'forwardSalt', internalType: 'bytes32', type: 'bytes32' },
      { name: 'token', internalType: 'address', type: 'address' },
      { name: 'amount', internalType: 'uint256', type: 'uint256' },
    ],
    name: 'receiverIntentId',
    outputs: [{ name: '', internalType: 'bytes32', type: 'bytes32' }],
    stateMutability: 'pure',
  },
  {
    type: 'function',
    inputs: [{ name: 'amount', internalType: 'uint256', type: 'uint256' }],
    name: 'recommendedIntentFee',
    outputs: [{ name: '', internalType: 'uint256', type: 'uint256' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'recommendedIntentFeeFlat',
    outputs: [{ name: '', internalType: 'uint256', type: 'uint256' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'recommendedIntentFeePpm',
    outputs: [{ name: '', internalType: 'uint256', type: 'uint256' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'renounceOwnership',
    outputs: [],
    stateMutability: 'payable',
  },
  {
    type: 'function',
    inputs: [
      { name: 'ppm', internalType: 'uint256', type: 'uint256' },
      { name: 'flat', internalType: 'uint256', type: 'uint256' },
    ],
    name: 'setRecommendedIntentFee',
    outputs: [],
    stateMutability: 'nonpayable',
  },
  {
    type: 'function',
    inputs: [{ name: 'id', internalType: 'bytes32', type: 'bytes32' }],
    name: 'settleIntent',
    outputs: [],
    stateMutability: 'nonpayable',
  },
  {
    type: 'function',
    inputs: [{ name: 'newOwner', internalType: 'address', type: 'address' }],
    name: 'transferOwnership',
    outputs: [],
    stateMutability: 'payable',
  },
  {
    type: 'function',
    inputs: [{ name: 'id', internalType: 'bytes32', type: 'bytes32' }],
    name: 'unclaimIntent',
    outputs: [],
    stateMutability: 'nonpayable',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'eventSeq',
        internalType: 'uint256',
        type: 'uint256',
        indexed: true,
      },
      {
        name: 'prevTip',
        internalType: 'bytes32',
        type: 'bytes32',
        indexed: true,
      },
      {
        name: 'newTip',
        internalType: 'bytes32',
        type: 'bytes32',
        indexed: true,
      },
      {
        name: 'eventSignature',
        internalType: 'bytes32',
        type: 'bytes32',
        indexed: false,
      },
      {
        name: 'abiEncodedEventData',
        internalType: 'bytes',
        type: 'bytes',
        indexed: false,
      },
    ],
    name: 'EventAppended',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'id', internalType: 'bytes32', type: 'bytes32', indexed: true },
      {
        name: 'solver',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      {
        name: 'depositAmount',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
    ],
    name: 'IntentClaimed',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'id', internalType: 'bytes32', type: 'bytes32', indexed: true },
      {
        name: 'caller',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      { name: 'solved', internalType: 'bool', type: 'bool', indexed: false },
      { name: 'funded', internalType: 'bool', type: 'bool', indexed: false },
      { name: 'settled', internalType: 'bool', type: 'bool', indexed: false },
      {
        name: 'refundBeneficiary',
        internalType: 'address',
        type: 'address',
        indexed: false,
      },
      {
        name: 'escrowToken',
        internalType: 'address',
        type: 'address',
        indexed: false,
      },
      {
        name: 'escrowRefunded',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
      {
        name: 'depositToken',
        internalType: 'address',
        type: 'address',
        indexed: false,
      },
      {
        name: 'depositToCaller',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
      {
        name: 'depositToRefundBeneficiary',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
      {
        name: 'depositToSolver',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
    ],
    name: 'IntentClosed',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'id', internalType: 'bytes32', type: 'bytes32', indexed: true },
      {
        name: 'creator',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      {
        name: 'intentType',
        internalType: 'uint8',
        type: 'uint8',
        indexed: false,
      },
      {
        name: 'token',
        internalType: 'address',
        type: 'address',
        indexed: false,
      },
      {
        name: 'amount',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
      {
        name: 'refundBeneficiary',
        internalType: 'address',
        type: 'address',
        indexed: false,
      },
      {
        name: 'deadline',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
      {
        name: 'intentSpecs',
        internalType: 'bytes',
        type: 'bytes',
        indexed: false,
      },
    ],
    name: 'IntentCreated',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'id', internalType: 'bytes32', type: 'bytes32', indexed: true },
      {
        name: 'funder',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      {
        name: 'token',
        internalType: 'address',
        type: 'address',
        indexed: false,
      },
      {
        name: 'amount',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
    ],
    name: 'IntentFunded',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'id', internalType: 'bytes32', type: 'bytes32', indexed: true },
      {
        name: 'solver',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      {
        name: 'escrowToken',
        internalType: 'address',
        type: 'address',
        indexed: false,
      },
      {
        name: 'escrowAmount',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
      {
        name: 'depositToken',
        internalType: 'address',
        type: 'address',
        indexed: false,
      },
      {
        name: 'depositAmount',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
    ],
    name: 'IntentSettled',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'id', internalType: 'bytes32', type: 'bytes32', indexed: true },
      {
        name: 'solver',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      {
        name: 'tronTxId',
        internalType: 'bytes32',
        type: 'bytes32',
        indexed: false,
      },
      {
        name: 'tronBlockNumber',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
    ],
    name: 'IntentSolved',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'id', internalType: 'bytes32', type: 'bytes32', indexed: true },
      {
        name: 'caller',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      {
        name: 'prevSolver',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      { name: 'funded', internalType: 'bool', type: 'bool', indexed: false },
      {
        name: 'depositToCaller',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
      {
        name: 'depositToRefundBeneficiary',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
      {
        name: 'depositToPrevSolver',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
    ],
    name: 'IntentUnclaimed',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'oldOwner',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      {
        name: 'newOwner',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
    ],
    name: 'OwnershipTransferred',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'id', internalType: 'bytes32', type: 'bytes32', indexed: true },
      {
        name: 'feePpm',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
      {
        name: 'feeFlat',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
      {
        name: 'tronPaymentAmount',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
    ],
    name: 'ReceiverIntentFeeSnap',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'id', internalType: 'bytes32', type: 'bytes32', indexed: true },
      {
        name: 'forwarder',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      {
        name: 'toTron',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      {
        name: 'forwardSalt',
        internalType: 'bytes32',
        type: 'bytes32',
        indexed: false,
      },
      {
        name: 'token',
        internalType: 'address',
        type: 'address',
        indexed: false,
      },
      {
        name: 'amount',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
    ],
    name: 'ReceiverIntentParams',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'feePpm',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
      {
        name: 'feeFlat',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
    ],
    name: 'RecommendedIntentFeeSet',
  },
  { type: 'error', inputs: [], name: 'AlreadyClaimed' },
  { type: 'error', inputs: [], name: 'AlreadyExists' },
  { type: 'error', inputs: [], name: 'AlreadyFunded' },
  { type: 'error', inputs: [], name: 'AlreadyInitialized' },
  { type: 'error', inputs: [], name: 'AlreadySolved' },
  { type: 'error', inputs: [], name: 'IncorrectPullAmount' },
  { type: 'error', inputs: [], name: 'InsufficientETH' },
  { type: 'error', inputs: [], name: 'IntentNotFound' },
  { type: 'error', inputs: [], name: 'InvalidDeadline' },
  { type: 'error', inputs: [], name: 'InvalidReceiverAmount' },
  { type: 'error', inputs: [], name: 'NewOwnerIsZeroAddress' },
  { type: 'error', inputs: [], name: 'NotATrc20Transfer' },
  { type: 'error', inputs: [], name: 'NotClaimed' },
  { type: 'error', inputs: [], name: 'NotExpiredYet' },
  { type: 'error', inputs: [], name: 'NotSolver' },
  { type: 'error', inputs: [], name: 'NothingToSettle' },
  { type: 'error', inputs: [], name: 'Reentrancy' },
  { type: 'error', inputs: [], name: 'TronInvalidCalldataLength' },
  { type: 'error', inputs: [], name: 'TronInvalidTrc20DataLength' },
  { type: 'error', inputs: [], name: 'Unauthorized' },
  { type: 'error', inputs: [], name: 'WrongTxProps' },
] as const

//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
// UntronIntentsIndex
//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

export const untronIntentsIndexAbi = [
  {
    type: 'function',
    inputs: [],
    name: 'eventChainTip',
    outputs: [{ name: '', internalType: 'bytes32', type: 'bytes32' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'eventSeq',
    outputs: [{ name: '', internalType: 'uint256', type: 'uint256' }],
    stateMutability: 'view',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'eventSeq',
        internalType: 'uint256',
        type: 'uint256',
        indexed: true,
      },
      {
        name: 'prevTip',
        internalType: 'bytes32',
        type: 'bytes32',
        indexed: true,
      },
      {
        name: 'newTip',
        internalType: 'bytes32',
        type: 'bytes32',
        indexed: true,
      },
      {
        name: 'eventSignature',
        internalType: 'bytes32',
        type: 'bytes32',
        indexed: false,
      },
      {
        name: 'abiEncodedEventData',
        internalType: 'bytes',
        type: 'bytes',
        indexed: false,
      },
    ],
    name: 'EventAppended',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'id', internalType: 'bytes32', type: 'bytes32', indexed: true },
      {
        name: 'solver',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      {
        name: 'depositAmount',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
    ],
    name: 'IntentClaimed',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'id', internalType: 'bytes32', type: 'bytes32', indexed: true },
      {
        name: 'caller',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      { name: 'solved', internalType: 'bool', type: 'bool', indexed: false },
      { name: 'funded', internalType: 'bool', type: 'bool', indexed: false },
      { name: 'settled', internalType: 'bool', type: 'bool', indexed: false },
      {
        name: 'refundBeneficiary',
        internalType: 'address',
        type: 'address',
        indexed: false,
      },
      {
        name: 'escrowToken',
        internalType: 'address',
        type: 'address',
        indexed: false,
      },
      {
        name: 'escrowRefunded',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
      {
        name: 'depositToken',
        internalType: 'address',
        type: 'address',
        indexed: false,
      },
      {
        name: 'depositToCaller',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
      {
        name: 'depositToRefundBeneficiary',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
      {
        name: 'depositToSolver',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
    ],
    name: 'IntentClosed',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'id', internalType: 'bytes32', type: 'bytes32', indexed: true },
      {
        name: 'creator',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      {
        name: 'intentType',
        internalType: 'uint8',
        type: 'uint8',
        indexed: false,
      },
      {
        name: 'token',
        internalType: 'address',
        type: 'address',
        indexed: false,
      },
      {
        name: 'amount',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
      {
        name: 'refundBeneficiary',
        internalType: 'address',
        type: 'address',
        indexed: false,
      },
      {
        name: 'deadline',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
      {
        name: 'intentSpecs',
        internalType: 'bytes',
        type: 'bytes',
        indexed: false,
      },
    ],
    name: 'IntentCreated',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'id', internalType: 'bytes32', type: 'bytes32', indexed: true },
      {
        name: 'funder',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      {
        name: 'token',
        internalType: 'address',
        type: 'address',
        indexed: false,
      },
      {
        name: 'amount',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
    ],
    name: 'IntentFunded',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'id', internalType: 'bytes32', type: 'bytes32', indexed: true },
      {
        name: 'solver',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      {
        name: 'escrowToken',
        internalType: 'address',
        type: 'address',
        indexed: false,
      },
      {
        name: 'escrowAmount',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
      {
        name: 'depositToken',
        internalType: 'address',
        type: 'address',
        indexed: false,
      },
      {
        name: 'depositAmount',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
    ],
    name: 'IntentSettled',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'id', internalType: 'bytes32', type: 'bytes32', indexed: true },
      {
        name: 'solver',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      {
        name: 'tronTxId',
        internalType: 'bytes32',
        type: 'bytes32',
        indexed: false,
      },
      {
        name: 'tronBlockNumber',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
    ],
    name: 'IntentSolved',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'id', internalType: 'bytes32', type: 'bytes32', indexed: true },
      {
        name: 'caller',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      {
        name: 'prevSolver',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      { name: 'funded', internalType: 'bool', type: 'bool', indexed: false },
      {
        name: 'depositToCaller',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
      {
        name: 'depositToRefundBeneficiary',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
      {
        name: 'depositToPrevSolver',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
    ],
    name: 'IntentUnclaimed',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'oldOwner',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      {
        name: 'newOwner',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
    ],
    name: 'OwnershipTransferred',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'id', internalType: 'bytes32', type: 'bytes32', indexed: true },
      {
        name: 'feePpm',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
      {
        name: 'feeFlat',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
      {
        name: 'tronPaymentAmount',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
    ],
    name: 'ReceiverIntentFeeSnap',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'id', internalType: 'bytes32', type: 'bytes32', indexed: true },
      {
        name: 'forwarder',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      {
        name: 'toTron',
        internalType: 'address',
        type: 'address',
        indexed: true,
      },
      {
        name: 'forwardSalt',
        internalType: 'bytes32',
        type: 'bytes32',
        indexed: false,
      },
      {
        name: 'token',
        internalType: 'address',
        type: 'address',
        indexed: false,
      },
      {
        name: 'amount',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
    ],
    name: 'ReceiverIntentParams',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'feePpm',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
      {
        name: 'feeFlat',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
    ],
    name: 'RecommendedIntentFeeSet',
  },
] as const

//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
// UntronIsolatedTestBase
//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

export const untronIsolatedTestBaseAbi = [
  {
    type: 'function',
    inputs: [],
    name: 'IS_TEST',
    outputs: [{ name: '', internalType: 'bool', type: 'bool' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'excludeArtifacts',
    outputs: [
      {
        name: 'excludedArtifacts_',
        internalType: 'string[]',
        type: 'string[]',
      },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'excludeContracts',
    outputs: [
      {
        name: 'excludedContracts_',
        internalType: 'address[]',
        type: 'address[]',
      },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'excludeSelectors',
    outputs: [
      {
        name: 'excludedSelectors_',
        internalType: 'struct StdInvariant.FuzzSelector[]',
        type: 'tuple[]',
        components: [
          { name: 'addr', internalType: 'address', type: 'address' },
          { name: 'selectors', internalType: 'bytes4[]', type: 'bytes4[]' },
        ],
      },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'excludeSenders',
    outputs: [
      {
        name: 'excludedSenders_',
        internalType: 'address[]',
        type: 'address[]',
      },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'failed',
    outputs: [{ name: '', internalType: 'bool', type: 'bool' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'setUp',
    outputs: [],
    stateMutability: 'nonpayable',
  },
  {
    type: 'function',
    inputs: [],
    name: 'targetArtifactSelectors',
    outputs: [
      {
        name: 'targetedArtifactSelectors_',
        internalType: 'struct StdInvariant.FuzzArtifactSelector[]',
        type: 'tuple[]',
        components: [
          { name: 'artifact', internalType: 'string', type: 'string' },
          { name: 'selectors', internalType: 'bytes4[]', type: 'bytes4[]' },
        ],
      },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'targetArtifacts',
    outputs: [
      {
        name: 'targetedArtifacts_',
        internalType: 'string[]',
        type: 'string[]',
      },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'targetContracts',
    outputs: [
      {
        name: 'targetedContracts_',
        internalType: 'address[]',
        type: 'address[]',
      },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'targetInterfaces',
    outputs: [
      {
        name: 'targetedInterfaces_',
        internalType: 'struct StdInvariant.FuzzInterface[]',
        type: 'tuple[]',
        components: [
          { name: 'addr', internalType: 'address', type: 'address' },
          { name: 'artifacts', internalType: 'string[]', type: 'string[]' },
        ],
      },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'targetSelectors',
    outputs: [
      {
        name: 'targetedSelectors_',
        internalType: 'struct StdInvariant.FuzzSelector[]',
        type: 'tuple[]',
        components: [
          { name: 'addr', internalType: 'address', type: 'address' },
          { name: 'selectors', internalType: 'bytes4[]', type: 'bytes4[]' },
        ],
      },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'targetSenders',
    outputs: [
      {
        name: 'targetedSenders_',
        internalType: 'address[]',
        type: 'address[]',
      },
    ],
    stateMutability: 'view',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: '', internalType: 'string', type: 'string', indexed: false },
    ],
    name: 'log',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: '', internalType: 'address', type: 'address', indexed: false },
    ],
    name: 'log_address',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'val',
        internalType: 'uint256[]',
        type: 'uint256[]',
        indexed: false,
      },
    ],
    name: 'log_array',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'val',
        internalType: 'int256[]',
        type: 'int256[]',
        indexed: false,
      },
    ],
    name: 'log_array',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'val',
        internalType: 'address[]',
        type: 'address[]',
        indexed: false,
      },
    ],
    name: 'log_array',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: '', internalType: 'bytes', type: 'bytes', indexed: false },
    ],
    name: 'log_bytes',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: '', internalType: 'bytes32', type: 'bytes32', indexed: false },
    ],
    name: 'log_bytes32',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: '', internalType: 'int256', type: 'int256', indexed: false },
    ],
    name: 'log_int',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'key', internalType: 'string', type: 'string', indexed: false },
      { name: 'val', internalType: 'address', type: 'address', indexed: false },
    ],
    name: 'log_named_address',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'key', internalType: 'string', type: 'string', indexed: false },
      {
        name: 'val',
        internalType: 'uint256[]',
        type: 'uint256[]',
        indexed: false,
      },
    ],
    name: 'log_named_array',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'key', internalType: 'string', type: 'string', indexed: false },
      {
        name: 'val',
        internalType: 'int256[]',
        type: 'int256[]',
        indexed: false,
      },
    ],
    name: 'log_named_array',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'key', internalType: 'string', type: 'string', indexed: false },
      {
        name: 'val',
        internalType: 'address[]',
        type: 'address[]',
        indexed: false,
      },
    ],
    name: 'log_named_array',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'key', internalType: 'string', type: 'string', indexed: false },
      { name: 'val', internalType: 'bytes', type: 'bytes', indexed: false },
    ],
    name: 'log_named_bytes',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'key', internalType: 'string', type: 'string', indexed: false },
      { name: 'val', internalType: 'bytes32', type: 'bytes32', indexed: false },
    ],
    name: 'log_named_bytes32',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'key', internalType: 'string', type: 'string', indexed: false },
      { name: 'val', internalType: 'int256', type: 'int256', indexed: false },
      {
        name: 'decimals',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
    ],
    name: 'log_named_decimal_int',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'key', internalType: 'string', type: 'string', indexed: false },
      { name: 'val', internalType: 'uint256', type: 'uint256', indexed: false },
      {
        name: 'decimals',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
    ],
    name: 'log_named_decimal_uint',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'key', internalType: 'string', type: 'string', indexed: false },
      { name: 'val', internalType: 'int256', type: 'int256', indexed: false },
    ],
    name: 'log_named_int',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'key', internalType: 'string', type: 'string', indexed: false },
      { name: 'val', internalType: 'string', type: 'string', indexed: false },
    ],
    name: 'log_named_string',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'key', internalType: 'string', type: 'string', indexed: false },
      { name: 'val', internalType: 'uint256', type: 'uint256', indexed: false },
    ],
    name: 'log_named_uint',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: '', internalType: 'string', type: 'string', indexed: false },
    ],
    name: 'log_string',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: '', internalType: 'uint256', type: 'uint256', indexed: false },
    ],
    name: 'log_uint',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: '', internalType: 'bytes', type: 'bytes', indexed: false },
    ],
    name: 'logs',
  },
] as const

//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
// UntronReceiver
//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

export const untronReceiverAbi = [
  { type: 'constructor', inputs: [], stateMutability: 'nonpayable' },
  { type: 'receive', stateMutability: 'payable' },
  {
    type: 'function',
    inputs: [],
    name: 'OWNER',
    outputs: [{ name: '', internalType: 'address payable', type: 'address' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [
      { name: 'token', internalType: 'address', type: 'address' },
      { name: 'amount', internalType: 'uint256', type: 'uint256' },
    ],
    name: 'pull',
    outputs: [],
    stateMutability: 'nonpayable',
  },
  { type: 'error', inputs: [], name: 'NotOwner' },
] as const

//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
// UntronTestBase
//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

export const untronTestBaseAbi = [
  {
    type: 'function',
    inputs: [],
    name: 'IS_TEST',
    outputs: [{ name: '', internalType: 'bool', type: 'bool' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'excludeArtifacts',
    outputs: [
      {
        name: 'excludedArtifacts_',
        internalType: 'string[]',
        type: 'string[]',
      },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'excludeContracts',
    outputs: [
      {
        name: 'excludedContracts_',
        internalType: 'address[]',
        type: 'address[]',
      },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'excludeSelectors',
    outputs: [
      {
        name: 'excludedSelectors_',
        internalType: 'struct StdInvariant.FuzzSelector[]',
        type: 'tuple[]',
        components: [
          { name: 'addr', internalType: 'address', type: 'address' },
          { name: 'selectors', internalType: 'bytes4[]', type: 'bytes4[]' },
        ],
      },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'excludeSenders',
    outputs: [
      {
        name: 'excludedSenders_',
        internalType: 'address[]',
        type: 'address[]',
      },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'failed',
    outputs: [{ name: '', internalType: 'bool', type: 'bool' }],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'setUp',
    outputs: [],
    stateMutability: 'nonpayable',
  },
  {
    type: 'function',
    inputs: [],
    name: 'targetArtifactSelectors',
    outputs: [
      {
        name: 'targetedArtifactSelectors_',
        internalType: 'struct StdInvariant.FuzzArtifactSelector[]',
        type: 'tuple[]',
        components: [
          { name: 'artifact', internalType: 'string', type: 'string' },
          { name: 'selectors', internalType: 'bytes4[]', type: 'bytes4[]' },
        ],
      },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'targetArtifacts',
    outputs: [
      {
        name: 'targetedArtifacts_',
        internalType: 'string[]',
        type: 'string[]',
      },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'targetContracts',
    outputs: [
      {
        name: 'targetedContracts_',
        internalType: 'address[]',
        type: 'address[]',
      },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'targetInterfaces',
    outputs: [
      {
        name: 'targetedInterfaces_',
        internalType: 'struct StdInvariant.FuzzInterface[]',
        type: 'tuple[]',
        components: [
          { name: 'addr', internalType: 'address', type: 'address' },
          { name: 'artifacts', internalType: 'string[]', type: 'string[]' },
        ],
      },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'targetSelectors',
    outputs: [
      {
        name: 'targetedSelectors_',
        internalType: 'struct StdInvariant.FuzzSelector[]',
        type: 'tuple[]',
        components: [
          { name: 'addr', internalType: 'address', type: 'address' },
          { name: 'selectors', internalType: 'bytes4[]', type: 'bytes4[]' },
        ],
      },
    ],
    stateMutability: 'view',
  },
  {
    type: 'function',
    inputs: [],
    name: 'targetSenders',
    outputs: [
      {
        name: 'targetedSenders_',
        internalType: 'address[]',
        type: 'address[]',
      },
    ],
    stateMutability: 'view',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: '', internalType: 'string', type: 'string', indexed: false },
    ],
    name: 'log',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: '', internalType: 'address', type: 'address', indexed: false },
    ],
    name: 'log_address',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'val',
        internalType: 'uint256[]',
        type: 'uint256[]',
        indexed: false,
      },
    ],
    name: 'log_array',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'val',
        internalType: 'int256[]',
        type: 'int256[]',
        indexed: false,
      },
    ],
    name: 'log_array',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      {
        name: 'val',
        internalType: 'address[]',
        type: 'address[]',
        indexed: false,
      },
    ],
    name: 'log_array',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: '', internalType: 'bytes', type: 'bytes', indexed: false },
    ],
    name: 'log_bytes',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: '', internalType: 'bytes32', type: 'bytes32', indexed: false },
    ],
    name: 'log_bytes32',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: '', internalType: 'int256', type: 'int256', indexed: false },
    ],
    name: 'log_int',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'key', internalType: 'string', type: 'string', indexed: false },
      { name: 'val', internalType: 'address', type: 'address', indexed: false },
    ],
    name: 'log_named_address',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'key', internalType: 'string', type: 'string', indexed: false },
      {
        name: 'val',
        internalType: 'uint256[]',
        type: 'uint256[]',
        indexed: false,
      },
    ],
    name: 'log_named_array',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'key', internalType: 'string', type: 'string', indexed: false },
      {
        name: 'val',
        internalType: 'int256[]',
        type: 'int256[]',
        indexed: false,
      },
    ],
    name: 'log_named_array',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'key', internalType: 'string', type: 'string', indexed: false },
      {
        name: 'val',
        internalType: 'address[]',
        type: 'address[]',
        indexed: false,
      },
    ],
    name: 'log_named_array',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'key', internalType: 'string', type: 'string', indexed: false },
      { name: 'val', internalType: 'bytes', type: 'bytes', indexed: false },
    ],
    name: 'log_named_bytes',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'key', internalType: 'string', type: 'string', indexed: false },
      { name: 'val', internalType: 'bytes32', type: 'bytes32', indexed: false },
    ],
    name: 'log_named_bytes32',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'key', internalType: 'string', type: 'string', indexed: false },
      { name: 'val', internalType: 'int256', type: 'int256', indexed: false },
      {
        name: 'decimals',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
    ],
    name: 'log_named_decimal_int',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'key', internalType: 'string', type: 'string', indexed: false },
      { name: 'val', internalType: 'uint256', type: 'uint256', indexed: false },
      {
        name: 'decimals',
        internalType: 'uint256',
        type: 'uint256',
        indexed: false,
      },
    ],
    name: 'log_named_decimal_uint',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'key', internalType: 'string', type: 'string', indexed: false },
      { name: 'val', internalType: 'int256', type: 'int256', indexed: false },
    ],
    name: 'log_named_int',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'key', internalType: 'string', type: 'string', indexed: false },
      { name: 'val', internalType: 'string', type: 'string', indexed: false },
    ],
    name: 'log_named_string',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: 'key', internalType: 'string', type: 'string', indexed: false },
      { name: 'val', internalType: 'uint256', type: 'uint256', indexed: false },
    ],
    name: 'log_named_uint',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: '', internalType: 'string', type: 'string', indexed: false },
    ],
    name: 'log_string',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: '', internalType: 'uint256', type: 'uint256', indexed: false },
    ],
    name: 'log_uint',
  },
  {
    type: 'event',
    anonymous: false,
    inputs: [
      { name: '', internalType: 'bytes', type: 'bytes', indexed: false },
    ],
    name: 'logs',
  },
] as const
