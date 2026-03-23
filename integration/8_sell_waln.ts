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
  const usdcMintStr = process.env.USDC_MINT;
  const amountRaw = process.env.WALN_AMOUNT;
  if (!walnMintStr) throw new Error("Set WALN_MINT");
  if (!usdcMintStr) throw new Error("Set USDC_MINT");
  if (!amountRaw) throw new Error("Set WALN_AMOUNT (in WALN base units)");

  const envProvider = AnchorProvider.env();
  const payer = loadKeypairFromEnv();
  const provider = createProviderWithPayer(envProvider, payer);
  const sdk = createSdk(provider);

  const walnMint = new PublicKey(walnMintStr);
  const usdcMint = new PublicKey(usdcMintStr);
  const walnAmount = new BN(amountRaw);

  const sellerWalnAccount = await getOrCreateAssociatedTokenAccount(
    provider.connection,
    payer,
    walnMint,
    payer.publicKey,
    false,
    undefined,
    undefined,
    TOKEN_2022_PROGRAM_ID
  );

  const sellerUsdcAccount = await getOrCreateAssociatedTokenAccount(
    provider.connection,
    payer,
    usdcMint,
    payer.publicKey
  );

  const [contractStatePda] = sdk.contractStatePda();
  const stateAccount = await sdk.program.account.programState.fetch(contractStatePda);
  const currentRoundIndex = new BN(stateAccount.roundCount.toString());
  const [roundRecord] = sdk.roundRecordPda(currentRoundIndex);
  const [roundLockedWaln] = sdk.roundLockedWalnPda(currentRoundIndex);

  const ix = await sdk.sellWalnIx({
    seller: payer.publicKey,
    sellerWalnAccount: sellerWalnAccount.address,
    sellerUsdcAccount: sellerUsdcAccount.address,
    walnMint,
    usdcMint,
    walnAmount,
    walnTokenProgram: TOKEN_2022_PROGRAM_ID,
    roundTriggerAccounts: [
      { pubkey: roundRecord, isWritable: true },
      { pubkey: roundLockedWaln, isWritable: true },
    ],
  });

  const tx = new Transaction().add(ix);
  const sig = await provider.sendAndConfirm(tx, [payer]);
  console.log("sell_waln tx:", sig);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
