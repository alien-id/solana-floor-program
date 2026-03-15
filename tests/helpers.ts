import * as anchor from "@coral-xyz/anchor";
import { AnchorProvider } from "@coral-xyz/anchor";
import { PublicKey } from "@solana/web3.js";
import { createMint, createAccount, mintTo, getAccount } from "@solana/spl-token";

export async function createTestMint(
  provider: AnchorProvider,
  decimals: number = 0
): Promise<PublicKey> {
  return createMint(
    provider.connection,
    (provider.wallet as anchor.Wallet).payer,
    provider.wallet.publicKey,
    null,
    decimals
  );
}

export async function createTestTokenAccount(
  provider: AnchorProvider,
  mint: PublicKey,
  owner: PublicKey
): Promise<PublicKey> {
  return createAccount(
    provider.connection,
    (provider.wallet as anchor.Wallet).payer,
    mint,
    owner
  );
}

export async function mintTokensTo(
  provider: AnchorProvider,
  mint: PublicKey,
  destination: PublicKey,
  amount: bigint
): Promise<void> {
  await mintTo(
    provider.connection,
    (provider.wallet as anchor.Wallet).payer,
    mint,
    destination,
    provider.wallet.publicKey,
    amount
  );
}

export async function getTokenBalance(
  provider: AnchorProvider,
  account: PublicKey
): Promise<bigint> {
  const info = await getAccount(provider.connection, account);
  return info.amount;
}

export async function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
