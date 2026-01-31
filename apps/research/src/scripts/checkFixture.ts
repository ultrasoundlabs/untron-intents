import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { sha256 } from "@noble/hashes/sha2.js";
import { recoverAddress, type Address, type Hex } from "viem";

type Fixture = {
  srs: Address[];
  compressedTronBlockMetadata: Hex;
  compressedSignatures: Hex;
  blockNumbers: string[];
  blockIds: Hex[];
  blockHashes: Hex[];
  blockHeaderRawBytes: Hex[];
  witnessEvmAddresses: Address[];
  witnessIndices: number[];
  witnessSignatures: Hex[];
};

function hexToBytes(hex: string): Uint8Array {
  const clean = hex.startsWith("0x") ? hex.slice(2) : hex;
  if (clean.length % 2 !== 0) {
    throw new Error(`Invalid hex length: ${clean.length}`);
  }
  const out = new Uint8Array(clean.length / 2);
  for (let i = 0; i < out.length; i++) {
    out[i] = parseInt(clean.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
}

async function main() {
  const argv = process.argv.slice(2);
  if (argv.length < 1) {
    // eslint-disable-next-line no-console
    console.error("Usage: tsx checkFixture.ts <fixturePath>");
    process.exit(1);
  }

  const path = resolve(argv[0]!);
  const json = JSON.parse(readFileSync(path, "utf8")) as Fixture;

  const {
    srs,
    blockHeaderRawBytes,
    blockHashes,
    witnessEvmAddresses,
    witnessIndices,
    witnessSignatures,
  } = json;

  if (blockHeaderRawBytes.length !== blockHashes.length) {
    throw new Error("blockHeaderRawBytes/blockHashes length mismatch in fixture");
  }
  if (blockHeaderRawBytes.length !== witnessEvmAddresses.length) {
    throw new Error("blockHeaderRawBytes/witnessEvmAddresses length mismatch in fixture");
  }
  if (blockHeaderRawBytes.length !== witnessIndices.length) {
    throw new Error("blockHeaderRawBytes/witnessIndices length mismatch in fixture");
  }
  if (blockHeaderRawBytes.length !== witnessSignatures.length) {
    throw new Error("blockHeaderRawBytes/witnessSignatures length mismatch in fixture");
  }

  // eslint-disable-next-line no-console
  console.log(`Loaded fixture with ${blockHeaderRawBytes.length} blocks`);

  for (let i = 0; i < blockHeaderRawBytes.length; i++) {
    const rawBytes = hexToBytes(blockHeaderRawBytes[i]!);
    const digest = sha256(rawBytes);
    const digestHex = `0x${Buffer.from(digest).toString("hex")}` as Hex;

    if (digestHex.toLowerCase() !== blockHashes[i]!.toLowerCase()) {
      // eslint-disable-next-line no-console
      console.error(`hash mismatch at index ${i}`);
      break;
    }

    const sigHex = witnessSignatures[i]!;
    const sigBytes = hexToBytes(sigHex);
    if (sigBytes.length !== 65) {
      // eslint-disable-next-line no-console
      console.error(`signature length != 65 at index ${i}: ${sigBytes.length}`);
      break;
    }

    // Tron stores [r(32) | s(32) | v(1)] with v in {0,1,27,28}.
    const r = sigBytes.subarray(0, 32);
    const s = sigBytes.subarray(32, 64);
    let v = sigBytes[64]!;
    if (v >= 27) v -= 27;
    if (v !== 0 && v !== 1) {
      // eslint-disable-next-line no-console
      console.error(`unexpected v at index ${i}: ${v}`);
      break;
    }

    const rsHex = Buffer.from(r).toString("hex") + Buffer.from(s).toString("hex");
    const sigForViem = `0x${rsHex}${v.toString(16).padStart(2, "0")}` as Hex;

    const recovered = await recoverAddress({
      hash: digestHex,
      signature: sigForViem,
    });

    const witnessAddr = witnessEvmAddresses[i]!;
    const idx = witnessIndices[i]!;
    const srsAddr = srs[idx]!;

    const okVsWitness = recovered.toLowerCase() === (witnessAddr as string).toLowerCase();
    const okVsSrs = recovered.toLowerCase() === (srsAddr as string).toLowerCase();

    // eslint-disable-next-line no-console
    console.log(
      [
        `blockIndex=${i}`,
        `digest=${digestHex}`,
        `recovered=${recovered}`,
        `witness=${witnessAddr}`,
        `srs[${idx}]=${srsAddr}`,
        `matchesWitness=${okVsWitness}`,
        `matchesSrs=${okVsSrs}`,
      ].join(" | ")
    );
  }
}

main().catch((err) => {
  // eslint-disable-next-line no-console
  console.error(err);
  process.exit(1);
});
