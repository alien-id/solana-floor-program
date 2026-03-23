import "dotenv/config";
import { AnchorProvider, BN } from "@coral-xyz/anchor";
import { Transaction } from "@solana/web3.js";
import {
  loadKeypairFromEnv,
  createProviderWithPayer,
  createSdk,
} from "./helpers/common";

async function main() {
  const amountRaw = process.env.AMOUNT_LAMPORTS;
  if (!amountRaw) throw new Error("Set AMOUNT_LAMPORTS (amount in lamports to fund the treasury)");

  const envProvider = AnchorProvider.env();
  const payer = loadKeypairFromEnv();
  const provider = createProviderWithPayer(envProvider, payer);
  const sdk = createSdk(provider);

  const ix = await sdk.admin(payer.publicKey).fundTreasury(new BN(amountRaw));

  const [treasury] = sdk.treasuryPda();

  const tx = new Transaction().add(ix);
  const sig = await provider.sendAndConfirm(tx, [payer]);
  console.log("fund_treasury tx:", sig);
  console.log("Treasury PDA:", treasury.toBase58());
  console.log("Funded:", amountRaw, "lamports");
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
