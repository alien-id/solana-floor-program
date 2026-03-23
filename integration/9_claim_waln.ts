import "dotenv/config";
import { AnchorProvider, BN } from "@coral-xyz/anchor";
import { PublicKey, Transaction } from "@solana/web3.js";
import {
  getOrCreateAssociatedTokenAccount,
  TOKEN_2022_PROGRAM_ID,
} from "@solana/spl-token";
import {
  loadKeypairFromEnv,
  createProviderWithPayer,
  createSdk,
} from "./helpers/common";

async function main() {
  const walnMintStr = process.env.WALN_MINT;
  const roundIndexRaw = process.env.ROUND_INDEX;
  if (!walnMintStr) throw new Error("Set WALN_MINT");
  if (!roundIndexRaw) throw new Error("Set ROUND_INDEX (0-based round index)");

  const envProvider = AnchorProvider.env();
  const payer = loadKeypairFromEnv();
  const provider = createProviderWithPayer(envProvider, payer);
  const sdk = createSdk(provider);

  const walnMint = new PublicKey(walnMintStr);
  const roundIndex = new BN(roundIndexRaw);

  const investorWalnAccount = await getOrCreateAssociatedTokenAccount(
    provider.connection,
    payer,
    walnMint,
    payer.publicKey,
    false,
    undefined,
    undefined,
    TOKEN_2022_PROGRAM_ID
  );

  const alloc = await sdk.fetchInvestorAlloc(roundIndex, payer.publicKey);
  if (!alloc) {
    throw new Error(`No allocation found for investor ${payer.publicKey.toBase58()} in round ${roundIndexRaw}`);
  }
  console.log("Investor allocation:", {
    walnAmount: alloc.walnAmount.toString(),
    unlock: alloc.unlock.toString(),
    claimed: alloc.claimed,
  });

  const ix = await sdk.claimWalnIx({
    investor: payer.publicKey,
    investorWalnAccount: investorWalnAccount.address,
    walnMint,
    roundIndex,
    walnTokenProgram: TOKEN_2022_PROGRAM_ID,
  });

  const tx = new Transaction().add(ix);
  const sig = await provider.sendAndConfirm(tx, [payer]);
  console.log("claim_waln tx:", sig);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
