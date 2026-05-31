import "dotenv/config";
import { AnchorProvider } from "@coral-xyz/anchor";
import { Transaction } from "@solana/web3.js";
import {
  loadKeypairFromEnv,
  createProviderWithPayer,
  createSdk,
} from "./helpers/common";

async function main() {
  const pausedStr = process.env.PAUSED;
  if (!pausedStr) throw new Error("Set PAUSED (true or false)");

  const paused = pausedStr === "true";

  const envProvider = AnchorProvider.env();
  const payer = loadKeypairFromEnv();
  const provider = createProviderWithPayer(envProvider, payer);
  const sdk = createSdk(provider);

  const ix = await sdk.admin(payer.publicKey).setSellPaused(paused);

  const tx = new Transaction().add(ix);
  const sig = await provider.sendAndConfirm(tx, [payer]);
  console.log("set_sell_paused tx:", sig);
  console.log("Sell paused:", paused);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
