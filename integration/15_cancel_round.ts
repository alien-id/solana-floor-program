import "dotenv/config";
import { AnchorProvider } from "@coral-xyz/anchor";
import { Transaction } from "@solana/web3.js";
import {
  loadKeypairFromEnv,
  createProviderWithPayer,
  createSdk,
} from "./helpers/common";

async function main() {
  const envProvider = AnchorProvider.env();
  const payer = loadKeypairFromEnv();
  const provider = createProviderWithPayer(envProvider, payer);
  const sdk = createSdk(provider);

  const ix = await sdk.admin(payer.publicKey).cancelRound();

  const tx = new Transaction().add(ix);
  const sig = await provider.sendAndConfirm(tx, [payer]);
  console.log("cancel_round tx:", sig);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
