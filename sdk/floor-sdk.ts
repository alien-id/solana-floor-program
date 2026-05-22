import * as anchor from "@coral-xyz/anchor";
import { AnchorProvider, BN, Program } from "@coral-xyz/anchor";
import { PublicKey, SystemProgram, TransactionInstruction } from "@solana/web3.js";
import {
  TOKEN_PROGRAM_ID,
  ASSOCIATED_TOKEN_PROGRAM_ID,
  getAssociatedTokenAddressSync,
} from "@solana/spl-token";
import { FloorProgram } from "../target/types/floor_program";
import {buildHookAccountsWithBumps} from "./utils";

export interface RoundRecordData {
  roundIndex: bigint;
  triggeredAt: bigint;
  walnPurchased: bigint;
  usdcSpent: bigint;
  totalAatVolumeAtTrigger: bigint;
  participantCount: number;
  bump: number;
}

export interface InvestorRecordData {
  investor: PublicKey;
  usdcDeposited: BN;
  usdcLockedCurrentRound: BN;
  usdcCommitted: BN;
  walnPurchasedTotal: BN;
  aatVolume: BN;
  usdcUnlockTs: BN;
}

const TOKEN_2022_PROGRAM_ID = new PublicKey(
  "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"
);

export class FloorSdk {
  readonly provider: AnchorProvider;
  readonly program: Program<FloorProgram>;

  constructor(provider: AnchorProvider) {
    this.provider = provider;
    const idl = require("../target/idl/floor_program.json");
    this.program = new Program(idl, provider);
  }

  get programId(): PublicKey {
    return this.program.programId;
  }

  // ---------------------------------------------------------------------------
  // PDAs
  // ---------------------------------------------------------------------------

  contractStatePda(): [PublicKey, number] {
    return PublicKey.findProgramAddressSync(
      [Buffer.from("contract_state")],
      this.programId
    );
  }

  treasuryPda(): [PublicKey, number] {
    return PublicKey.findProgramAddressSync(
        [Buffer.from("treasury")],
        this.programId
    );
  }

  usdcVaultPda(): [PublicKey, number] {
    return PublicKey.findProgramAddressSync(
      [Buffer.from("usdc_vault")],
      this.programId
    );
  }

  walnVaultPda(): [PublicKey, number] {
    return PublicKey.findProgramAddressSync(
      [Buffer.from("waln_vault")],
      this.programId
    );
  }

  investorPoolPda(): [PublicKey, number] {
    return PublicKey.findProgramAddressSync(
      [Buffer.from("investor_pool")],
      this.programId
    );
  }

  /** @deprecated Use investorPoolPda() + fetchInvestorPool() instead */
  lobbyEntryPda(investor: PublicKey): [PublicKey, number] {
    return PublicKey.findProgramAddressSync(
      [Buffer.from("lobby_entry"), investor.toBuffer()],
      this.programId
    );
  }

  roundLockedWalnPda(roundIndex: BN): [PublicKey, number] {
    const roundBuf = roundIndex.toArrayLike(Buffer, "le", 8);
    return PublicKey.findProgramAddressSync(
      [Buffer.from("round_locked_waln"), roundBuf],
      this.programId
    );
  }

  roundRecordPda(roundIndex: BN): [PublicKey, number] {
    const roundBuf = roundIndex.toArrayLike(Buffer, "le", 8);
    return PublicKey.findProgramAddressSync(
      [Buffer.from("round_record"), roundBuf],
      this.programId
    );
  }

  /** Shared PDA that acts as mint authority for all AAT NFTs. */
  nftAuthorityPda(): [PublicKey, number] {
    return PublicKey.findProgramAddressSync(
      [Buffer.from("nft_authority")],
      this.programId
    );
  }

  /** PDA-derived Token-2022 mint for an investor's AAT NFT. */
  aatNftMintPda(investor: PublicKey): [PublicKey, number] {
    return PublicKey.findProgramAddressSync(
      [Buffer.from("aat_nft"), investor.toBuffer()],
      this.programId
    );
  }

  /** Investor's ATA for their AAT NFT (Token-2022). */
  investorAatAccount(investor: PublicKey, mint: PublicKey): PublicKey {
    return getAssociatedTokenAddressSync(
      mint,
      investor,
      false,
      TOKEN_2022_PROGRAM_ID
    );
  }

  // ---------------------------------------------------------------------------
  // Account fetchers
  // ---------------------------------------------------------------------------

  async fetchInvestorPool(): Promise<{ bump: number; count: number; investors: InvestorRecordData[] }> {
    const [investorPool] = this.investorPoolPda();
    const raw = await this.program.account.investorPool.fetch(investorPool) as any;
    return {
      bump: raw.bump,
      count: raw.investors.length,
      investors: raw.investors,
    };
  }

