import "dotenv/config";
import { AnchorProvider, BN } from "@coral-xyz/anchor";
import { PublicKey, Transaction } from "@solana/web3.js";
import {
  loadKeypairFromEnv,
  createProviderWithPayer,
  createSdk,
} from "./helpers/common";

async function main() {
  const investorStr = process.env.INVESTOR_PUBKEY;
  const unlockTsRaw = process.env.UNLOCK_TIMESTAMP;
  if (!investorStr) throw new Error("Set INVESTOR_PUBKEY");
  if (!unlockTsRaw) throw new Error("Set UNLOCK_TIMESTAMP (Unix timestamp in seconds)");

  const envProvider = AnchorProvider.env();
  const payer = loadKeypairFromEnv();
  const provider = createProviderWithPayer(envProvider, payer);
  const sdk = createSdk(provider);

  const investor = new PublicKey(investorStr);
  const newUnlockTs = new BN(unlockTsRaw);

  const ix = await sdk.admin(payer.publicKey).setInvestorUsdcUnlock(investor, newUnlockTs);

  const tx = new Transaction().add(ix);
  const sig = await provider.sendAndConfirm(tx, [payer]);
  console.log("set_investor_usdc_unlock tx:", sig);
  console.log("Investor:", investorStr);
  console.log("New unlock timestamp:", unlockTsRaw);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
