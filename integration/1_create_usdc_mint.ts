import "dotenv/config";
import { AnchorProvider } from "@coral-xyz/anchor";
import {
  createMint,
  getOrCreateAssociatedTokenAccount,
  mintTo,
} from "@solana/spl-token";
import { loadKeypairFromEnv, USDC_DECIMALS } from "./helpers/common";

const MINT_AMOUNT = 1_000_000n * 10n ** BigInt(USDC_DECIMALS);

async function main() {
  const provider = AnchorProvider.env();
  const payer = loadKeypairFromEnv();

  const usdcMint = await createMint(
    provider.connection,
    payer,
    payer.publicKey,
    null,
    USDC_DECIMALS
  );

  console.log("Created USDC mint:", usdcMint.toBase58());

  const payerAta = await getOrCreateAssociatedTokenAccount(
    provider.connection,
    payer,
    usdcMint,
    payer.publicKey
  );

  await mintTo(
    provider.connection,
    payer,
    usdcMint,
    payerAta.address,
    payer,
    MINT_AMOUNT
  );

  console.log("Minted 1,000,000 USDC to:", payerAta.address.toBase58());
  console.log("USDC_MINT=" + usdcMint.toBase58());
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
