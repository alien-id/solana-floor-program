import * as anchor from "@coral-xyz/anchor";
import { AnchorProvider, BN, Program } from "@coral-xyz/anchor";
import { PublicKey, SystemProgram, TransactionInstruction } from "@solana/web3.js";
import {
  TOKEN_PROGRAM_ID,
  ASSOCIATED_TOKEN_PROGRAM_ID,
  getAssociatedTokenAddressSync,
} from "@solana/spl-token";
import { FloorProgram } from "../target/types/floor_program";

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

  lobbyEntryPda(investor: PublicKey): [PublicKey, number] {
    return PublicKey.findProgramAddressSync(
      [Buffer.from("lobby_entry"), investor.toBuffer()],
      this.programId
    );
  }

  lockedWalnPda(investor: PublicKey, roundIndex: BN): [PublicKey, number] {
    const roundBuf = roundIndex.toArrayLike(Buffer, "le", 8);
    return PublicKey.findProgramAddressSync(
      [Buffer.from("locked_waln"), investor.toBuffer(), roundBuf],
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

    return this.program.methods
      .initialize(args.floorPriceUsdc, args.roundSizeWaln, args.lockPeriodSeconds)
      .accounts({
        admin: args.admin,
        contractState,
        usdcMint: args.usdcMint,
        walnMint: args.walnMint,
        usdcVault,
        walnVault,
        systemProgram: SystemProgram.programId,
        usdcTokenProgram: args.usdcTokenProgram ?? TOKEN_PROGRAM_ID,
        walnTokenProgram: args.walnTokenProgram ?? TOKEN_PROGRAM_ID,
      })
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
      })
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
    const [lobbyEntry] = this.lobbyEntryPda(args.investor);

    return this.program.methods
      .depositUsdc(args.usdcAmount)
      .accounts({
        investor: args.investor,
        contractState,
        lobbyEntry,
        usdcMint: args.usdcMint,
        investorUsdcAccount: args.investorUsdcAccount,
        usdcVault,
        aatNft: args.aatNft,
        usdcTokenProgram: args.usdcTokenProgram ?? TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
      })
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
    const [lobbyEntry] = this.lobbyEntryPda(args.investor);

    return this.program.methods
      .withdrawUsdc(args.amount)
      .accounts({
        investor: args.investor,
        contractState,
        lobbyEntry,
        usdcMint: args.usdcMint,
        investorUsdcAccount: args.investorUsdcAccount,
        usdcVault,
        usdcTokenProgram: args.usdcTokenProgram ?? TOKEN_PROGRAM_ID,
      })
      .instruction();
  }

  async sellWalnIx(args: {
    seller: PublicKey;
    sellerWalnAccount: PublicKey;
    sellerUsdcAccount: PublicKey;
    walnMint: PublicKey;
    usdcMint: PublicKey;
    walnAmount: BN;
    walnTokenProgram?: PublicKey;
    usdcTokenProgram?: PublicKey;
    roundTriggerAccounts?: { pubkey: PublicKey; isWritable: boolean }[];
  }): Promise<TransactionInstruction> {
    const [contractState] = this.contractStatePda();
    const [walnVault] = this.walnVaultPda();
    const [usdcVault] = this.usdcVaultPda();

    return this.program.methods
      .sellWaln(args.walnAmount)
      .accounts({
        seller: args.seller,
        contractState,
        walnMint: args.walnMint,
        usdcMint: args.usdcMint,
        sellerWalnAccount: args.sellerWalnAccount,
        sellerUsdcAccount: args.sellerUsdcAccount,
        walnVault,
        usdcVault,
        walnTokenProgram: args.walnTokenProgram ?? TOKEN_PROGRAM_ID,
        usdcTokenProgram: args.usdcTokenProgram ?? TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
      })
      .remainingAccounts(
        (args.roundTriggerAccounts ?? []).map((a) => ({
          pubkey: a.pubkey,
          isSigner: false,
          isWritable: a.isWritable,
        }))
      )
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
    const [lockedWaln] = this.lockedWalnPda(args.investor, args.roundIndex);

    return this.program.methods
      .claimWaln(args.roundIndex)
      .accounts({
        investor: args.investor,
        contractState,
        lockedWaln,
        walnMint: args.walnMint,
        investorWalnAccount: args.investorWalnAccount,
        walnVault,
        walnTokenProgram: args.walnTokenProgram ?? TOKEN_PROGRAM_ID,
      })
      .instruction();
  }

  admin(adminPubkey: PublicKey) {
    const [contractState] = this.contractStatePda();
    const accounts = { admin: adminPubkey, contractState };

    return {
      setFloorPrice: (newPriceUsdc: BN): Promise<TransactionInstruction> =>
        this.program.methods.setFloorPrice(newPriceUsdc).accounts(accounts).instruction(),
      setRoundSize: (newRoundSizeWaln: BN): Promise<TransactionInstruction> =>
        this.program.methods.setRoundSize(newRoundSizeWaln).accounts(accounts).instruction(),
      setLockPeriod: (newLockPeriod: BN): Promise<TransactionInstruction> =>
        this.program.methods.setLockPeriod(newLockPeriod).accounts(accounts).instruction(),
      setPaused: (paused: boolean): Promise<TransactionInstruction> =>
        this.program.methods.setPaused(paused).accounts(accounts).instruction(),
    };
  }
}
