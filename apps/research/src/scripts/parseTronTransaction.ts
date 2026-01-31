import { z } from "zod";
import { parseEnv } from "../lib/env.js";
import { log } from "../lib/logger.js";
import { createTronClients } from "@untron/tron-protocol";
import type { BytesMessage } from "@untron/tron-protocol/api";
import { createHash } from "node:crypto";
import {
  Transaction,
  Transaction_Contract_ContractType,
  transaction_Contract_ContractTypeToJSON,
  transaction_Result_codeToJSON,
  transaction_Result_contractResultToJSON,
  type Transaction_Contract,
  Transaction_raw,
} from "@untron/tron-protocol/tron";
import { TriggerSmartContract } from "@untron/tron-protocol/core/contract/smart_contract";

function toHex(buf: Uint8Array | Buffer): string {
  return Buffer.from(buf).toString("hex");
}

function toHex0x(buf: Uint8Array | Buffer): string {
  return `0x${Buffer.from(buf).toString("hex")}`;
}

function tronAddressToEvmAddress(bytes: Uint8Array | Buffer): string | null {
  // Tron address is 21 bytes: 0x41 prefix + 20-byte EVM address
  if (bytes.length !== 21 || bytes[0] !== 0x41) return null;
  return `0x${Buffer.from(bytes.subarray(1)).toString("hex")}`;
}

function readVarint(buf: Uint8Array, startOffset: number): { value: bigint; offset: number } {
  let result = 0n;
  let shift = 0n;
  let offset = startOffset;

  while (offset < buf.length) {
    const byteValue = buf[offset];
    if (byteValue === undefined) {
      throw new Error("Unexpected end of buffer while reading varint");
    }
    const byte = BigInt(byteValue);
    result |= (byte & 0x7fn) << shift;
    offset += 1;
    if ((byte & 0x80n) === 0n) break;
    shift += 7n;
  }

  return { value: result, offset };
}

type ParsedTransactionResult = {
  fee?: bigint;
  ret?: bigint;
  contractRet?: bigint;
};

type ParsedAny = {
  typeUrl?: string;
  value?: Uint8Array;
};

type ParsedContract = {
  type?: bigint;
  parameter?: ParsedAny;
  provider?: string;
  contractName?: string;
  permissionId?: bigint;
};

type ParsedTransactionRaw = {
  refBlockBytes?: string;
  refBlockNum?: bigint;
  refBlockHash?: string;
  expiration?: bigint;
  timestamp?: bigint;
  feeLimit?: bigint;
  data?: string;
  contracts: ParsedContract[];
};

type ParsedTransaction = {
  raw?: ParsedTransactionRaw;
  signatures: string[];
  results: ParsedTransactionResult[];
};

type ParsedTriggerSmartContract = {
  ownerAddress?: string;
  contractAddress?: string;
  callValue?: bigint;
  data?: string;
  callTokenValue?: bigint;
  tokenId?: bigint;
};

function parseAny(bytes: Uint8Array): ParsedAny {
  let offset = 0;
  const out: ParsedAny = {};

  while (offset < bytes.length) {
    const keyRes = readVarint(bytes, offset);
    const key = keyRes.value;
    offset = keyRes.offset;

    const fieldNumber = Number(key >> 3n);
    const wireType = Number(key & 0x7n);

    if (wireType === 2) {
      const lenRes = readVarint(bytes, offset);
      const len = Number(lenRes.value);
      offset = lenRes.offset;

      const end = offset + len;
      const fieldBytes = bytes.slice(offset, end);
      offset = end;

      switch (fieldNumber) {
        case 1: {
          // type_url
          out.typeUrl = Buffer.from(fieldBytes).toString("utf8");
          break;
        }
        case 2: {
          // value
          out.value = fieldBytes;
          break;
        }
        default: {
          // unknown len-delimited field – ignore
          break;
        }
      }
    } else if (wireType === 0) {
      // VARINT – skip
      const valRes = readVarint(bytes, offset);
      offset = valRes.offset;
    } else {
      throw new Error(`Unsupported protobuf wire type ${wireType} in Any at offset ${offset}`);
    }
  }

  return out;
}

