import "dotenv/config";
import { AnchorProvider, BN } from "@coral-xyz/anchor";
import { Transaction } from "@solana/web3.js";
import {
  loadKeypairFromEnv,
  createProviderWithPayer,
  createSdk,
} from "./helpers/common";

async function main() {
  const lockRaw = process.env.USDC_WITHDRAW_LOCK_SECONDS;
  if (!lockRaw) throw new Error("Set USDC_WITHDRAW_LOCK_SECONDS (e.g. 7776000 for 90 days)");

  const envProvider = AnchorProvider.env();
  const payer = loadKeypairFromEnv();
  const provider = createProviderWithPayer(envProvider, payer);
  const sdk = createSdk(provider);

  const ix = await sdk.admin(payer.publicKey).setUsdcWithdrawLock(new BN(lockRaw));

  const tx = new Transaction().add(ix);
  const sig = await provider.sendAndConfirm(tx, [payer]);
  console.log("set_usdc_withdraw_lock tx:", sig);
  console.log("New USDC withdraw lock (seconds):", lockRaw);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
