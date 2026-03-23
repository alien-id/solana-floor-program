import "dotenv/config";
import { AnchorProvider, BN } from "@coral-xyz/anchor";
import { Transaction } from "@solana/web3.js";
import {
  loadKeypairFromEnv,
  createProviderWithPayer,
  createSdk,
} from "./helpers/common";

async function main() {
  const priceRaw = process.env.FLOOR_PRICE_USDC;
  if (!priceRaw) throw new Error("Set FLOOR_PRICE_USDC (in USDC base units, e.g. 100000 for 0.1 USDC)");

  const envProvider = AnchorProvider.env();
  const payer = loadKeypairFromEnv();
  const provider = createProviderWithPayer(envProvider, payer);
  const sdk = createSdk(provider);

  const ix = await sdk.admin(payer.publicKey).setFloorPrice(new BN(priceRaw));

  const tx = new Transaction().add(ix);
  const sig = await provider.sendAndConfirm(tx, [payer]);
  console.log("set_floor_price tx:", sig);
  console.log("New floor price:", priceRaw);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