function parseTransactionResult(bytes: Uint8Array): ParsedTransactionResult {
  let offset = 0;
  const out: ParsedTransactionResult = {};

  while (offset < bytes.length) {
    const keyRes = readVarint(bytes, offset);
    const key = keyRes.value;
    offset = keyRes.offset;

    const fieldNumber = Number(key >> 3n);
    const wireType = Number(key & 0x7n);

    if (wireType === 0) {
      const valRes = readVarint(bytes, offset);
      const value = valRes.value;
      offset = valRes.offset;

      switch (fieldNumber) {
        case 1: {
          // fee
          out.fee = value;
          break;
        }
        case 2: {
          // ret (status code enum)
          out.ret = value;
          break;
        }
        case 3: {
          // contractRet enum
          out.contractRet = value;
          break;
        }
        default: {
          // other int64 / enum fields – parsed but not stored
          break;
        }
      }
    } else if (wireType === 2) {
      // LEN-DELIMITED – skip contents
      const lenRes = readVarint(bytes, offset);
      const len = Number(lenRes.value);
      offset = lenRes.offset + len;
    } else {
      throw new Error(
        `Unsupported protobuf wire type ${wireType} in Transaction.Result at offset ${offset}`
      );
    }
  }

  return out;
}

function parseContract(bytes: Uint8Array): ParsedContract {
  let offset = 0;
  const out: ParsedContract = {};

  while (offset < bytes.length) {
    const keyRes = readVarint(bytes, offset);
    const key = keyRes.value;
    offset = keyRes.offset;

    const fieldNumber = Number(key >> 3n);
    const wireType = Number(key & 0x7n);

    if (wireType === 0) {
      const valRes = readVarint(bytes, offset);
      const value = valRes.value;
      offset = valRes.offset;

      switch (fieldNumber) {
        case 1: {
          // type
          out.type = value;
          break;
        }
        case 5: {
          // Permission_id
          out.permissionId = value;
          break;
        }
        default: {
          // unknown varint field – parsed but not stored
          break;
        }
      }
    } else if (wireType === 2) {
      const lenRes = readVarint(bytes, offset);
      const len = Number(lenRes.value);
      offset = lenRes.offset;

      const end = offset + len;
      const fieldBytes = bytes.slice(offset, end);
      offset = end;

      switch (fieldNumber) {
        case 2: {
          // parameter (google.protobuf.Any)
          out.parameter = parseAny(fieldBytes);
          break;
        }
        case 3: {
          // provider
          out.provider = toHex0x(fieldBytes);
          break;
        }
        case 4: {
          // ContractName
          out.contractName = Buffer.from(fieldBytes).toString("utf8");
          break;
        }
        default: {
          // unknown bytes field – ignore
          break;
        }
      }
    } else {
      throw new Error(
        `Unsupported protobuf wire type ${wireType} in Transaction.Contract at offset ${offset}`
      );
    }
  }

  return out;
}

