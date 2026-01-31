type Jsonish = null | boolean | number | string | Jsonish[] | { [k: string]: Jsonish };

function isRecord(x: unknown): x is Record<string, unknown> {
  return typeof x === "object" && x !== null;
}

function truncateString(s: string, max: number): string {
  if (s.length <= max) return s;
  return `${s.slice(0, Math.max(0, max - 20))}…(len=${s.length})`;
}

function sanitizeUnknown(x: unknown, depth: number, maxString: number): Jsonish {
  if (depth <= 0) return "[max-depth]";
  if (x == null) return null;
  if (typeof x === "string") return truncateString(x, maxString);
  if (typeof x === "number") return Number.isFinite(x) ? x : String(x);
  if (typeof x === "boolean") return x;
  if (typeof x === "bigint") return x.toString();

  if (Array.isArray(x)) {
    const out: Jsonish[] = [];
    const limit = 20;
    for (let i = 0; i < Math.min(limit, x.length); i++) {
      out.push(sanitizeUnknown(x[i], depth - 1, maxString));
    }
    if (x.length > limit) out.push(`[+${x.length - limit} more]`);
    return out;
  }

  // Avoid dumping binary blobs / typed arrays
  if (x instanceof Uint8Array) return { type: "Uint8Array", byteLength: x.byteLength };
  if (typeof Buffer !== "undefined" && Buffer.isBuffer(x))
    return { type: "Buffer", byteLength: x.byteLength };

  if (!isRecord(x)) return String(x);

  // Prefer known "error-like" fields and avoid huge nested request/response bodies.
  const errorLikeKeys = [
    "name",
    "message",
    "shortMessage",
    "details",
    "code",
    "cause",
    "metaMessages",
    "version",
    "status",
  ];

  const out: Record<string, Jsonish> = {};
  for (const k of errorLikeKeys) {
    if (!(k in x)) continue;
    // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
    out[k] = sanitizeUnknown((x as any)[k], depth - 1, maxString);
  }

  // If we didn't capture anything useful, include a shallow view of keys (but truncated).
  if (Object.keys(out).length === 0) {
    const keys = Object.keys(x).slice(0, 20);
    out.type = "object";
    out.keys = keys as unknown as Jsonish;
  }

  return out;
}

export function summarizeError(err: unknown): { message: string; data?: Jsonish } {
  if (err instanceof Error) {
    return {
      message: err.message || err.name || "Error",
      data: sanitizeUnknown(err, 3, 800),
    };
  }
  if (typeof err === "string") return { message: err };
  return {
    message: "Unknown error",
    data: sanitizeUnknown(err, 3, 800),
  };
}
