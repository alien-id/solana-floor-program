import "dotenv/config";
import { AnchorProvider, BN } from "@coral-xyz/anchor";
import { PublicKey, Transaction } from "@solana/web3.js";
import { getOrCreateAssociatedTokenAccount } from "@solana/spl-token";
import {
  loadKeypairFromEnv,
  createProviderWithPayer,
  createSdk,
} from "./helpers/common";

async function main() {
  const usdcMintStr = process.env.USDC_MINT;
  const amountRaw = process.env.USDC_AMOUNT;
  if (!usdcMintStr) throw new Error("Set USDC_MINT");
  if (!amountRaw) throw new Error("Set USDC_AMOUNT (in USDC base units)");

  const envProvider = AnchorProvider.env();
  const payer = loadKeypairFromEnv();
  const provider = createProviderWithPayer(envProvider, payer);
  const sdk = createSdk(provider);

  const usdcMint = new PublicKey(usdcMintStr);
  const amount = new BN(amountRaw);

  const investorUsdcAccount = await getOrCreateAssociatedTokenAccount(
    provider.connection,
    payer,
    usdcMint,
    payer.publicKey
  );

  const ix = await sdk.withdrawUsdcIx({
    investor: payer.publicKey,
    investorUsdcAccount: investorUsdcAccount.address,
    usdcMint,
    amount,
  });

  const tx = new Transaction().add(ix);
  const sig = await provider.sendAndConfirm(tx, [payer]);
  console.log("withdraw_usdc tx:", sig);

  const record = await sdk.fetchInvestorRecord(payer.publicKey);
  if (record) {
    console.log("Investor record:", {
      usdcDeposited: record.usdcDeposited.toString(),
      usdcLockedCurrentRound: record.usdcLockedCurrentRound.toString(),
    });
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