function parseTransactionRaw(bytes: Uint8Array): ParsedTransactionRaw {
  let offset = 0;
  const out: ParsedTransactionRaw = {
    contracts: [],
  };

  while (offset < bytes.length) {
    const keyRes = readVarint(bytes, offset);
    const key = keyRes.value;
    offset = keyRes.offset;

    const fieldNumber = Number(key >> 3n);
    const wireType = Number(key & 0x7n);

    if (wireType === 0) {
      const valRes = readVarint(bytes, offset);
      const value = valRes.value;
      offset = valRes.offset;

      switch (fieldNumber) {
        case 3: {
          // ref_block_num
          out.refBlockNum = value;
          break;
        }
        case 8: {
          // expiration
          out.expiration = value;
          break;
        }
        case 14: {
          // timestamp
          out.timestamp = value;
          break;
        }
        case 18: {
          // fee_limit
          out.feeLimit = value;
          break;
        }
        default: {
          // other int64 fields – ignore for now
          break;
        }
      }
    } else if (wireType === 2) {
      const lenRes = readVarint(bytes, offset);
      const len = Number(lenRes.value);
      offset = lenRes.offset;

      const end = offset + len;
      const fieldBytes = bytes.slice(offset, end);
      offset = end;

      switch (fieldNumber) {
        case 1: {
          // ref_block_bytes
          out.refBlockBytes = toHex0x(fieldBytes);
          break;
        }
        case 4: {
          // ref_block_hash
          out.refBlockHash = toHex0x(fieldBytes);
          break;
        }
        case 10: {
          // data
          out.data = toHex0x(fieldBytes);
          break;
        }
        case 11: {
          // Contract
          out.contracts.push(parseContract(fieldBytes));
          break;
        }
        default: {
          // other bytes / message fields – ignore
          break;
        }
      }
    } else {
      throw new Error(
        `Unsupported protobuf wire type ${wireType} in Transaction.raw at offset ${offset}`
      );
    }
  }

  return out;
}

function parseTransactionBytes(bytes: Uint8Array): ParsedTransaction {
  let offset = 0;
  const out: ParsedTransaction = {
    signatures: [],
    results: [],
  };

  while (offset < bytes.length) {
    const keyRes = readVarint(bytes, offset);
    const key = keyRes.value;
    offset = keyRes.offset;

    const fieldNumber = Number(key >> 3n);
    const wireType = Number(key & 0x7n);

    if (wireType === 2) {
      const lenRes = readVarint(bytes, offset);
      const len = Number(lenRes.value);
      offset = lenRes.offset;

      const end = offset + len;
      const fieldBytes = bytes.slice(offset, end);
      offset = end;

      switch (fieldNumber) {
        case 1: {
          // raw_data
          out.raw = parseTransactionRaw(fieldBytes);
          break;
        }
        case 2: {
          // signature
          out.signatures.push(toHex0x(fieldBytes));
          break;
        }
        case 5: {
          // Result
          out.results.push(parseTransactionResult(fieldBytes));
          break;
        }
        default: {
          // other len-delimited fields – ignore
          break;
        }
      }
    } else if (wireType === 0) {
      // VARINT – skip
      const valRes = readVarint(bytes, offset);
      offset = valRes.offset;
    } else {
      throw new Error(
        `Unsupported protobuf wire type ${wireType} in Transaction at offset ${offset}`
      );
    }
  }

  return out;
}

function parseTriggerSmartContractBytes(bytes: Uint8Array): ParsedTriggerSmartContract {
  let offset = 0;
  const out: ParsedTriggerSmartContract = {};

  while (offset < bytes.length) {
    const keyRes = readVarint(bytes, offset);
    const key = keyRes.value;
    offset = keyRes.offset;

    const fieldNumber = Number(key >> 3n);
    const wireType = Number(key & 0x7n);

    if (wireType === 0) {
      // VARINT
      const valRes = readVarint(bytes, offset);
      const value = valRes.value;
      offset = valRes.offset;

      switch (fieldNumber) {
        case 3: {
          // call_value
          out.callValue = value;
          break;
        }
        case 5: {
          // call_token_value
          out.callTokenValue = value;
          break;
        }
        case 6: {
          // token_id
          out.tokenId = value;
          break;
        }
        default: {
          // unknown varint field – parsed but not stored
          break;
        }
      }
    } else if (wireType === 2) {
      // LEN-DELIMITED (bytes)
      const lenRes = readVarint(bytes, offset);
      const len = Number(lenRes.value);
      offset = lenRes.offset;

      const end = offset + len;
      const fieldBytes = bytes.slice(offset, end);
      offset = end;

      switch (fieldNumber) {
        case 1: {
          // owner_address
          out.ownerAddress = toHex(fieldBytes);
          break;
        }
        case 2: {
          // contract_address
          out.contractAddress = toHex(fieldBytes);
          break;
        }
        case 4: {
          // data
          out.data = toHex(fieldBytes);
          break;
        }
        default: {
          // unknown bytes field – ignore
          break;
        }
      }
    } else {
      throw new Error(
        `Unsupported protobuf wire type ${wireType} in TriggerSmartContract at offset ${offset}`
      );
    }
  }

  return out;
}

