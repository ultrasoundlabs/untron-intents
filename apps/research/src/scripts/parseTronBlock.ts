import Long from "long";
import { z } from "zod";
import { parseEnv } from "../lib/env.js";
import { log } from "../lib/logger.js";
import { createTronClients } from "@untron/tron-protocol";
import { BlockHeader as BlockHeaderCodec, BlockHeader_raw } from "@untron/tron-protocol/tron";
import type { BlockExtention, EmptyMessage, NumberMessage } from "@untron/tron-protocol/api";
import { sha256 } from "@noble/hashes/sha2.js";
import { recoverAddress, type Address, type Hex } from "viem";

function toHex(bytes: Uint8Array | Buffer): string {
  return Buffer.from(bytes).toString("hex");
}

function toHex0x(bytes: Uint8Array | Buffer): Hex {
  return `0x${Buffer.from(bytes).toString("hex")}` as Hex;
}

function tronWitnessAddressToEvmAddress(bytes: Uint8Array | Buffer): Address | null {
  // Tron witness address is 21 bytes: 0x41 prefix + 20-byte EVM address
  if (bytes.length !== 21 || bytes[0] !== 0x41) return null;
  return `0x${Buffer.from(bytes.subarray(1)).toString("hex")}` as Address;
}

function tronWitnessSignatureToViemSignature(sig: Buffer): Hex | null {
  // Tron stores signatures as [r(32) | s(32) | v(1)]
  if (!sig || sig.length < 65) return null;
  const r = sig.subarray(0, 32);
  const s = sig.subarray(32, 64);
  let v = Number(sig[64]! & 0xff);

  // Normalize eth-style v (27/28) -> 0/1 if needed
  if (v >= 27) v -= 27;
  if (v !== 0 && v !== 1) return null;

  const rs = Buffer.concat([r, s]);
  const vHex = v.toString(16).padStart(2, "0");
  return `0x${rs.toString("hex")}${vHex}` as Hex;
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

type ParsedBlockHeaderRaw = {
  timestamp?: bigint;
  txTrieRoot?: string;
  parentHash?: string;
  number?: bigint;
  witnessId?: bigint;
  witnessAddress?: string;
  version?: bigint;
  accountStateRoot?: string;
};

type ParsedBlockHeader = {
  raw?: ParsedBlockHeaderRaw;
  witnessSignature?: string;
};

function parseBlockHeaderBytes(bytes: Uint8Array): ParsedBlockHeader {
  let offset = 0;
  const out: ParsedBlockHeader = {};

  while (offset < bytes.length) {
    const keyRes = readVarint(bytes, offset);
    const key = keyRes.value;
    offset = keyRes.offset;

    const fieldNumber = Number(key >> 3n);
    const wireType = Number(key & 0x7n);

    if (wireType === 2) {
      // LEN-DELIMITED
      const lenRes = readVarint(bytes, offset);
      const len = Number(lenRes.value);
      offset = lenRes.offset;

      const end = offset + len;
      const fieldBytes = bytes.slice(offset, end);
      offset = end;

      switch (fieldNumber) {
        case 1: {
          // rawData (nested BlockHeader_raw)
          out.raw = parseBlockHeaderRawBytes(fieldBytes);
          break;
        }
        case 2: {
          // witnessSignature
          out.witnessSignature = toHex(fieldBytes);
          break;
        }
        default: {
          // unknown len-delimited field – ignore
          break;
        }
      }
    } else {
      throw new Error(
        `Unsupported protobuf wire type ${wireType} in BlockHeader at offset ${offset}`
      );
    }
  }

  return out;
}

function parseBlockHeaderRawBytes(bytes: Uint8Array): ParsedBlockHeaderRaw {
  let offset = 0;
  const out: ParsedBlockHeaderRaw = {};

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
        case 1: {
          // timestamp
          out.timestamp = value;
          break;
        }
        case 7: {
          // number
          out.number = value;
          break;
        }
        case 8: {
          // witnessId
          out.witnessId = value;
          break;
        }
        case 10: {
          // version (int32 but encoded as varint)
          out.version = value;
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
        case 2: {
          // txTrieRoot
          out.txTrieRoot = toHex(fieldBytes);
          break;
        }
        case 3: {
          // parentHash
          out.parentHash = toHex(fieldBytes);
          break;
        }
        case 9: {
          // witnessAddress
          out.witnessAddress = toHex(fieldBytes);
          break;
        }
        case 11: {
          // accountStateRoot
          out.accountStateRoot = toHex(fieldBytes);
          break;
        }
        default: {
          // unknown bytes field – ignore
          break;
        }
      }
    } else {
      throw new Error(`Unsupported protobuf wire type ${wireType} at offset ${offset}`);
    }
  }

  return out;
}