  async fetchInvestorRecord(investor: PublicKey): Promise<InvestorRecordData | null> {
    const pool = await this.fetchInvestorPool();
    return pool.investors.find((r) => r.investor.equals(investor)) ?? null;
  }

  // ---------------------------------------------------------------------------
  // Instructions
  // ---------------------------------------------------------------------------

  async initializeIx(args: {
    admin: PublicKey;
    usdcMint: PublicKey;
    walnMint: PublicKey;
    floorPriceUsdc: BN;
    roundSizeWaln: BN;
    lockPeriodSeconds: BN;
    usdcTokenProgram?: PublicKey;
    walnTokenProgram?: PublicKey;
  }): Promise<TransactionInstruction> {
    const [contractState] = this.contractStatePda();
    const [usdcVault] = this.usdcVaultPda();
    const [walnVault] = this.walnVaultPda();
    const [investorPool] = this.investorPoolPda();

    return this.program.methods
      .initialize(args.floorPriceUsdc, args.roundSizeWaln, args.lockPeriodSeconds)
      .accounts({
        admin: args.admin,
        contractState,
        usdcMint: args.usdcMint,
        walnMint: args.walnMint,
        usdcVault,
        walnVault,
        investorPool,
        systemProgram: SystemProgram.programId,
        usdcTokenProgram: args.usdcTokenProgram ?? TOKEN_PROGRAM_ID,
        walnTokenProgram: args.walnTokenProgram ?? TOKEN_PROGRAM_ID,
      } as any)
      .instruction();
  }

  async mintAatNftIx(args: {
    admin: PublicKey;
    investor: PublicKey;
    aatVolume: BN;
  }): Promise<TransactionInstruction> {
    const [contractState] = this.contractStatePda();
    const [nftAuthority] = this.nftAuthorityPda();
    const [mint] = this.aatNftMintPda(args.investor);
    const investorAatAccount = this.investorAatAccount(args.investor, mint);

    return this.program.methods
      .mintAatNft(args.aatVolume)
      .accounts({
        admin: args.admin,
        mint,
        contractState,
        investor: args.investor,
        investorAatAccount,
        nftAuthority,
        systemProgram: SystemProgram.programId,
        tokenProgram: TOKEN_2022_PROGRAM_ID,
        associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
      } as any)
      .instruction();
  }

  async updateAatVolumeIx(args: {
    admin: PublicKey;
    investor: PublicKey;
    newAatVolume: BN;
  }): Promise<TransactionInstruction> {
    const [contractState] = this.contractStatePda();
    const [investorPool] = this.investorPoolPda();
    const [nftAuthority] = this.nftAuthorityPda();
    const [mint] = this.aatNftMintPda(args.investor);

    return this.program.methods
      .updateAatVolume(args.newAatVolume)
      .accounts({
        admin: args.admin,
        contractState,
        investorPool,
        investor: args.investor,
        mint,
        nftAuthority,
        tokenProgram: TOKEN_2022_PROGRAM_ID,
      } as any)
      .instruction();
  }

  async depositUsdcIx(args: {
    investor: PublicKey;
    investorUsdcAccount: PublicKey;
    usdcMint: PublicKey;
    aatNft: PublicKey;
    usdcAmount: BN;
    usdcTokenProgram?: PublicKey;
  }): Promise<TransactionInstruction> {
    const [contractState] = this.contractStatePda();
    const [usdcVault] = this.usdcVaultPda();
    const [investorPool] = this.investorPoolPda();

    return this.program.methods
      .depositUsdc(args.usdcAmount)
      .accounts({
        investor: args.investor,
        contractState,
        investorPool,
        usdcMint: args.usdcMint,
        investorUsdcAccount: args.investorUsdcAccount,
        usdcVault,
        aatNft: args.aatNft,
        usdcTokenProgram: args.usdcTokenProgram ?? TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
      } as any)
      .instruction();
  }

  async withdrawUsdcIx(args: {
    investor: PublicKey;
    investorUsdcAccount: PublicKey;
    usdcMint: PublicKey;
    amount: BN;
    usdcTokenProgram?: PublicKey;
  }): Promise<TransactionInstruction> {
    const [contractState] = this.contractStatePda();
    const [usdcVault] = this.usdcVaultPda();
    const [investorPool] = this.investorPoolPda();

    return this.program.methods
      .withdrawUsdc(args.amount)
      .accounts({
        investor: args.investor,
        contractState,
        investorPool,
        usdcMint: args.usdcMint,
        investorUsdcAccount: args.investorUsdcAccount,
        usdcVault,
        usdcTokenProgram: args.usdcTokenProgram ?? TOKEN_PROGRAM_ID,
      } as any)
      .instruction();
  }

