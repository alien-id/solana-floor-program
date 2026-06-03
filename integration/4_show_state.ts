import "dotenv/config";
import { AnchorProvider } from "@coral-xyz/anchor";
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

  const [contractState] = sdk.contractStatePda();
  const [usdcVault] = sdk.usdcVaultPda();
  const [walnVault] = sdk.walnVaultPda();
  const [investorPool] = sdk.investorPoolPda();

  const state = await sdk.program.account.programState.fetch(contractState);

  console.log("Contract State PDA:", contractState.toBase58());
  console.log("USDC Vault PDA:    ", usdcVault.toBase58());
  console.log("WALN Vault PDA:    ", walnVault.toBase58());
  console.log("Investor Pool PDA: ", investorPool.toBase58());
  console.log("State:", {
    admin: state.admin.toBase58(),
    usdcMint: state.usdcMint.toBase58(),
    walnMint: state.walnMint.toBase58(),
    floorPriceUsdc: state.floorPriceUsdc.toString(),
    roundSizeWaln: state.roundSizeWaln.toString(),
    lockPeriodSeconds: state.lockPeriodSeconds.toString(),
    sellPaused: state.sellPaused,
    roundStarted: state.roundStarted,
    roundCount: state.roundCount.toString(),
    currentRoundWaln: state.currentRoundWaln.toString(),
    totalUsdcInLobby: state.totalUsdcInLobby.toString(),
    totalAatVolume: state.totalAatVolume.toString(),
  });

  const pool = await sdk.fetchInvestorPool();
  console.log("Investor pool count:", pool.count);
  for (const inv of pool.investors) {
    console.log(" -", inv.investor.toBase58(), {
      usdcDeposited: inv.usdcDeposited.toString(),
      usdcLockedCurrentRound: inv.usdcLockedCurrentRound.toString(),
      usdcCommitted: inv.usdcCommitted.toString(),
      walnPurchasedTotal: inv.walnPurchasedTotal.toString(),
      aatVolume: inv.aatVolume.toString(),
    });
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
