import "dotenv/config";
import { AnchorProvider, BN } from "@coral-xyz/anchor";
import { PublicKey, Transaction } from "@solana/web3.js";
import {
  loadKeypairFromEnv,
  createProviderWithPayer,
  createSdk,
} from "./helpers/common";

async function main() {
  const investorStr = process.env.INVESTOR;
  const newVolumeRaw = process.env.NEW_AAT_VOLUME;
  if (!investorStr) throw new Error("Set INVESTOR (investor pubkey)");
  if (!newVolumeRaw) throw new Error("Set NEW_AAT_VOLUME (new allocation amount)");

  const envProvider = AnchorProvider.env();
  const payer = loadKeypairFromEnv();
  const provider = createProviderWithPayer(envProvider, payer);
  const sdk = createSdk(provider);

  const investor = new PublicKey(investorStr);
  const newVolume = new BN(newVolumeRaw);

  const ix = await sdk.updateAatVolumeIx({
    admin: payer.publicKey,
    investor,
    newVolume,
  });

  const tx = new Transaction().add(ix);
  const sig = await provider.sendAndConfirm(tx, [payer]);
  console.log("update_aat_volume tx:", sig);
  console.log("Investor:", investorStr);
  console.log("New AAT volume:", newVolumeRaw);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
