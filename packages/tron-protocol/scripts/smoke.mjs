import { createTronClients } from "../dist/index.js";

const host = process.env.TRON_GRPC_HOST;
if (!host) {
  console.log("Skipping smoke: TRON_GRPC_HOST not set");
  process.exit(0);
}

const apiKey = process.env.TRON_API_KEY;

const { wallet, callOpts } = createTronClients(host, apiKey, { insecure: true });

await new Promise((resolve, reject) => {
  wallet.getNowBlock({}, callOpts, (err, res) => {
    if (err) return reject(err);
    const numLong = res?.blockHeader?.rawData?.number;
    const numStr =
      numLong && typeof numLong.toString === "function"
        ? numLong.toString()
        : String(numLong ?? "");
    console.log("Now block number:", numStr);
    resolve();
  });
});
