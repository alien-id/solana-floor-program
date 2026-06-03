import "dotenv/config";
import { AnchorProvider, BN } from "@coral-xyz/anchor";
import { PublicKey, Transaction } from "@solana/web3.js";
import { TOKEN_2022_PROGRAM_ID } from "@solana/spl-token";
import {
  loadKeypairFromEnv,
  loadKeypairFromFile,
  createProviderWithPayer,
  createSdk,
} from "./helpers/common";

async function main() {
  const usdcMintStr = process.env.USDC_MINT;
  const walnMintStr = process.env.WALN_MINT;
  if (!usdcMintStr) throw new Error("Set USDC_MINT");
  if (!walnMintStr) throw new Error("Set WALN_MINT");

  const floorPriceUsdc = new BN(process.env.FLOOR_PRICE_USDC ?? "100000");
  const roundSizeWaln = new BN(process.env.ROUND_SIZE_WALN ?? "200000000000");
  const lockPeriodSeconds = new BN(process.env.LOCK_PERIOD_SECONDS ?? "0");

  const envProvider = AnchorProvider.env();
  const payer = loadKeypairFromEnv();
  const provider = createProviderWithPayer(envProvider, payer);
  const sdk = createSdk(provider);

  const upgradeAuthorityPath = process.env.UPGRADE_AUTHORITY_KEYPAIR_PATH;
  const upgradeAuthority = upgradeAuthorityPath
    ? loadKeypairFromFile(upgradeAuthorityPath)
    : payer;

  const ix = await sdk.initializeIx({
    admin: upgradeAuthority.publicKey,
    usdcMint: new PublicKey(usdcMintStr),
    walnMint: new PublicKey(walnMintStr),
    floorPriceUsdc,
    roundSizeWaln,
    lockPeriodSeconds,
    walnTokenProgram: TOKEN_2022_PROGRAM_ID,
  });

  const signers = upgradeAuthority.publicKey.equals(payer.publicKey)
    ? [payer]
    : [payer, upgradeAuthority];

  const tx = new Transaction().add(ix);
  const sig = await provider.sendAndConfirm(tx, signers);
  console.log("initialize tx:", sig);
  console.log("admin (upgrade authority):", upgradeAuthority.publicKey.toBase58());

  const [contractState] = sdk.contractStatePda();
  console.log("Contract state PDA:", contractState.toBase58());
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