function parseTxIdArg(argv: string[]): string {
  const args = argv.slice(2);
  let candidate: string | undefined;

  if (args.length === 1) {
    // Direct invocation: `tsx parseTronTransaction.ts <txId>`
    candidate = args[0];
  } else if (args.length >= 2) {
    // Runner style: `tsx src/run.ts parseTronTransaction <txId>`
    candidate = args[1];
  }

  const value = candidate ?? "";

  if (!/^(0x)?[0-9a-fA-F]{64}$/.test(value)) {
    throw new Error("Expected txId as 32-byte hex string (with or without 0x prefix)");
  }

  return value.replace(/^0x/i, "").toLowerCase();
}

function splitSelectorAndArgs(data: Buffer): { selector?: string; args?: string } {
  if (data.length === 0) return {};
  if (data.length <= 4) {
    return { selector: toHex0x(data) };
  }

  const selector = data.subarray(0, 4);
  const args = data.subarray(4);

  return {
    selector: toHex0x(selector),
    args: toHex0x(args),
  };
}

function decodeTriggerContracts(contracts: Transaction_Contract[]) {
  return contracts
    .filter((c) => c.type === Transaction_Contract_ContractType.TriggerSmartContract)
    .map((contract, index) => {
      const param = contract.parameter;
      if (!param) {
        return {
          index,
          type: transaction_Contract_ContractTypeToJSON(contract.type),
          error: "Missing parameter",
        };
      }

      const rawBytes = param.value as Buffer;

      const manual = parseTriggerSmartContractBytes(rawBytes);
      const decoded = TriggerSmartContract.decode(rawBytes);

      const ownerTron = decoded.ownerAddress;
      const contractTron = decoded.contractAddress;
      const ownerEvm = tronAddressToEvmAddress(ownerTron);
      const contractEvm = tronAddressToEvmAddress(contractTron);
      const dataBuf = decoded.data;
      const { selector, args } = splitSelectorAndArgs(dataBuf);

      return {
        index,
        type: transaction_Contract_ContractTypeToJSON(contract.type),
        manual: {
          ownerAddress: manual.ownerAddress,
          contractAddress: manual.contractAddress,
          callValue: manual.callValue !== undefined ? manual.callValue.toString() : undefined,
          data: manual.data ? `0x${manual.data}` : undefined,
          callTokenValue:
            manual.callTokenValue !== undefined ? manual.callTokenValue.toString() : undefined,
          tokenId: manual.tokenId !== undefined ? manual.tokenId.toString() : undefined,
        },
        tsProto: {
          ownerAddress: toHex0x(ownerTron),
          ownerAddressEvm: ownerEvm ?? undefined,
          contractAddress: toHex0x(contractTron),
          contractAddressEvm: contractEvm ?? undefined,
          callValue: decoded.callValue.toString(),
          data: toHex0x(dataBuf),
          selector,
          args,
          callTokenValue: decoded.callTokenValue.toString(),
          tokenId: decoded.tokenId.toString(),
        },
      };
    });
}

