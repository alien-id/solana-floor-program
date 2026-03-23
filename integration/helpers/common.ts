import { AnchorProvider, Wallet } from "@coral-xyz/anchor";
import { Keypair } from "@solana/web3.js";
import { FloorSdk } from "../../sdk/floor-sdk";
import fs from "fs";
import path from "path";

export const USDC_DECIMALS = 6;
export const WALN_DECIMALS = 9;

export const ALIEN_ID_HOOK_PROGRAM_ID = "AXmwHw9zuXBv5vNc28BoPfm8MS9gR3zbR5EN9nWiLMm8";
export const SAS_PROGRAM_ID = "22zoJMtdu4tQc2PzL74ZUT7FrwgB1Udec8DdW4yw4BdG";

export function loadKeypairFromEnv(): Keypair {
  const keypairPath =
    process.env.KEYPAIR_PATH ||
    process.env.ANCHOR_WALLET;
  if (!keypairPath) {
    throw new Error("Set KEYPAIR_PATH or ANCHOR_WALLET");
  }
  const resolved = path.resolve(keypairPath);
  const secret = JSON.parse(fs.readFileSync(resolved, "utf8"));
  return Keypair.fromSecretKey(Uint8Array.from(secret));
}

export function loadKeypairFromFile(filePath: string): Keypair {
  const resolved = path.resolve(filePath);
  const secret = JSON.parse(fs.readFileSync(resolved, "utf8"));
  return Keypair.fromSecretKey(Uint8Array.from(secret));
}

export function createProviderWithPayer(
  envProvider: AnchorProvider,
  payer: Keypair
): AnchorProvider {
  return new AnchorProvider(
    envProvider.connection,
    new Wallet(payer),
    envProvider.opts
  );
}

export function createSdk(provider: AnchorProvider): FloorSdk {
  return new FloorSdk(provider);
}
