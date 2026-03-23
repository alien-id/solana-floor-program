import "dotenv/config";
import { AnchorProvider, BN } from "@coral-xyz/anchor";
import { Transaction } from "@solana/web3.js";
import {
  loadKeypairFromEnv,
  createProviderWithPayer,
  createSdk,
} from "./helpers/common";

async function main() {
  const sizeRaw = process.env.ROUND_SIZE_WALN;
  if (!sizeRaw) throw new Error("Set ROUND_SIZE_WALN (in WALN base units, e.g. 200000000000 for 200 WALN)");

  const envProvider = AnchorProvider.env();
  const payer = loadKeypairFromEnv();
  const provider = createProviderWithPayer(envProvider, payer);
  const sdk = createSdk(provider);

  const ix = await sdk.admin(payer.publicKey).setRoundSize(new BN(sizeRaw));

  const tx = new Transaction().add(ix);
  const sig = await provider.sendAndConfirm(tx, [payer]);
  console.log("set_round_size tx:", sig);
  console.log("New round size:", sizeRaw);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
