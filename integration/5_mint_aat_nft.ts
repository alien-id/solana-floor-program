import "dotenv/config";
import { AnchorProvider, BN } from "@coral-xyz/anchor";
import { ComputeBudgetProgram, PublicKey, Transaction } from "@solana/web3.js";
import {
  loadKeypairFromEnv,
  createProviderWithPayer,
  createSdk,
} from "./helpers/common";

async function main() {
  const investorStr = process.env.INVESTOR;
  const aatVolumeRaw = process.env.AAT_VOLUME;
  if (!investorStr) throw new Error("Set INVESTOR (investor pubkey)");
  if (!aatVolumeRaw) throw new Error("Set AAT_VOLUME (allocation amount)");

  const envProvider = AnchorProvider.env();
  const payer = loadKeypairFromEnv();
  const provider = createProviderWithPayer(envProvider, payer);
  const sdk = createSdk(provider);

  const investor = new PublicKey(investorStr);
  const aatVolume = new BN(aatVolumeRaw);

  const ix = await sdk.mintAatNftIx({
    admin: payer.publicKey,
    investor,
    aatVolume,
  });

  const [nftMint] = sdk.aatNftMintPda(investor);

  const tx = new Transaction().add(
    ComputeBudgetProgram.setComputeUnitLimit({ units: 400_000 }),
    ix
  );
  const sig = await provider.sendAndConfirm(tx, [payer]);
  console.log("mint_aat_nft tx:", sig);
  console.log("AAT NFT mint PDA:", nftMint.toBase58());
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