  async startRoundIx(args: {
    caller: PublicKey;
    roundIndex: BN;
  }): Promise<TransactionInstruction> {
    const [contractState] = this.contractStatePda();
    const [investorPool] = this.investorPoolPda();
    const [roundLockedWaln] = this.roundLockedWalnPda(args.roundIndex);
    const [treasury] = this.treasuryPda();
    return this.program.methods
      .startRound(args.roundIndex)
      .accounts({
        caller: args.caller,
        contractState,
        investorPool,
        roundLockedWaln,
        treasury,
        systemProgram: SystemProgram.programId,
      } as any)
      .instruction();
  }

  /**
   * Build a sellWaln instruction.
   *
   * roundTriggerAccounts: only the RoundRecord account is needed in remaining_accounts
   * when the sell closes the round. round_locked_waln is now a named account derived
   * from roundIndex.
   */
  async sellWalnIx(args: {
    seller: PublicKey;
    sellerWalnAccount: PublicKey;
    sellerUsdcAccount: PublicKey;
    walnMint: PublicKey;
    usdcMint: PublicKey;
    roundIndex: BN;
    walnAmount: BN;
    walnTokenProgram?: PublicKey;
    usdcTokenProgram?: PublicKey;
    roundTriggerAccounts?: { pubkey: PublicKey; isWritable: boolean }[];
  }): Promise<TransactionInstruction> {
    const [contractState] = this.contractStatePda();
    const [walnVault] = this.walnVaultPda();
    const [usdcVault] = this.usdcVaultPda();
    const [investorPool] = this.investorPoolPda();
    const [roundLockedWaln] = this.roundLockedWalnPda(args.roundIndex);
    const [treasury] = this.treasuryPda();
    const { accounts: hookAccounts, bumps: hookBumps } = await buildHookAccountsWithBumps(
      this.program.provider.connection,
      args.seller,
      args.walnMint,
    );
    const triggerAccounts = (args.roundTriggerAccounts ?? []).map((a) => ({
      pubkey: a.pubkey,
      isSigner: false,
      isWritable: a.isWritable,
    }));
    return this.program.methods
      .sellWaln(args.roundIndex, args.walnAmount, hookBumps)
      .accounts({
        seller: args.seller,
        contractState,
        investorPool,
        roundLockedWaln,
        walnMint: args.walnMint,
        usdcMint: args.usdcMint,
        sellerWalnAccount: args.sellerWalnAccount,
        sellerUsdcAccount: args.sellerUsdcAccount,
        walnVault,
        usdcVault,
        treasury,
        walnTokenProgram: args.walnTokenProgram ?? TOKEN_PROGRAM_ID,
        usdcTokenProgram: args.usdcTokenProgram ?? TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
      } as any)
      .remainingAccounts([...hookAccounts, ...triggerAccounts])
      .instruction();
  }

  async claimWalnIx(args: {
    investor: PublicKey;
    investorWalnAccount: PublicKey;
    walnMint: PublicKey;
    roundIndex: BN;
    walnTokenProgram?: PublicKey;
  }): Promise<TransactionInstruction> {
    const [contractState] = this.contractStatePda();
    const [walnVault] = this.walnVaultPda();
    const [roundLockedWaln] = this.roundLockedWalnPda(args.roundIndex);
    const [treasury] = this.treasuryPda();
    const { accounts: hookAccounts, bumps: hookBumps } = await buildHookAccountsWithBumps(
      this.program.provider.connection,
      contractState,
      args.walnMint,
    );

    return this.program.methods
      .claimWaln(args.roundIndex, hookBumps)
      .accounts({
        investor: args.investor,
        contractState,
        roundLockedWaln,
        treasury,
        walnMint: args.walnMint,
        investorWalnAccount: args.investorWalnAccount,
        walnVault,
        walnTokenProgram: args.walnTokenProgram ?? TOKEN_PROGRAM_ID,
      } as any)
      .remainingAccounts(hookAccounts)
      .instruction();
  }

  /**
   * Fetch the RoundLockedWaln account and return the InvestorAlloc for a given investor.
   * unlock is now a field on the parent struct (not per-alloc).
   * InvestorAlloc layout (40 bytes): 32 pubkey + 8 waln_amount. waln_amount == 0 means claimed.
   */
  async fetchInvestorAlloc(roundIndex: BN, investor: PublicKey): Promise<{
    investor: PublicKey;
    walnAmount: bigint;
    unlock: bigint;
  } | null> {
    const [roundLockedWalnPda] = this.roundLockedWalnPda(roundIndex);
    let rlw: any;
    try {
      rlw = await this.program.account.roundLockedWaln.fetch(roundLockedWalnPda);
    } catch {
      return null;
    }
    const alloc = (rlw.investors as any[]).find((a: any) =>
      (a.investor as PublicKey).equals(investor)
    );
    if (!alloc) return null;
    return {
      investor: alloc.investor as PublicKey,
      walnAmount: BigInt(alloc.walnAmount.toString()),
      unlock: BigInt((rlw.unlock as BN).toString()),
    };
  }