async function main() {
  const env = parseEnv(
    z.object({
      TRON_GRPC_HOST: z.string().min(1),
      TRON_API_KEY: z.string().optional(),
    })
  );

  const txIdHex = parseTxIdArg(process.argv);

  const { wallet, callOpts } = createTronClients(env.TRON_GRPC_HOST, env.TRON_API_KEY, {
    insecure: true,
  });

  const request: BytesMessage = { value: Buffer.from(txIdHex, "hex") };

  const tx = await new Promise<Transaction>((resolve, reject) => {
    wallet.getTransactionById(request, callOpts.metadata, (err, res) => {
      if (err || !res) return reject(err ?? new Error("Empty response from getTransactionById"));
      resolve(res);
    });
  });

  const raw = tx.rawData;
  if (!raw) {
    throw new Error("Transaction rawData is missing");
  }

  // Tron txid is sha256(protobuf(Transaction.rawData)).
  const rawDataBytes = Transaction_raw.encode(raw).finish();
  const computedTxIdHex = createHash("sha256").update(rawDataBytes).digest("hex");
  if (computedTxIdHex !== txIdHex) {
    throw new Error(
      `TxID mismatch: provided=0x${txIdHex} computed=0x${computedTxIdHex} (sha256(raw_data_bytes))`
    );
  }

  const encodedTx = Transaction.encode(tx).finish();
  const manualTx = parseTransactionBytes(encodedTx);

  log.info("Transaction encoded bytes (hex)", {
    txId: `0x${txIdHex}`,
    hex: toHex(encodedTx),
  });

  const triggerContracts = decodeTriggerContracts(raw.contract ?? []);

  log.info("TriggerSmartContract transaction decoding", {
    txId: `0x${txIdHex}`,
    resultsTsProto:
      tx.ret?.map((result, index) => ({
        index,
        fee: result.fee.toString(),
        code: transaction_Result_codeToJSON(result.ret),
        contractResult: transaction_Result_contractResultToJSON(result.contractRet),
      })) ?? [],
    resultsManual: manualTx.results.map((result, index) => ({
      index,
      fee: result.fee !== undefined ? result.fee.toString() : undefined,
      retCode: result.ret !== undefined ? result.ret.toString() : undefined,
      contractRetCode: result.contractRet !== undefined ? result.contractRet.toString() : undefined,
    })),
    rawData: {
      refBlockBytes: toHex0x(raw.refBlockBytes),
      refBlockNum: raw.refBlockNum.toString(),
      refBlockHash: toHex0x(raw.refBlockHash),
      expiration: raw.expiration.toString(),
      timestamp: raw.timestamp.toString(),
      feeLimit: raw.feeLimit.toString(),
      contracts: (raw.contract ?? []).map((c, index) => ({
        index,
        type: transaction_Contract_ContractTypeToJSON(c.type),
        hasParameter: !!c.parameter,
      })),
    },
    rawDataManual: manualTx.raw
      ? {
          refBlockBytes: manualTx.raw.refBlockBytes,
          refBlockNum: manualTx.raw.refBlockNum?.toString(),
          refBlockHash: manualTx.raw.refBlockHash,
          expiration: manualTx.raw.expiration?.toString(),
          timestamp: manualTx.raw.timestamp?.toString(),
          feeLimit: manualTx.raw.feeLimit?.toString(),
          data: manualTx.raw.data,
          contracts: manualTx.raw.contracts.map((c, index) => ({
            index,
            typeNumeric: c.type !== undefined ? c.type.toString() : undefined,
            typeJson:
              c.type !== undefined
                ? transaction_Contract_ContractTypeToJSON(
                    Number(c.type) as Transaction_Contract_ContractType
                  )
                : undefined,
            provider: c.provider,
            contractName: c.contractName,
            permissionId: c.permissionId !== undefined ? c.permissionId.toString() : undefined,
            parameter: c.parameter
              ? {
                  typeUrl: c.parameter.typeUrl,
                  valueHex: c.parameter.value ? toHex0x(c.parameter.value) : undefined,
                }
              : undefined,
          })),
        }
      : undefined,
    signaturesManual: manualTx.signatures,
    triggerContracts,
    triggerContractsPretty: JSON.stringify(triggerContracts, null, 2),
  });
}

main().catch((err) => {
  log.error(err);
  process.exit(1);
});
