import "dotenv/config";
import { AnchorProvider } from "@coral-xyz/anchor";
import { PublicKey, Transaction } from "@solana/web3.js";
import {
  getAssociatedTokenAddressSync,
  TOKEN_PROGRAM_ID,
} from "@solana/spl-token";
import {
  loadKeypairFromEnv,
  createProviderWithPayer,
  createSdk,
} from "./helpers/common";

async function main() {
  const investorStr = process.env.INVESTOR;
  if (!investorStr) throw new Error("Set INVESTOR (investor pubkey)");

  const envProvider = AnchorProvider.env();
  const payer = loadKeypairFromEnv();
  const provider = createProviderWithPayer(envProvider, payer);
  const sdk = createSdk(provider);

  const investor = new PublicKey(investorStr);

  const [contractState] = sdk.contractStatePda();
  const state = await sdk.program.account.programState.fetch(contractState);
  const usdcMint = state.usdcMint;
  const investorUsdcAccount = getAssociatedTokenAddressSync(
    usdcMint,
    investor,
    true,
    TOKEN_PROGRAM_ID
  );

  const ix = await sdk.admin(payer.publicKey).removeInvestorFromPool({
    investor,
    usdcMint,
    investorUsdcAccount,
  });

  const tx = new Transaction().add(ix);
  const sig = await provider.sendAndConfirm(tx, [payer]);
  console.log("remove_investor_from_pool tx:", sig);
  console.log("Investor:", investorStr);
  console.log("Refund USDC ATA:", investorUsdcAccount.toBase58());
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
