/**
 * Minimally computes Tron Merkle proof for a transaction in a block.
 * Usage: tsx src/scripts/tronTxMerkleProof.ts <blockNumber> <txId>
 */
import Long from "long";
import { z } from "zod";
import { parseEnv } from "../lib/env.js";
import { createTronClients } from "@untron/tron-protocol";
import { BlockExtention } from "@untron/tron-protocol/api";
import { Transaction } from "@untron/tron-protocol/tron";
import { sha256 } from "@noble/hashes/sha2.js";

function toHex0x(buf: Uint8Array | Buffer): string {
  return `0x${Buffer.from(buf).toString("hex")}`;
}

function sha256Concat(a: Buffer, b: Buffer): Buffer {
  return Buffer.from(sha256(Buffer.concat([a, b])));
}

function computeMerkleRootCarryUp(leaves: Buffer[]): Buffer {
  if (leaves.length === 0) throw new Error("empty merkle tree");
  let level = leaves.slice();
  while (level.length > 1) {
    const next: Buffer[] = [];
    for (let i = 0; i < level.length; i += 2) {
      const left = level[i]!;
      const right = level[i + 1];
      if (!right) {
        // Tron txTrieRoot uses a carry-up rule (no self-duplication).
        next.push(left);
      } else {
        next.push(sha256Concat(left, right));
      }
    }
    level = next;
  }
  return level[0]!;
}

function computeMerkleRootDuplicateLast(leaves: Buffer[]): Buffer {
  if (leaves.length === 0) throw new Error("empty merkle tree");
  let level = leaves.slice();
  while (level.length > 1) {
    const next: Buffer[] = [];
    for (let i = 0; i < level.length; i += 2) {
      const left = level[i]!;
      const right = level[i + 1] ?? left;
      next.push(sha256Concat(left, right));
    }
    level = next;
  }
  return level[0]!;
}

class MerkleNode {
  digest: Buffer;
  left?: MerkleNode;
  right?: MerkleNode;
  parent?: MerkleNode;

  constructor(digest: Buffer, left?: MerkleNode, right?: MerkleNode) {
    this.digest = digest;
    this.left = left;
    this.right = right;
    if (left) left.parent = this;
    if (right) right.parent = this;
  }

  isRoot() {
    return !this.parent;
  }
  isLeftChild() {
    return this.parent?.left === this;
  }
  getAncestor(degree: number) {
    let curr: MerkleNode = this;
    while (degree-- > 0 && curr.parent) curr = curr.parent;
    return curr;
  }
}

class MerkleLeaf extends MerkleNode {
  constructor(
    public data: Buffer,
    digest: Buffer
  ) {
    super(digest);
  }
}

function decompose(n: number): number[] {
  const exponents: number[] = [];
  for (let i = 0; i < 32; i++) {
    if ((n >> i) & 1) exponents.push(i);
  }
  return exponents;
}

class InMemoryMerkleTree {
  root?: MerkleNode;
  leaves: MerkleLeaf[] = [];

  appendEntry(data: Buffer) {
    const digest = Buffer.from(sha256(data));
    const tail = new MerkleLeaf(data, digest);

    if (this.leaves.length === 0) {
      this.leaves.push(tail);
      this.root = tail;
      return;
    }

    const node = this.leaves[this.leaves.length - 1]!.getAncestor(
      decompose(this.leaves.length)[0]!
    );
    this.leaves.push(tail);
    const parentDigest = sha256Concat(node.digest, tail.digest);

    if (node.isRoot()) {
      this.root = new MerkleNode(parentDigest, node, tail);
      return;
    }

    const curr = node.parent!;
    const subRoot = new MerkleNode(parentDigest, node, tail);
    curr.right = subRoot;
    subRoot.parent = curr;

    let ptr = curr;
    while (true) {
      ptr.digest = sha256Concat(ptr.left!.digest, ptr.right!.digest);
      if (!ptr.parent) break;
      ptr = ptr.parent;
    }
  }