  admin(adminPubkey: PublicKey) {
    const [contractState] = this.contractStatePda();
    const [investorPool] = this.investorPoolPda();
    const accounts = { admin: adminPubkey, contractState };

    return {
      setFloorPrice: (newPriceUsdc: BN): Promise<TransactionInstruction> =>
        this.program.methods.setFloorPrice(newPriceUsdc).accounts(accounts as any).instruction(),
      setRoundSize: (newRoundSizeWaln: BN): Promise<TransactionInstruction> =>
        this.program.methods.setRoundSize(newRoundSizeWaln).accounts(accounts as any).instruction(),
      setLockPeriod: (newLockPeriod: BN): Promise<TransactionInstruction> =>
        this.program.methods.setLockPeriod(newLockPeriod).accounts(accounts as any).instruction(),
      setUsdcWithdrawLock: (newLockSeconds: BN): Promise<TransactionInstruction> =>
        this.program.methods.setUsdcWithdrawLock(newLockSeconds).accounts(accounts as any).instruction(),
      setInvestorUsdcUnlock: (investor: PublicKey, newUnlockTs: BN): Promise<TransactionInstruction> =>
        this.program.methods.setInvestorUsdcUnlock(investor, newUnlockTs).accounts({
          admin: adminPubkey,
          contractState,
          investorPool,
        } as any).instruction(),
      setPaused: (paused: boolean): Promise<TransactionInstruction> =>
        this.program.methods.setPaused(paused).accounts(accounts as any).instruction(),
      fundTreasury: (amount: BN): Promise<TransactionInstruction> =>
        this.program.methods.fundTreasury(amount).accounts(accounts as any).instruction(),
      cancelRound: (roundIndex: BN): Promise<TransactionInstruction> => {
        const [roundLockedWaln] = this.roundLockedWalnPda(roundIndex);
        const [treasury] = this.treasuryPda();
        return this.program.methods.cancelRound(roundIndex).accounts({
          admin: adminPubkey,
          contractState,
          investorPool,
          roundLockedWaln,
          treasury,
        } as any).instruction();
      },
      removeInvestorFromPool: (investor: PublicKey): Promise<TransactionInstruction> =>
        this.program.methods.removeInvestorFromPool(investor).accounts({
          admin: adminPubkey,
          contractState,
          investorPool,
        } as any).instruction(),
      transferAuthority: (newAdmin: PublicKey): Promise<TransactionInstruction> =>
        this.program.methods.transferAuthority().accounts({ admin: adminPubkey, newAdmin, contractState } as any).instruction(),
    };
  }

  async acceptAuthorityIx(pendingAdmin: PublicKey): Promise<TransactionInstruction> {
    const [contractState] = this.contractStatePda();
    return this.program.methods
      .acceptAuthority()
      .accounts({ pendingAdmin, contractState } as any)
      .instruction();
  }

  /**
   * Fetch and decode the RoundRecord account for a given round index.
   *
   * RoundRecord is created via raw CPI and is not an Anchor account type
   * in the IDL, so we decode the bytes manually here.
   */
  async fetchRoundRecord(roundIndex: BN): Promise<RoundRecordData> {
    const [roundRecordPda] = this.roundRecordPda(roundIndex);
    const info = await this.provider.connection.getAccountInfo(roundRecordPda);
    if (!info) {
      throw new Error(`RoundRecord account not found for roundIndex=${roundIndex.toString()}`);
    }

    // Account layout (all LE) after 8-byte discriminator:
    // offset 0: round_index (u64)
    // offset 8: triggered_at (i64)
    // offset 16: waln_purchased (u64)
    // offset 24: usdc_spent (u64)
    // offset 32: total_aat_volume_at_trigger (u64)
    // offset 40: participant_count (u32)
    // offset 44: bump (u8)
    const buf = Buffer.from(info.data).subarray(8);

    const roundIndexDecoded = buf.readBigUInt64LE(0);
    const triggeredAt = buf.readBigInt64LE(8);
    const walnPurchased = buf.readBigUInt64LE(16);
    const usdcSpent = buf.readBigUInt64LE(24);
    const totalAatVolumeAtTrigger = buf.readBigUInt64LE(32);
    const participantCount = buf.readUInt32LE(40);
    const bump = buf.readUInt8(44);

    return {
      roundIndex: roundIndexDecoded,
      triggeredAt,
      walnPurchased,
      usdcSpent,
      totalAatVolumeAtTrigger,
      participantCount,
      bump,
    };
  }
}
