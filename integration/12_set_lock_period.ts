import "dotenv/config";
import { AnchorProvider, BN } from "@coral-xyz/anchor";
import { Transaction } from "@solana/web3.js";
import {
  loadKeypairFromEnv,
  createProviderWithPayer,
  createSdk,
} from "./helpers/common";

async function main() {
  const periodRaw = process.env.LOCK_PERIOD_SECONDS;
  if (!periodRaw) throw new Error("Set LOCK_PERIOD_SECONDS (e.g. 86400 for 1 day)");

  const envProvider = AnchorProvider.env();
  const payer = loadKeypairFromEnv();
  const provider = createProviderWithPayer(envProvider, payer);
  const sdk = createSdk(provider);

  const ix = await sdk.admin(payer.publicKey).setLockPeriod(new BN(periodRaw));

  const tx = new Transaction().add(ix);
  const sig = await provider.sendAndConfirm(tx, [payer]);
  console.log("set_lock_period tx:", sig);
  console.log("New lock period (seconds):", periodRaw);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