async function main() {
  const env = parseEnv(
    z.object({
      TRON_GRPC_HOST: z.string().min(1),
      TRON_API_KEY: z.string().optional(),
    })
  );

  const args = process.argv.slice(2);
  let blockNumberArg: string | undefined;

  if (args.length === 1) {
    // Direct invocation style: `tsx parseTronBlock.ts 12345`
    const candidate = args[0] ?? "";
    if (/^[0-9]+$/.test(candidate)) {
      blockNumberArg = candidate;
    }
  } else if (args.length >= 2) {
    // Runner style: `tsx src/run.ts parseTronBlock 12345`
    const candidate = args[1] ?? "";
    if (/^[0-9]+$/.test(candidate)) {
      blockNumberArg = candidate;
    }
  }

  const { wallet, callOpts } = createTronClients(env.TRON_GRPC_HOST, env.TRON_API_KEY, {
    insecure: true,
  });

  let block: BlockExtention;

  if (blockNumberArg) {
    const blockNumberLong = Long.fromString(blockNumberArg, true);
    const request: NumberMessage = { num: blockNumberLong };

    block = await new Promise((resolve, reject) => {
      wallet.getBlockByNum2(request, callOpts.metadata, (err, res) => {
        if (err || !res) return reject(err ?? new Error("Empty response from getBlockByNum2"));
        resolve(res);
      });
    });
  } else {
    // No block number provided: default to latest block (getNowBlock2)
    block = await new Promise((resolve, reject) => {
      wallet.getNowBlock2({} as EmptyMessage, callOpts.metadata, (err, res) => {
        if (err || !res) return reject(err ?? new Error("Empty response from getNowBlock2"));
        resolve(res);
      });
    });
  }

  const header = block.blockHeader;
  const raw = header?.rawData;

  if (!header || !raw) {
    throw new Error("Block header or rawData is missing on the fetched block");
  }

  // Encode raw header and compute the digest Tron signs (sha256 of BlockHeader_raw)
  const rawBytes = BlockHeader_raw.encode(raw as BlockHeader_raw).finish();
  const digestHex = toHex0x(sha256(rawBytes));

  // Derive the EVM-style address from Tron witnessAddress (0x41 prefix + 20 bytes)
  const tronWitness = raw.witnessAddress;
  const witnessEvmAddress = tronWitness ? tronWitnessAddressToEvmAddress(tronWitness) : null;

  // Convert Tron witnessSignature ([r|s|v]) into a viem-style 65-byte signature hex
  const witnessSigBuf = header.witnessSignature as Buffer | undefined;
  const signatureHex = witnessSigBuf ? tronWitnessSignatureToViemSignature(witnessSigBuf) : null;

  let recoveredAddress: Address | null = null;
  let signatureVerified: boolean | null = null;

  if (witnessEvmAddress && signatureHex) {
    try {
      recoveredAddress = await recoverAddress({
        hash: digestHex,
        signature: signatureHex,
      });
      if (recoveredAddress) {
        signatureVerified =
          recoveredAddress.toLowerCase() === (witnessEvmAddress as string).toLowerCase();
      }
    } catch (err) {
      log.error("Failed to recover address from Tron witness signature", { err });
    }
  } else {
    log.warn("Cannot verify Tron witness signature", {
      hasWitnessAddress: !!tronWitness,
      witnessAddressLength: tronWitness?.length,
      hasWitnessSignature: !!witnessSigBuf,
      witnessSignatureLength: witnessSigBuf?.length,
    });
  }

  const encodedHeader = BlockHeaderCodec.encode(header).finish();

  log.info("BlockHeader encoded bytes (hex)", {
    hex: toHex(encodedHeader),
  });

  const parsedHeader = parseBlockHeaderBytes(encodedHeader);
  const parsedRaw = parsedHeader.raw;

  log.info("BlockHeader comparison (manual protobuf decoding vs gRPC object)", {
    manual: {
      rawData: {
        timestamp: parsedRaw?.timestamp !== undefined ? parsedRaw.timestamp.toString() : undefined,
        txTrieRoot: parsedRaw?.txTrieRoot,
        parentHash: parsedRaw?.parentHash,
        number: parsedRaw?.number !== undefined ? parsedRaw.number.toString() : undefined,
        witnessId: parsedRaw?.witnessId !== undefined ? parsedRaw.witnessId.toString() : undefined,
        witnessAddress: parsedRaw?.witnessAddress,
        version: parsedRaw?.version !== undefined ? parsedRaw.version.toString() : undefined,
        accountStateRoot: parsedRaw?.accountStateRoot,
      },
      witnessSignature: parsedHeader.witnessSignature,
    },
    grpc: {
      rawData: {
        timestamp: raw.timestamp.toString(),
        txTrieRoot: toHex(raw.txTrieRoot),
        parentHash: toHex(raw.parentHash),
        number: raw.number.toString(),
        witnessId: raw.witnessId.toString(), // these are deprecated in Tron protocol and are always undefined
        witnessAddress: toHex(raw.witnessAddress),
        version: raw.version.toString(), // always 32
        accountStateRoot: toHex(raw.accountStateRoot), // these are deprecated too
      },
      witnessSignature: toHex(header.witnessSignature),
    },
    viem: {
      hash: digestHex,
      signature: signatureHex ?? undefined,
      recoveredAddress: recoveredAddress ?? undefined,
      witnessEvmAddress: witnessEvmAddress ?? undefined,
      verified: signatureVerified ?? undefined,
    },
  });
}

main().catch((err) => {
  log.error(err);
  process.exit(1);
});