  getProof(index: number) {
    if (index < 0 || index >= this.leaves.length) throw new Error("Index out of bounds");

    const leafNode = this.leaves[index]!;
    const proof: Buffer[] = [];
    let curr: MerkleNode = leafNode;
    let pathBits = 0;
    let depth = 0;

    while (curr.parent) {
      const parent = curr.parent;
      if (curr.isLeftChild()) {
        proof.push(parent.right!.digest);
      } else {
        proof.push(parent.left!.digest);
        pathBits |= 1 << depth;
      }
      curr = parent;
      depth++;
    }

    return {
      leaf: leafNode.digest,
      proof,
      root: this.root!.digest,
      index: pathBits,
      totalLeaves: this.leaves.length,
    };
  }
}

async function main() {
  const rawArgs = process.argv.slice(2);
  const args = rawArgs.length > 0 && /^\d+$/.test(rawArgs[0]!) ? rawArgs : rawArgs.slice(1);

  if (args.length < 2) {
    console.error("Usage: tsx tronTxMerkleProof.ts <blockNumber> <txId>");
    process.exit(1);
  }

  const blockNumber = Long.fromString(args[0]!);
  const txIdHex = args[1]!.replace(/^0x/i, "").toLowerCase();

  const env = parseEnv(
    z.object({ TRON_GRPC_HOST: z.string().min(1), TRON_API_KEY: z.string().optional() })
  );
  const { wallet, callOpts } = createTronClients(env.TRON_GRPC_HOST, env.TRON_API_KEY, {
    insecure: true,
  });

  const block = await new Promise<BlockExtention>((resolve, reject) => {
    wallet.getBlockByNum2({ num: blockNumber }, callOpts.metadata, (err, res) =>
      err ? reject(err) : resolve(res)
    );
  });

  if (!block.transactions || block.transactions.length === 0)
    throw new Error("Block has no transactions");

  const tree = new InMemoryMerkleTree();
  let targetIndex = -1;

  for (let i = 0; i < block.transactions.length; i++) {
    const txExt = block.transactions[i]!;
    const tx: Transaction = txExt.transaction!;
    // a proof that we can in fact get the transaction results from the transaction object
    // that we hash for the merkle tree
    console.log(`tx ${i}: ${tx.ret[0]!.ret === 0 ? "SUCESS" : "FAILED"}`); // i shit you not it's literally SUCESS in the protocol
    const encoded = Buffer.from(Transaction.encode(tx).finish());
    tree.appendEntry(encoded);

    const txid = Buffer.from(txExt.txid).toString("hex").toLowerCase();
    if (txid === txIdHex) targetIndex = i;
  }

  if (targetIndex === -1) throw new Error("Transaction not found in block");

  const result = tree.getProof(targetIndex);

  const headerRoot = block.blockHeader?.rawData?.txTrieRoot;
  if (!headerRoot) {
    throw new Error("Block header does not contain txTrieRoot");
  }
  const calculatedRootHex = toHex0x(result.root);
  const headerRootHex = toHex0x(headerRoot);

  const leafDigests = tree.leaves.map((l) => l.digest);
  const carryUpRoot = computeMerkleRootCarryUp(leafDigests);
  const duplicateLastRoot = computeMerkleRootDuplicateLast(leafDigests);

  if (calculatedRootHex !== headerRootHex) {
    throw new Error(
      `Merkle root mismatch! Calculated: ${calculatedRootHex}, Header: ${headerRootHex}`
    );
  }

  console.log(
    JSON.stringify(
      {
        root: toHex0x(result.root),
        leaf: toHex0x(result.leaf),
        proof: result.proof.map(toHex0x),
        index: result.index.toString(),
        totalLeaves: result.totalLeaves.toString(),
        diagnostics: {
          txCount: block.transactions.length,
          targetIndex,
          proofLen: result.proof.length,
          headerRoot: headerRootHex,
          carryUpRoot: toHex0x(carryUpRoot),
          duplicateLastRoot: toHex0x(duplicateLastRoot),
          carryUpMatchesHeader: toHex0x(carryUpRoot) === headerRootHex,
          duplicateLastMatchesHeader: toHex0x(duplicateLastRoot) === headerRootHex,
        },
      },
      null,
      2
    )
  );
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
