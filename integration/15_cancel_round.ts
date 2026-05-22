import "dotenv/config";
import { AnchorProvider, BN } from "@coral-xyz/anchor";
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

  const [contractStatePda] = sdk.contractStatePda();
  const stateAccount = await sdk.program.account.programState.fetch(contractStatePda);
  const roundIndex = new BN(stateAccount.roundCount.toString());
  console.log("Cancelling round index:", roundIndex.toString());

  const ix = await sdk.admin(payer.publicKey).cancelRound(roundIndex);

  const tx = new Transaction().add(ix);
  const sig = await provider.sendAndConfirm(tx, [payer]);
  console.log("cancel_round tx:", sig);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
