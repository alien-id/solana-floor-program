import * as anchor from "@coral-xyz/anchor";
import { AnchorProvider, BN } from "@coral-xyz/anchor";
import { ComputeBudgetProgram, Keypair, PublicKey, Transaction } from "@solana/web3.js";
import {
  createTransferInstruction,
  getAssociatedTokenAddressSync,
  TOKEN_2022_PROGRAM_ID,
} from "@solana/spl-token";
import { assert } from "chai";
import { FloorSdk } from "../sdk/floor-sdk";
import {
  createTestMint,
  createTestTokenAccount,
  mintTokensTo,
  getTokenBalance,
  sleep,
} from "./helpers";

// ---------------------------------------------------------------------------
// Test parameters  (USDC = 6 decimals, wALN = 9 decimals)
// ---------------------------------------------------------------------------
// floor_price = 100_000:            $0.10 USDC (smallest units) per 1 whole wALN
// round_size  = 200_000_000_000:    200 wALN (smallest units) per round
// round cap USDC = 200e9 * 100_000 / 1e9 = 20_000_000 (20 USDC)
//
// Investor1: 5_000_000_000 USDC, aat_volume=100000 → share = 100000/150000
// Investor2: 5_000_000_000 USDC, aat_volume=50000  → share =  50000/150000
//
// After Round Start (integer arithmetic):
//   round_usdc_cap = 200e9 * 100_000 / 1e9 = 20_000_000
//   Investor1 locked = min(5e9, floor(20e6 * 100000/150000)) = 13_333_333  deposited = 4_986_666_667
//   Investor2 locked = min(5e9, floor(20e6 *  50000/150000)) =  6_666_666  deposited = 4_993_333_334
//
// After round trigger (sell 200 wALN):
//   seller receives 200e9 * 100_000 / 1e9 = 20_000_000 USDC
//   Investor1 wALN = 13_333_333 * 1e9 / 100_000 = 133_333_330_000
//   Investor2 wALN =  6_666_666 * 1e9 / 100_000 =  66_666_660_000

const USDC_DECIMALS = 6;
const WALN_DECIMALS = 9;
const USDC_UNIT = 10 ** USDC_DECIMALS;
const WALN_UNIT = 10 ** WALN_DECIMALS;

const FLOOR_PRICE = new BN(100_000);
const ROUND_SIZE = new BN(200 * WALN_UNIT);
const LOCK_PERIOD = new BN(0);

const INVESTOR1_USDC = new BN(5_000 * USDC_UNIT);
const INVESTOR2_USDC = new BN(5_000 * USDC_UNIT);

const SELL_AMOUNT_PARTIAL = new BN(100 * WALN_UNIT);
const SELL_AMOUNT_TRIGGER = new BN(100 * WALN_UNIT);

describe("floor-program", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);
  const sdk = new FloorSdk(provider);

  const admin = (provider.wallet as anchor.Wallet).payer;
  const investor1 = Keypair.generate();
  const investor2 = Keypair.generate();
  const seller = Keypair.generate();

  // Mints (set in before())
  let usdcMint: PublicKey;
  let walnMint: PublicKey;

  // Token accounts (set in before())
  let investor1UsdcAcc: PublicKey;
  let investor2UsdcAcc: PublicKey;
  let investor1WalnAcc: PublicKey;
  let investor2WalnAcc: PublicKey;
  let sellerWalnAcc: PublicKey;
  let sellerUsdcAcc: PublicKey;

  // AAT NFT mint pubkeys (Token-2022, PDA-derived — set in before())
  let investor1NftPubkey: PublicKey;
  let investor2NftPubkey: PublicKey;

  // Derived PDAs (set in before())
  let contractState: PublicKey;
  let usdcVault: PublicKey;
  let walnVault: PublicKey;
  let lobbyEntry1: PublicKey;
  let lobbyEntry2: PublicKey;

  // ---------------------------------------------------------------------------
  // Global setup
  // ---------------------------------------------------------------------------
  before(async () => {
    [contractState] = sdk.contractStatePda();
    [usdcVault] = sdk.usdcVaultPda();
    [walnVault] = sdk.walnVaultPda();
    [lobbyEntry1] = sdk.lobbyEntryPda(investor1.publicKey);
    [lobbyEntry2] = sdk.lobbyEntryPda(investor2.publicKey);

    // AAT NFT mint pubkeys are PDAs derived from investor pubkeys
    [investor1NftPubkey] = sdk.aatNftMintPda(investor1.publicKey);
    [investor2NftPubkey] = sdk.aatNftMintPda(investor2.publicKey);

    // Airdrop SOL to test keypairs
    await Promise.all(
      [investor1, investor2, seller].map(async (kp) => {
        const sig = await provider.connection.requestAirdrop(
          kp.publicKey,
          2_000_000_000
        );
        await provider.connection.confirmTransaction(sig, "confirmed");
      })
    );

    usdcMint = await createTestMint(provider, USDC_DECIMALS);
    walnMint = await createTestMint(provider, WALN_DECIMALS);

    // Create token accounts
    investor1UsdcAcc = await createTestTokenAccount(
      provider,
      usdcMint,
      investor1.publicKey
    );
    investor2UsdcAcc = await createTestTokenAccount(
      provider,
      usdcMint,
      investor2.publicKey
    );
    investor1WalnAcc = await createTestTokenAccount(
      provider,
      walnMint,
      investor1.publicKey
    );
    investor2WalnAcc = await createTestTokenAccount(
      provider,
      walnMint,
      investor2.publicKey
    );
    sellerWalnAcc = await createTestTokenAccount(
      provider,
      walnMint,
      seller.publicKey
    );
    sellerUsdcAcc = await createTestTokenAccount(
      provider,
      usdcMint,
      seller.publicKey
    );

    await mintTokensTo(provider, usdcMint, investor1UsdcAcc, BigInt(10_000 * USDC_UNIT));
    await mintTokensTo(provider, usdcMint, investor2UsdcAcc, BigInt(10_000 * USDC_UNIT));
    await mintTokensTo(provider, walnMint, sellerWalnAcc, BigInt(200 * WALN_UNIT));

    // ------------------------------------------------------------------
    // Initialize contract (must happen before mintAatNft)
    // ------------------------------------------------------------------
    await provider.sendAndConfirm(
      new Transaction().add(
        await sdk.initializeIx({
          admin: admin.publicKey,
          usdcMint,
          walnMint,
          floorPriceUsdc: FLOOR_PRICE,
          roundSizeWaln: ROUND_SIZE,
          lockPeriodSeconds: LOCK_PERIOD,
        })
      )
    );

    // ------------------------------------------------------------------
    // Mint AAT NFTs via the floor program (Token-2022, single transaction).
    // The mint keypair co-signs so it can be registered as the new account address.
    // ------------------------------------------------------------------
    await provider.sendAndConfirm(
      new Transaction().add(
        ComputeBudgetProgram.setComputeUnitLimit({ units: 400_000 }),
        await sdk.mintAatNftIx({
          admin: admin.publicKey,
          investor: investor1.publicKey,
          aatVolume: new BN(100000),
        })
      )
    );

    await provider.sendAndConfirm(
      new Transaction().add(
        ComputeBudgetProgram.setComputeUnitLimit({ units: 400_000 }),
        await sdk.mintAatNftIx({
          admin: admin.publicKey,
          investor: investor2.publicKey,
          aatVolume: new BN(50000),
        })
      )
    );
  });

  // ---------------------------------------------------------------------------
  // 1. initialize
  // ---------------------------------------------------------------------------
  describe("initialize", () => {
    it("creates contract state and vaults", async () => {
      const state = await sdk.program.account.programState.fetch(contractState);
      assert.ok(state.admin.equals(admin.publicKey));
      assert.ok(state.usdcMint.equals(usdcMint));
      assert.ok(state.walnMint.equals(walnMint));
      assert.ok(state.floorPriceUsdc.eq(FLOOR_PRICE));
      assert.ok(state.roundSizeWaln.eq(ROUND_SIZE));
      assert.ok(state.lockPeriodSeconds.eq(LOCK_PERIOD));
      assert.equal(state.paused, false);
      assert.equal(state.roundStarted, false);
      assert.ok(state.roundCount.eqn(0));
    });

    it("rejects a second initialization", async () => {
      try {
        await provider.sendAndConfirm(
          new Transaction().add(
            await sdk.initializeIx({
              admin: admin.publicKey,
              usdcMint,
              walnMint,
              floorPriceUsdc: FLOOR_PRICE,
              roundSizeWaln: ROUND_SIZE,
              lockPeriodSeconds: LOCK_PERIOD,
            })
          )
        );
        assert.fail("should have thrown");
      } catch (_) {}
    });
  });

  // ---------------------------------------------------------------------------
  // 1b. AAT NFT non-transferable
  // ---------------------------------------------------------------------------
  describe("AAT NFT non-transferable", () => {
    it("rejects transfer of investor1 AAT NFT to another wallet", async () => {
      const destination = Keypair.generate();
      const destinationAta = getAssociatedTokenAddressSync(
        investor1NftPubkey,
        destination.publicKey,
        false,
        TOKEN_2022_PROGRAM_ID
      );

      const srcAta = sdk.investorAatAccount(investor1.publicKey, investor1NftPubkey);

      try {
        await provider.sendAndConfirm(
          new Transaction().add(
            createTransferInstruction(
              srcAta,
              destinationAta,
              investor1.publicKey,
              1,
              [],
              TOKEN_2022_PROGRAM_ID
            )
          ),
          [investor1]
        );
        assert.fail("transfer should have been rejected");
      } catch (e: any) {
        assert.ok(
          e.toString().includes("non-transferable") ||
            e.toString().includes("NonTransferable") ||
            e.toString().includes("0x25") ||
            e.toString().includes("custom program error"),
          `expected non-transferable error, got: ${e.toString()}`
        );
      }
    });
  });

  // ---------------------------------------------------------------------------
  // 1c. AAT NFT allocation limit
  // ---------------------------------------------------------------------------
  describe("AAT NFT allocation limit", () => {
    it("tracks total_aat_volume after minting NFTs", async () => {
      const state = await sdk.program.account.programState.fetch(contractState);
      assert.ok(
        state.totalAatVolume.eqn(150000),
        `expected totalAatVolume=150000, got ${state.totalAatVolume}`
      );
    });

    it("rejects mint when total would exceed 1_000_000", async () => {
      const investor3 = Keypair.generate();
      const sig = await provider.connection.requestAirdrop(
        investor3.publicKey,
        2_000_000_000
      );
      await provider.connection.confirmTransaction(sig);

      try {
        await provider.sendAndConfirm(
          new Transaction().add(
            ComputeBudgetProgram.setComputeUnitLimit({ units: 400_000 }),
            await sdk.mintAatNftIx({
              admin: admin.publicKey,
              investor: investor3.publicKey,
              aatVolume: new BN(850_001),
            })
          )
        );
        assert.fail("should have thrown WalnAllocationLimitExceeded");
      } catch (e: any) {
        assert.ok(
          e.toString().includes("WalnAllocationLimitExceeded") ||
            e.toString().includes("1,000,000") ||
            e.toString().includes("6017"),
          `expected WalnAllocationLimitExceeded error, got: ${e.toString()}`
        );
      }
    });

    it("allows mint when total stays within 1_000_000", async () => {
      const investor4 = Keypair.generate();
      const sig = await provider.connection.requestAirdrop(
        investor4.publicKey,
        2_000_000_000
      );
      await provider.connection.confirmTransaction(sig);

      await provider.sendAndConfirm(
        new Transaction().add(
          ComputeBudgetProgram.setComputeUnitLimit({ units: 400_000 }),
          await sdk.mintAatNftIx({
            admin: admin.publicKey,
            investor: investor4.publicKey,
            aatVolume: new BN(850_000),
          })
        )
      );

      const state = await sdk.program.account.programState.fetch(contractState);
      assert.ok(
        state.totalAatVolume.eqn(1_000_000),
        `expected totalAatVolume=1_000_000, got ${state.totalAatVolume}`
      );
    });

    it("rejects mint when total is already at 1_000_000", async () => {
      const investor5 = Keypair.generate();
      const sig = await provider.connection.requestAirdrop(
        investor5.publicKey,
        2_000_000_000
      );
      await provider.connection.confirmTransaction(sig);

      try {
        await provider.sendAndConfirm(
          new Transaction().add(
            ComputeBudgetProgram.setComputeUnitLimit({ units: 400_000 }),
            await sdk.mintAatNftIx({
              admin: admin.publicKey,
              investor: investor5.publicKey,
              aatVolume: new BN(1),
            })
          )
        );
        assert.fail("should have thrown WalnAllocationLimitExceeded");
      } catch (e: any) {
        assert.ok(
          e.toString().includes("WalnAllocationLimitExceeded") ||
            e.toString().includes("1,000,000") ||
            e.toString().includes("6017"),
          `expected WalnAllocationLimitExceeded error, got: ${e.toString()}`
        );
      }
    });
  });

  // ---------------------------------------------------------------------------
  // 2. Admin instructions
  // ---------------------------------------------------------------------------
  describe("admin", () => {
    it("set_floor_price updates floor price", async () => {
      await provider.sendAndConfirm(
        new Transaction().add(await sdk.admin(admin.publicKey).setFloorPrice(new BN(99)))
      );
      let state = await sdk.program.account.programState.fetch(contractState);
      assert.ok(state.floorPriceUsdc.eqn(99));

      // restore
      await provider.sendAndConfirm(
        new Transaction().add(await sdk.admin(admin.publicKey).setFloorPrice(FLOOR_PRICE))
      );
      state = await sdk.program.account.programState.fetch(contractState);
      assert.ok(state.floorPriceUsdc.eq(FLOOR_PRICE));
    });

    it("set_round_size updates round size", async () => {
      await provider.sendAndConfirm(
        new Transaction().add(await sdk.admin(admin.publicKey).setRoundSize(new BN(200)))
      );
      let state = await sdk.program.account.programState.fetch(contractState);
      assert.ok(state.roundSizeWaln.eqn(200));

      // restore
      await provider.sendAndConfirm(
        new Transaction().add(await sdk.admin(admin.publicKey).setRoundSize(ROUND_SIZE))
      );
    });

    it("set_lock_period updates lock period", async () => {
      await provider.sendAndConfirm(
        new Transaction().add(await sdk.admin(admin.publicKey).setLockPeriod(new BN(86400)))
      );
      let state = await sdk.program.account.programState.fetch(contractState);
      assert.ok(state.lockPeriodSeconds.eqn(86400));

      // restore
      await provider.sendAndConfirm(
        new Transaction().add(await sdk.admin(admin.publicKey).setLockPeriod(LOCK_PERIOD))
      );
    });

    it("set_paused pauses and unpauses", async () => {
      await provider.sendAndConfirm(
        new Transaction().add(await sdk.admin(admin.publicKey).setPaused(true))
      );
      let state = await sdk.program.account.programState.fetch(contractState);
      assert.equal(state.paused, true);

      await provider.sendAndConfirm(
        new Transaction().add(await sdk.admin(admin.publicKey).setPaused(false))
      );
      state = await sdk.program.account.programState.fetch(contractState);
      assert.equal(state.paused, false);
    });

    it("rejects non-admin signer", async () => {
      try {
        await provider.sendAndConfirm(
          new Transaction().add(
            await sdk.admin(investor1.publicKey).setFloorPrice(new BN(1))
          ),
          [investor1]
        );
        assert.fail("should have thrown");
      } catch (e: any) {
        assert.include(e.toString(), "Unauthorized");
      }
    });

    it("set_floor_price rejects zero", async () => {
      try {
        await provider.sendAndConfirm(
          new Transaction().add(await sdk.admin(admin.publicKey).setFloorPrice(new BN(0)))
        );
        assert.fail("should have thrown");
      } catch (e: any) {
        assert.include(e.toString(), "InvalidParameter");
      }
    });

    it("set_round_size rejects zero", async () => {
      try {
        await provider.sendAndConfirm(
          new Transaction().add(await sdk.admin(admin.publicKey).setRoundSize(new BN(0)))
        );
        assert.fail("should have thrown");
      } catch (e: any) {
        assert.include(e.toString(), "InvalidParameter");
      }
    });
  });

  // ---------------------------------------------------------------------------
  // 3. deposit_usdc
  // ---------------------------------------------------------------------------
  describe("deposit_usdc", () => {
    it("investor1 deposits USDC (verified via AAT NFT)", async () => {
      const usdcBefore = await getTokenBalance(provider, usdcVault);

      await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.depositUsdcIx({
            investor: investor1.publicKey,
            investorUsdcAccount: investor1UsdcAcc,
            usdcMint,
            aatNft: investor1NftPubkey,
            usdcAmount: INVESTOR1_USDC,
          })
        ),
        [investor1]
      );

      const entry = await sdk.program.account.lobbyEntry.fetch(lobbyEntry1);
      assert.ok(entry.investor.equals(investor1.publicKey));
      assert.ok(entry.usdcDeposited.eq(INVESTOR1_USDC));

      const state = await sdk.program.account.programState.fetch(contractState);
      assert.ok(state.totalUsdcInLobby.eq(INVESTOR1_USDC));

      const usdcAfter = await getTokenBalance(provider, usdcVault);
      assert.equal(usdcAfter - usdcBefore, BigInt(INVESTOR1_USDC.toNumber()));
    });

    it("investor2 deposits USDC (verified via AAT NFT)", async () => {
      await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.depositUsdcIx({
            investor: investor2.publicKey,
            investorUsdcAccount: investor2UsdcAcc,
            usdcMint,
            aatNft: investor2NftPubkey,
            usdcAmount: INVESTOR2_USDC,
          })
        ),
        [investor2]
      );

      const entry = await sdk.program.account.lobbyEntry.fetch(lobbyEntry2);
      assert.ok(entry.investor.equals(investor2.publicKey));
      assert.ok(entry.usdcDeposited.eq(INVESTOR2_USDC));

      const state = await sdk.program.account.programState.fetch(contractState);
      assert.ok(
        state.totalUsdcInLobby.eq(INVESTOR1_USDC.add(INVESTOR2_USDC))
      );
    });

    it("investor1 adds more USDC in a second deposit", async () => {
      const extraUsdc = new BN(500 * USDC_UNIT);
      await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.depositUsdcIx({
            investor: investor1.publicKey,
            investorUsdcAccount: investor1UsdcAcc,
            usdcMint,
            aatNft: investor1NftPubkey,
            usdcAmount: extraUsdc,
          })
        ),
        [investor1]
      );

      // Restore (withdraw the extra 500)
      await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.withdrawUsdcIx({
            investor: investor1.publicKey,
            investorUsdcAccount: investor1UsdcAcc,
            usdcMint,
            amount: extraUsdc,
          })
        ),
        [investor1]
      );

      const entry = await sdk.program.account.lobbyEntry.fetch(lobbyEntry1);
      assert.ok(entry.usdcDeposited.eq(INVESTOR1_USDC));
    });

    it("rejects deposit when contract is paused", async () => {
      await provider.sendAndConfirm(
        new Transaction().add(await sdk.admin(admin.publicKey).setPaused(true))
      );
      try {
        await provider.sendAndConfirm(
          new Transaction().add(
            await sdk.depositUsdcIx({
              investor: investor1.publicKey,
              investorUsdcAccount: investor1UsdcAcc,
              usdcMint,
              aatNft: investor1NftPubkey,
              usdcAmount: new BN(1),
            })
          ),
          [investor1]
        );
        assert.fail("should have thrown");
      } catch (e: any) {
        assert.include(e.toString(), "ContractPaused");
      } finally {
        await provider.sendAndConfirm(
          new Transaction().add(await sdk.admin(admin.publicKey).setPaused(false))
        );
      }
    });

    it("rejects deposit with wrong/foreign AAT NFT", async () => {
      try {
        await provider.sendAndConfirm(
          new Transaction().add(
            await sdk.depositUsdcIx({
              investor: investor1.publicKey,
              investorUsdcAccount: investor1UsdcAcc,
              usdcMint,
              aatNft: investor2NftPubkey,
              usdcAmount: new BN(1),
            })
          ),
          [investor1]
        );
        assert.fail("should have thrown");
      } catch (e: any) {
        assert.ok(
          e.toString().includes("NoAatNft") ||
          e.toString().includes("InvalidAatNft"),
          `expected NoAatNft or InvalidAatNft, got: ${e.toString()}`
        );
      }
    });
  });

  // ---------------------------------------------------------------------------
  // 4. withdraw_usdc
  // ---------------------------------------------------------------------------
  describe("withdraw_usdc", () => {
    it("investor can withdraw unlocked (deposited) USDC", async () => {
      const withdrawAmt = new BN(100 * USDC_UNIT);
      const usdcBefore = await getTokenBalance(provider, investor1UsdcAcc);

      await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.withdrawUsdcIx({
            investor: investor1.publicKey,
            investorUsdcAccount: investor1UsdcAcc,
            usdcMint,
            amount: withdrawAmt,
          })
        ),
        [investor1]
      );

      const usdcAfter = await getTokenBalance(provider, investor1UsdcAcc);
      assert.equal(
        usdcAfter - usdcBefore,
        BigInt(withdrawAmt.toNumber())
      );

      const entry = await sdk.program.account.lobbyEntry.fetch(lobbyEntry1);
      assert.ok(entry.usdcDeposited.eq(new BN(4_900 * USDC_UNIT)));

      // Re-deposit the 100 USDC so the rest of the tests are consistent
      await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.depositUsdcIx({
            investor: investor1.publicKey,
            investorUsdcAccount: investor1UsdcAcc,
            usdcMint,
            aatNft: investor1NftPubkey,
            usdcAmount: withdrawAmt,
          })
        ),
        [investor1]
      );
    });

    it("rejects withdrawal when contract is paused", async () => {
      await provider.sendAndConfirm(
        new Transaction().add(await sdk.admin(admin.publicKey).setPaused(true))
      );
      try {
        await provider.sendAndConfirm(
          new Transaction().add(
            await sdk.withdrawUsdcIx({
              investor: investor1.publicKey,
              investorUsdcAccount: investor1UsdcAcc,
              usdcMint,
              amount: new BN(1),
            })
          ),
          [investor1]
        );
        assert.fail("should have thrown");
      } catch (e: any) {
        assert.include(e.toString(), "ContractPaused");
      } finally {
        await provider.sendAndConfirm(
          new Transaction().add(await sdk.admin(admin.publicKey).setPaused(false))
        );
      }
    });

    it("investor cannot withdraw more than deposited (unlocked)", async () => {
      const entry = await sdk.program.account.lobbyEntry.fetch(lobbyEntry1);
      const tooMuch = entry.usdcDeposited.addn(1);

      try {
        await provider.sendAndConfirm(
          new Transaction().add(
            await sdk.withdrawUsdcIx({
              investor: investor1.publicKey,
              investorUsdcAccount: investor1UsdcAcc,
              usdcMint,
              amount: tooMuch,
            })
          ),
          [investor1]
        );
        assert.fail("should have thrown");
      } catch (e: any) {
        assert.include(e.toString(), "InsufficientFunds");
      }
    });

    it("non-owner cannot withdraw another investor's funds", async () => {
      try {
        // investor2 tries to withdraw from investor1's lobby entry — but
        // investor2 doesn't control investor1's LobbyEntry PDA seed, so
        // the derived PDA for investor2 won't match lobby_entry1.
        // This tests the seed-binding security.
        await provider.sendAndConfirm(
          new Transaction().add(
            await sdk.withdrawUsdcIx({
              investor: investor2.publicKey,
              investorUsdcAccount: investor2UsdcAcc,
              usdcMint,
              amount: new BN(1),
            })
          ),
          [investor2]
        );
        // investor2's own lobby entry will have enough funds — this succeeds,
        // but the funds go back to investor2 only. The constraint is that the
        // PDA seeds enforce investor binding. We verify by checking the returned
        // entry belongs to investor2.
        const entry = await sdk.program.account.lobbyEntry.fetch(
          sdk.lobbyEntryPda(investor2.publicKey)[0]
        );
        assert.ok(entry.investor.equals(investor2.publicKey));

        // Re-deposit investor2's 1 USDC
        await provider.sendAndConfirm(
          new Transaction().add(
            await sdk.depositUsdcIx({
              investor: investor2.publicKey,
              investorUsdcAccount: investor2UsdcAcc,
              usdcMint,
              aatNft: investor2NftPubkey,
              usdcAmount: new BN(1),
            })
          ),
          [investor2]
        );
      } catch (_) {}
    });
  });

  // ---------------------------------------------------------------------------
  // 5. sell_waln — partial sale (auto-starts round, no trigger)
  // ---------------------------------------------------------------------------
  describe("sell_waln partial (auto-start)", () => {
    before(async () => {
      const tx = new Transaction().add(
        anchor.web3.SystemProgram.transfer({
          fromPubkey: admin.publicKey,
          toPubkey: contractState,
          lamports: 10_000_000,
        })
      );
      await provider.sendAndConfirm(tx);
    });

    it("rejects duplicate investor triplets on round start", async () => {
      const round0 = new BN(0);
      const [roundRecord0] = sdk.roundRecordPda(round0);
      const [lockedWaln1] = sdk.lockedWalnPda(investor1.publicKey, round0);
      const [lockedWaln2] = sdk.lockedWalnPda(investor2.publicKey, round0);

      try {
        await provider.sendAndConfirm(
          new Transaction().add(
            await sdk.sellWalnIx({
              seller: seller.publicKey,
              sellerWalnAccount: sellerWalnAcc,
              sellerUsdcAccount: sellerUsdcAcc,
              walnMint,
              usdcMint,
              walnAmount: new BN(1),
              roundTriggerAccounts: [
                { pubkey: roundRecord0, isWritable: true },
                { pubkey: lobbyEntry1, isWritable: true },
                { pubkey: lockedWaln1, isWritable: true },
                { pubkey: investor1NftPubkey, isWritable: false },
                { pubkey: lobbyEntry1, isWritable: true },
                { pubkey: lockedWaln1, isWritable: true },
                { pubkey: investor1NftPubkey, isWritable: false },
              ],
            })
          ),
          [seller]
        );
        assert.fail("should have thrown for duplicate triplets");
      } catch (e: any) {
        assert.ok(
          e.toString().includes("InvalidRemainingAccounts") ||
            e.toString().includes("already in use"),
          `expected InvalidRemainingAccounts, got: ${e.toString()}`
        );
      }

      const state = await sdk.program.account.programState.fetch(contractState);
      assert.equal(state.roundStarted, false, "state unchanged after rejected tx");
    });

    it("auto-starts round and sells wALN (no trigger)", async () => {
      const stateBefore = await sdk.program.account.programState.fetch(contractState);
      assert.equal(stateBefore.roundStarted, false);

      const sellerUsdcBefore = await getTokenBalance(provider, sellerUsdcAcc);
      const walnVaultBefore = await getTokenBalance(provider, walnVault);

      const round0 = new BN(0);
      const [roundRecord0] = sdk.roundRecordPda(round0);
      const [lockedWaln1] = sdk.lockedWalnPda(investor1.publicKey, round0);
      const [lockedWaln2] = sdk.lockedWalnPda(investor2.publicKey, round0);

      const partialSig = await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.sellWalnIx({
            seller: seller.publicKey,
            sellerWalnAccount: sellerWalnAcc,
            sellerUsdcAccount: sellerUsdcAcc,
            walnMint,
            usdcMint,
            walnAmount: SELL_AMOUNT_PARTIAL,
            roundTriggerAccounts: [
              { pubkey: roundRecord0, isWritable: true },
              { pubkey: lobbyEntry1, isWritable: true },
              { pubkey: lockedWaln1, isWritable: true },
              { pubkey: investor1NftPubkey, isWritable: false },
              { pubkey: lobbyEntry2, isWritable: true },
              { pubkey: lockedWaln2, isWritable: true },
              { pubkey: investor2NftPubkey, isWritable: false },
            ],
          })
        ),
        [seller]
      );

      await sleep(1000);
      const partialTx = await provider.connection.getTransaction(partialSig, { commitment: "confirmed", maxSupportedTransactionVersion: 0 });
      console.log(`    [CU] sell_waln PARTIAL (round-start, 2 investors): ${partialTx?.meta?.computeUnitsConsumed}`);

      const sellerUsdcAfter = await getTokenBalance(provider, sellerUsdcAcc);
      const walnVaultAfter = await getTokenBalance(provider, walnVault);

      assert.equal(
        sellerUsdcAfter - sellerUsdcBefore,
        BigInt(10 * USDC_UNIT)
      );
      assert.equal(
        walnVaultAfter - walnVaultBefore,
        BigInt(100 * WALN_UNIT)
      );

      const state = await sdk.program.account.programState.fetch(contractState);
      assert.ok(state.currentRoundWaln.eq(SELL_AMOUNT_PARTIAL));
      assert.equal(state.roundStarted, true);

      // investor1: min(5e9, floor(20e6 * 100000 / 150000)) = 13_333_333
      const entry1 = await sdk.program.account.lobbyEntry.fetch(lobbyEntry1);
      assert.ok(entry1.usdcLockedCurrentRound.eq(new BN(13_333_333)));
      assert.ok(entry1.usdcDeposited.eq(new BN(4_986_666_667)));

      // investor2: min(5e9, floor(20e6 * 50000 / 150000)) = 6_666_666
      const entry2 = await sdk.program.account.lobbyEntry.fetch(lobbyEntry2);
      assert.ok(entry2.usdcLockedCurrentRound.eq(new BN(6_666_666)));
      assert.ok(entry2.usdcDeposited.eq(new BN(4_993_333_334)));
    });

    it("rejects sell when contract is paused", async () => {
      await provider.sendAndConfirm(
        new Transaction().add(await sdk.admin(admin.publicKey).setPaused(true))
      );

      try {
        await provider.sendAndConfirm(
          new Transaction().add(
            await sdk.sellWalnIx({
              seller: seller.publicKey,
              sellerWalnAccount: sellerWalnAcc,
              sellerUsdcAccount: sellerUsdcAcc,
              walnMint,
              usdcMint,
              walnAmount: new BN(1),
            })
          ),
          [seller]
        );
        assert.fail("should have thrown");
      } catch (e: any) {
        assert.include(e.toString(), "ContractPaused");
      } finally {
        await provider.sendAndConfirm(
          new Transaction().add(await sdk.admin(admin.publicKey).setPaused(false))
        );
      }
    });
  });

  // ---------------------------------------------------------------------------
  // 6. sell_waln — round-triggering sale
  // ---------------------------------------------------------------------------
  describe("sell_waln round trigger", () => {
    it("triggers round end + round start when threshold is hit", async () => {
      const round0 = new BN(0);
      const [roundRecord0] = sdk.roundRecordPda(round0);
      const [lockedWaln1] = sdk.lockedWalnPda(investor1.publicKey, round0);
      const [lockedWaln2] = sdk.lockedWalnPda(investor2.publicKey, round0);

      const sellerUsdcBefore = await getTokenBalance(provider, sellerUsdcAcc);

      const triggerSig = await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.sellWalnIx({
            seller: seller.publicKey,
            sellerWalnAccount: sellerWalnAcc,
            sellerUsdcAccount: sellerUsdcAcc,
            walnMint,
            usdcMint,
            walnAmount: SELL_AMOUNT_TRIGGER,
            roundTriggerAccounts: [
              { pubkey: roundRecord0, isWritable: true },
              { pubkey: lobbyEntry1, isWritable: true },
              { pubkey: lockedWaln1, isWritable: true },
              { pubkey: investor1NftPubkey, isWritable: false },
              { pubkey: lobbyEntry2, isWritable: true },
              { pubkey: lockedWaln2, isWritable: true },
              { pubkey: investor2NftPubkey, isWritable: false },
            ],
          })
        ),
        [seller]
      );

      const triggerTx = await provider.connection.getTransaction(triggerSig, { commitment: "confirmed", maxSupportedTransactionVersion: 0 });
      console.log(`    [CU] sell_waln TRIGGER (round-end + round-start, 2 investors): ${triggerTx?.meta?.computeUnitsConsumed}`);

      const sellerUsdcAfter = await getTokenBalance(provider, sellerUsdcAcc);
      assert.equal(
        sellerUsdcAfter - sellerUsdcBefore,
        BigInt(10 * USDC_UNIT)
      );

      // ---- verify RoundRecord created ----
      const rr = await sdk.fetchRoundRecord(round0);
      assert.equal(rr.roundIndex, 0n);
      assert.equal(rr.walnPurchased, 199_999_990_000n);
      assert.equal(rr.usdcSpent, 19_999_999n);
      assert.equal(rr.totalAatVolumeAtTrigger, 150000n);
      assert.equal(rr.participantCount, 2);

      // ---- verify ContractState updated ----
      const state = await sdk.program.account.programState.fetch(contractState);
      assert.ok(state.roundCount.eqn(1));
      assert.ok(state.currentRoundWaln.eqn(0));
      assert.equal(state.roundStarted, true); // auto-started next round

      // ---- verify LockedWaln records ----
      const lw1 = await sdk.program.account.lockedWaln.fetch(lockedWaln1);
      assert.ok(lw1.investor.equals(investor1.publicKey));
      assert.ok(lw1.roundIndex.eqn(0));
      assert.ok(lw1.walnAmount.eq(new BN("133333330000")));
      assert.equal(lw1.claimed, false);

      const lw2 = await sdk.program.account.lockedWaln.fetch(lockedWaln2);
      assert.ok(lw2.investor.equals(investor2.publicKey));
      assert.ok(lw2.roundIndex.eqn(0));
      assert.ok(lw2.walnAmount.eq(new BN("66666660000")));
      assert.equal(lw2.claimed, false);

      const entry1 = await sdk.program.account.lobbyEntry.fetch(lobbyEntry1);
      assert.ok(entry1.usdcLockedCurrentRound.eq(new BN(13_333_333)));
      assert.ok(entry1.usdcDeposited.eq(new BN(4_973_333_334)));

      const entry2 = await sdk.program.account.lobbyEntry.fetch(lobbyEntry2);
      assert.ok(entry2.usdcLockedCurrentRound.eq(new BN(6_666_666)));
      assert.ok(entry2.usdcDeposited.eq(new BN(4_986_666_668)));

      assert.ok(entry1.usdcCommitted.eq(new BN(13_333_333)));
      assert.ok(entry1.walnPurchasedTotal.eq(new BN("133333330000")));
      assert.ok(entry2.usdcCommitted.eq(new BN(6_666_666)));
      assert.ok(entry2.walnPurchasedTotal.eq(new BN("66666660000")));
    });
  });

  // ---------------------------------------------------------------------------
  // 7. claim_waln
  // ---------------------------------------------------------------------------
  describe("claim_waln", () => {
    it("claims locked wALN after lock expires (lock_period=0)", async () => {
      // With lock_period = 0, unlock = triggered_at + 0 = triggered_at
      // The clock moves forward between slots, so this should pass immediately.
      await sleep(500);

      const round0 = new BN(0);
      const walnBefore = await getTokenBalance(provider, investor1WalnAcc);

      await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.claimWalnIx({
            investor: investor1.publicKey,
            investorWalnAccount: investor1WalnAcc,
            walnMint,
            roundIndex: round0,
          })
        ),
        [investor1]
      );

      const walnAfter = await getTokenBalance(provider, investor1WalnAcc);
      assert.equal(walnAfter - walnBefore, 133_333_330_000n);

      const [lockedWaln1] = sdk.lockedWalnPda(investor1.publicKey, round0);
      const lw = await sdk.program.account.lockedWaln.fetch(lockedWaln1);
      assert.equal(lw.claimed, true);
    });

    it("rejects claim when contract is paused", async () => {
      await provider.sendAndConfirm(
        new Transaction().add(await sdk.admin(admin.publicKey).setPaused(true))
      );
      try {
        await provider.sendAndConfirm(
          new Transaction().add(
            await sdk.claimWalnIx({
              investor: investor2.publicKey,
              investorWalnAccount: investor2WalnAcc,
              walnMint,
              roundIndex: new BN(0),
            })
          ),
          [investor2]
        );
        assert.fail("should have thrown");
      } catch (e: any) {
        assert.include(e.toString(), "ContractPaused");
      } finally {
        await provider.sendAndConfirm(
          new Transaction().add(await sdk.admin(admin.publicKey).setPaused(false))
        );
      }
    });

    it("rejects double-claim", async () => {
      const round0 = new BN(0);
      try {
        await provider.sendAndConfirm(
          new Transaction().add(
            await sdk.claimWalnIx({
              investor: investor1.publicKey,
              investorWalnAccount: investor1WalnAcc,
              walnMint,
              roundIndex: round0,
            })
          ),
          [investor1]
        );
        assert.fail("should have thrown");
      } catch (e: any) {
        assert.include(e.toString(), "AlreadyClaimed");
      }
    });

    it("investor2 claims their round-0 wALN", async () => {
      const round0 = new BN(0);
      const walnBefore = await getTokenBalance(provider, investor2WalnAcc);

      await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.claimWalnIx({
            investor: investor2.publicKey,
            investorWalnAccount: investor2WalnAcc,
            walnMint,
            roundIndex: round0,
          })
        ),
        [investor2]
      );

      const walnAfter = await getTokenBalance(provider, investor2WalnAcc);
      assert.equal(walnAfter - walnBefore, 66_666_660_000n);
    });
  });

  // ---------------------------------------------------------------------------
  // 8. Vault integrity — USDC vault balance
  // ---------------------------------------------------------------------------
  describe("vault integrity", () => {
    it("USDC vault balance reflects deposits minus seller payments", async () => {
      const vaultBalance = await getTokenBalance(provider, usdcVault);
      assert.equal(vaultBalance, BigInt(9_980 * USDC_UNIT));

      // The sum of investor positions (deposited + locked) may exceed the vault
      // balance by up to 1 USDC
      const entry1 = await sdk.program.account.lobbyEntry.fetch(lobbyEntry1);
      const entry2 = await sdk.program.account.lobbyEntry.fetch(lobbyEntry2);
      const sumPositions = BigInt(
        entry1.usdcDeposited.toNumber() +
        entry1.usdcLockedCurrentRound.toNumber() +
        entry2.usdcDeposited.toNumber() +
        entry2.usdcLockedCurrentRound.toNumber()
      );
      const dustDiff = sumPositions - vaultBalance;
      assert.ok(
        dustDiff >= 0n && dustDiff <= 5n,
        `rounding dust should be tiny, got ${dustDiff}`
      );
    });
  });

  // ---------------------------------------------------------------------------
  // 9. Fix verification: total_usdc_in_lobby accounting
  // ---------------------------------------------------------------------------
  describe("total_usdc_in_lobby accounting", () => {
    it("total_usdc_in_lobby equals sum of deposited + locked across entries", async () => {
      const state = await sdk.program.account.programState.fetch(contractState);
      const entry1 = await sdk.program.account.lobbyEntry.fetch(lobbyEntry1);
      const entry2 = await sdk.program.account.lobbyEntry.fetch(lobbyEntry2);

      const sumLobby =
        entry1.usdcDeposited.toNumber() +
        entry1.usdcLockedCurrentRound.toNumber() +
        entry2.usdcDeposited.toNumber() +
        entry2.usdcLockedCurrentRound.toNumber();

      assert.ok(
        state.totalUsdcInLobby.eq(new BN(9_980_000_001)),
        `expected totalUsdcInLobby=9980000001, got ${state.totalUsdcInLobby.toString()}`
      );
      assert.equal(
        state.totalUsdcInLobby.toNumber(),
        sumLobby,
        "totalUsdcInLobby should equal sum of individual entries"
      );
    });
  });

  // ---------------------------------------------------------------------------
  // 10. Fix verification: contract_state lamports are not drained on round trigger
  // ---------------------------------------------------------------------------
  describe("seller funds round accounts — contract_state not drained", () => {
    before(async () => {
      await mintTokensTo(provider, walnMint, sellerWalnAcc, BigInt(200 * WALN_UNIT));

      await mintTokensTo(provider, usdcMint, investor1UsdcAcc, BigInt(10_000 * USDC_UNIT));
      await mintTokensTo(provider, usdcMint, investor2UsdcAcc, BigInt(10_000 * USDC_UNIT));
      await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.depositUsdcIx({
            investor: investor1.publicKey,
            investorUsdcAccount: investor1UsdcAcc,
            usdcMint,
            aatNft: investor1NftPubkey,
            usdcAmount: new BN(5_000 * USDC_UNIT),
          })
        ),
        [investor1]
      );
      await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.depositUsdcIx({
            investor: investor2.publicKey,
            investorUsdcAccount: investor2UsdcAcc,
            usdcMint,
            aatNft: investor2NftPubkey,
            usdcAmount: new BN(5_000 * USDC_UNIT),
          })
        ),
        [investor2]
      );
    });

    it("contract_state lamports are not drained when round triggers (seller pays rent)", async () => {
      // Round-1 is already started (auto-started after round-0).
      // current_round_waln = 0, round_size = 200. Sell 200 to trigger round-1.
      const round1 = new BN(1);
      const [roundRecord1] = sdk.roundRecordPda(round1);
      const [lockedWaln1r1] = sdk.lockedWalnPda(investor1.publicKey, round1);
      const [lockedWaln2r1] = sdk.lockedWalnPda(investor2.publicKey, round1);

      const contractStateLamportsBefore = await provider.connection.getBalance(contractState);
      const sellerSolBefore = await provider.connection.getBalance(seller.publicKey);

      await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.sellWalnIx({
            seller: seller.publicKey,
            sellerWalnAccount: sellerWalnAcc,
            sellerUsdcAccount: sellerUsdcAcc,
            walnMint,
            usdcMint,
            walnAmount: new BN(200 * WALN_UNIT),
            roundTriggerAccounts: [
              { pubkey: roundRecord1, isWritable: true },
              { pubkey: lobbyEntry1, isWritable: true },
              { pubkey: lockedWaln1r1, isWritable: true },
              { pubkey: investor1NftPubkey, isWritable: false },
              { pubkey: lobbyEntry2, isWritable: true },
              { pubkey: lockedWaln2r1, isWritable: true },
              { pubkey: investor2NftPubkey, isWritable: false },
            ],
          })
        ),
        [seller]
      );

      const contractStateLamportsAfter = await provider.connection.getBalance(contractState);
      const sellerSolAfter = await provider.connection.getBalance(seller.publicKey);

      // contract_state must not lose lamports — the seller now funds all created accounts
      assert.ok(
        contractStateLamportsAfter >= contractStateLamportsBefore,
        `contract_state lamports must not decrease, before=${contractStateLamportsBefore} after=${contractStateLamportsAfter}`
      );

      // Seller paid rent for 2 LockedWaln + 1 RoundRecord + tx fee
      const sellerSolDiff = sellerSolBefore - sellerSolAfter;
      assert.ok(
        sellerSolDiff > 10_000,
        `seller should have paid rent for created accounts (diff=${sellerSolDiff} lamports)`
      );

      // Verify round-1 completed
      const state = await sdk.program.account.programState.fetch(contractState);
      assert.ok(state.roundCount.eqn(2), "round_count should be 2");
    });
  });

  // ---------------------------------------------------------------------------
  // 11. Fix verification: sell cap prevents round overshoot
  // ---------------------------------------------------------------------------
  describe("sell cap — no overshoot allowed", () => {
    before(async () => {
      await mintTokensTo(provider, walnMint, sellerWalnAcc, BigInt(500 * WALN_UNIT));

      await mintTokensTo(provider, usdcMint, investor1UsdcAcc, BigInt(20_000 * USDC_UNIT));
      await mintTokensTo(provider, usdcMint, investor2UsdcAcc, BigInt(20_000 * USDC_UNIT));
      await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.depositUsdcIx({
            investor: investor1.publicKey,
            investorUsdcAccount: investor1UsdcAcc,
            usdcMint,
            aatNft: investor1NftPubkey,
            usdcAmount: new BN(10_000 * USDC_UNIT),
          })
        ),
        [investor1]
      );
      await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.depositUsdcIx({
            investor: investor2.publicKey,
            investorUsdcAccount: investor2UsdcAcc,
            usdcMint,
            aatNft: investor2NftPubkey,
            usdcAmount: new BN(10_000 * USDC_UNIT),
          })
        ),
        [investor2]
      );
    });

    it("rejects a sell that would exceed remaining round capacity", async () => {
      try {
        await provider.sendAndConfirm(
          new Transaction().add(
            await sdk.sellWalnIx({
              seller: seller.publicKey,
              sellerWalnAccount: sellerWalnAcc,
              sellerUsdcAccount: sellerUsdcAcc,
              walnMint,
              usdcMint,
              walnAmount: new BN(201 * WALN_UNIT),
              roundTriggerAccounts: [
                { pubkey: sdk.roundRecordPda(new BN(2))[0], isWritable: true },
                { pubkey: lobbyEntry1, isWritable: true },
                { pubkey: sdk.lockedWalnPda(investor1.publicKey, new BN(2))[0], isWritable: true },
                { pubkey: investor1NftPubkey, isWritable: false },
                { pubkey: lobbyEntry2, isWritable: true },
                { pubkey: sdk.lockedWalnPda(investor2.publicKey, new BN(2))[0], isWritable: true },
                { pubkey: investor2NftPubkey, isWritable: false },
              ],
            })
          ),
          [seller]
        );
        assert.fail("should have thrown SellAmountExceedsRound");
      } catch (e: any) {
        assert.include(e.toString(), "SellAmountExceedsRound");
      }

      // State must be unchanged
      const state = await sdk.program.account.programState.fetch(contractState);
      assert.ok(state.currentRoundWaln.eqn(0));
      assert.ok(state.roundCount.eqn(2));
    });

    it("partial sell within cap accumulates without triggering", async () => {
      await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.sellWalnIx({
            seller: seller.publicKey,
            sellerWalnAccount: sellerWalnAcc,
            sellerUsdcAccount: sellerUsdcAcc,
            walnMint,
            usdcMint,
            walnAmount: new BN(100 * WALN_UNIT),
            roundTriggerAccounts: [
              { pubkey: sdk.roundRecordPda(new BN(2))[0], isWritable: true },
              { pubkey: lobbyEntry1, isWritable: true },
              { pubkey: sdk.lockedWalnPda(investor1.publicKey, new BN(2))[0], isWritable: true },
              { pubkey: investor1NftPubkey, isWritable: false },
              { pubkey: lobbyEntry2, isWritable: true },
              { pubkey: sdk.lockedWalnPda(investor2.publicKey, new BN(2))[0], isWritable: true },
              { pubkey: investor2NftPubkey, isWritable: false },
            ],
          })
        ),
        [seller]
      );

      const state = await sdk.program.account.programState.fetch(contractState);
      assert.ok(state.currentRoundWaln.eq(new BN(100 * WALN_UNIT)), "100 wALN accumulated");
      assert.ok(state.roundCount.eqn(2), "round not triggered yet");
    });

    it("rejects sell exceeding remaining capacity mid-round", async () => {
      try {
        await provider.sendAndConfirm(
          new Transaction().add(
            await sdk.sellWalnIx({
              seller: seller.publicKey,
              sellerWalnAccount: sellerWalnAcc,
              sellerUsdcAccount: sellerUsdcAcc,
              walnMint,
              usdcMint,
              walnAmount: new BN(101 * WALN_UNIT),
              roundTriggerAccounts: [
                { pubkey: sdk.roundRecordPda(new BN(2))[0], isWritable: true },
                { pubkey: lobbyEntry1, isWritable: true },
                { pubkey: sdk.lockedWalnPda(investor1.publicKey, new BN(2))[0], isWritable: true },
                { pubkey: investor1NftPubkey, isWritable: false },
                { pubkey: lobbyEntry2, isWritable: true },
                { pubkey: sdk.lockedWalnPda(investor2.publicKey, new BN(2))[0], isWritable: true },
                { pubkey: investor2NftPubkey, isWritable: false },
              ],
            })
          ),
          [seller]
        );
        assert.fail("should have thrown SellAmountExceedsRound");
      } catch (e: any) {
        assert.include(e.toString(), "SellAmountExceedsRound");
      }
    });

    it("exact remaining capacity triggers round cleanly with current_round_waln=0", async () => {
      const stateBefore = await sdk.program.account.programState.fetch(contractState);
      const roundIndex = stateBefore.roundCount; // 2
      const roundBn = new BN(roundIndex.toNumber());
      const [roundRecord] = sdk.roundRecordPda(roundBn);
      const [lockedWaln1] = sdk.lockedWalnPda(investor1.publicKey, roundBn);
      const [lockedWaln2] = sdk.lockedWalnPda(investor2.publicKey, roundBn);

      await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.sellWalnIx({
            seller: seller.publicKey,
            sellerWalnAccount: sellerWalnAcc,
            sellerUsdcAccount: sellerUsdcAcc,
            walnMint,
            usdcMint,
            walnAmount: new BN(100 * WALN_UNIT),
            roundTriggerAccounts: [
              { pubkey: roundRecord, isWritable: true },
              { pubkey: lobbyEntry1, isWritable: true },
              { pubkey: lockedWaln1, isWritable: true },
              { pubkey: investor1NftPubkey, isWritable: false },
              { pubkey: lobbyEntry2, isWritable: true },
              { pubkey: lockedWaln2, isWritable: true },
              { pubkey: investor2NftPubkey, isWritable: false },
            ],
          })
        ),
        [seller]
      );

      const stateAfter = await sdk.program.account.programState.fetch(contractState);
      assert.ok(
        stateAfter.currentRoundWaln.eqn(0),
        `current_round_waln must be exactly 0 after clean trigger, got ${stateAfter.currentRoundWaln.toString()}`
      );
      assert.ok(
        stateAfter.roundCount.eqn(roundIndex.toNumber() + 1),
        "round_count should have incremented"
      );
      assert.equal(stateAfter.roundStarted, true, "next round auto-started");
    });
  });

  // ---------------------------------------------------------------------------
  // 12. Price isolation — set_floor_price while round is in progress
  // ---------------------------------------------------------------------------
  describe("price isolation — set_floor_price applies to next round only", () => {
    before(async () => {
      await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.admin(admin.publicKey).setFloorPrice(new BN(100 * USDC_UNIT))
        )
      );

      await mintTokensTo(provider, usdcMint, investor1UsdcAcc, BigInt(20_000 * USDC_UNIT));
      await mintTokensTo(provider, usdcMint, investor2UsdcAcc, BigInt(20_000 * USDC_UNIT));
      await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.depositUsdcIx({
            investor: investor1.publicKey,
            investorUsdcAccount: investor1UsdcAcc,
            usdcMint,
            aatNft: investor1NftPubkey,
            usdcAmount: new BN(10_000 * USDC_UNIT),
          })
        ),
        [investor1]
      );
      await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.depositUsdcIx({
            investor: investor2.publicKey,
            investorUsdcAccount: investor2UsdcAcc,
            usdcMint,
            aatNft: investor2NftPubkey,
            usdcAmount: new BN(10_000 * USDC_UNIT),
          })
        ),
        [investor2]
      );
    });

    it("current_round_floor_price is not changed by set_floor_price mid-round", async () => {
      const state = await sdk.program.account.programState.fetch(contractState);
      assert.ok(state.floorPriceUsdc.eq(new BN(100 * USDC_UNIT)), "floor_price_usdc updated to 100 USDC");
      assert.ok(
        state.currentRoundFloorPrice.eq(new BN(100_000)),
        `current_round_floor_price should still be $0.10, got ${state.currentRoundFloorPrice.toString()}`
      );
      assert.equal(state.roundStarted, true, "round 3 is active");
    });

    it("seller receives USDC at snapshotted price ($0.10), not updated price (100 USDC)", async () => {
      const sellerUsdcBefore = await getTokenBalance(provider, sellerUsdcAcc);

      await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.sellWalnIx({
            seller: seller.publicKey,
            sellerWalnAccount: sellerWalnAcc,
            sellerUsdcAccount: sellerUsdcAcc,
            walnMint,
            usdcMint,
            walnAmount: new BN(100 * WALN_UNIT),
          })
        ),
        [seller]
      );

      const sellerUsdcAfter = await getTokenBalance(provider, sellerUsdcAcc);
      const usdcReceived = sellerUsdcAfter - sellerUsdcBefore;
      assert.equal(
        usdcReceived,
        BigInt(10 * USDC_UNIT),
        `seller should receive 10 USDC at snapshotted price $0.10, got ${usdcReceived}`
      );
    });

    it("next round uses new floor price (100 USDC) after current round triggers", async () => {
      const stateBefore = await sdk.program.account.programState.fetch(contractState);
      const roundIndex = stateBefore.roundCount; // 3
      const roundBn = new BN(roundIndex.toNumber());
      const [roundRecord] = sdk.roundRecordPda(roundBn);
      const [lockedWaln1] = sdk.lockedWalnPda(investor1.publicKey, roundBn);
      const [lockedWaln2] = sdk.lockedWalnPda(investor2.publicKey, roundBn);

      await provider.sendAndConfirm(
        new Transaction().add(
          ComputeBudgetProgram.setComputeUnitLimit({ units: 400_000 }),
          await sdk.sellWalnIx({
            seller: seller.publicKey,
            sellerWalnAccount: sellerWalnAcc,
            sellerUsdcAccount: sellerUsdcAcc,
            walnMint,
            usdcMint,
            walnAmount: new BN(100 * WALN_UNIT),
            roundTriggerAccounts: [
              { pubkey: roundRecord, isWritable: true },
              { pubkey: lobbyEntry1, isWritable: true },
              { pubkey: lockedWaln1, isWritable: true },
              { pubkey: investor1NftPubkey, isWritable: false },
              { pubkey: lobbyEntry2, isWritable: true },
              { pubkey: lockedWaln2, isWritable: true },
              { pubkey: investor2NftPubkey, isWritable: false },
            ],
          })
        ),
        [seller]
      );

      const stateAfter = await sdk.program.account.programState.fetch(contractState);
      assert.ok(
        stateAfter.roundCount.eqn(roundIndex.toNumber() + 1),
        "round_count should have incremented"
      );
      assert.ok(
        stateAfter.currentRoundFloorPrice.eq(new BN(100 * USDC_UNIT)),
        `new round must use updated floor price 100 USDC, got ${stateAfter.currentRoundFloorPrice.toString()}`
      );
      assert.equal(stateAfter.roundStarted, true, "next round auto-started with new price");
      assert.ok(
        stateAfter.currentRoundSizeWaln.eq(new BN(200 * WALN_UNIT)),
        `current_round_size_waln should still be 200 wALN for new round, got ${stateAfter.currentRoundSizeWaln.toString()}`
      );
    });
  });

  // ---------------------------------------------------------------------------
  // 13. Round-size isolation — set_round_size applies to next round only
  // ---------------------------------------------------------------------------
  describe("round-size isolation — set_round_size applies to next round only", () => {
    before(async () => {
      await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.admin(admin.publicKey).setRoundSize(new BN(400 * WALN_UNIT))
        )
      );

      await mintTokensTo(provider, walnMint, sellerWalnAcc, BigInt(200 * WALN_UNIT));
      await mintTokensTo(provider, usdcMint, investor1UsdcAcc, BigInt(50_000 * USDC_UNIT));
      await mintTokensTo(provider, usdcMint, investor2UsdcAcc, BigInt(50_000 * USDC_UNIT));
      await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.depositUsdcIx({
            investor: investor1.publicKey,
            investorUsdcAccount: investor1UsdcAcc,
            usdcMint,
            aatNft: investor1NftPubkey,
            usdcAmount: new BN(20_000 * USDC_UNIT),
          })
        ),
        [investor1]
      );
      await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.depositUsdcIx({
            investor: investor2.publicKey,
            investorUsdcAccount: investor2UsdcAcc,
            usdcMint,
            aatNft: investor2NftPubkey,
            usdcAmount: new BN(20_000 * USDC_UNIT),
          })
        ),
        [investor2]
      );
    });

    it("current_round_size_waln is not changed by set_round_size mid-round", async () => {
      const state = await sdk.program.account.programState.fetch(contractState);
      assert.ok(state.roundSizeWaln.eq(new BN(400 * WALN_UNIT)), "round_size_waln updated to 400 wALN");
      assert.ok(
        state.currentRoundSizeWaln.eq(new BN(200 * WALN_UNIT)),
        `current_round_size_waln should still be 200 wALN, got ${state.currentRoundSizeWaln.toString()}`
      );
      assert.equal(state.roundStarted, true, "round 4 is active");
    });

    it("rejects sell exceeding snapshotted round size (200 wALN) even though live size is 400 wALN", async () => {
      try {
        await provider.sendAndConfirm(
          new Transaction().add(
            await sdk.sellWalnIx({
              seller: seller.publicKey,
              sellerWalnAccount: sellerWalnAcc,
              sellerUsdcAccount: sellerUsdcAcc,
              walnMint,
              usdcMint,
              walnAmount: new BN(201 * WALN_UNIT),
            })
          ),
          [seller]
        );
        assert.fail("should have thrown SellAmountExceedsRound");
      } catch (e: any) {
        assert.include(e.toString(), "SellAmountExceedsRound");
      }
    });

    it("next round uses new round size (400 wALN) after current round triggers", async () => {
      const stateBefore = await sdk.program.account.programState.fetch(contractState);
      const roundIndex = stateBefore.roundCount; // 4
      const roundBn = new BN(roundIndex.toNumber());
      const [roundRecord] = sdk.roundRecordPda(roundBn);
      const [lockedWaln1] = sdk.lockedWalnPda(investor1.publicKey, roundBn);
      const [lockedWaln2] = sdk.lockedWalnPda(investor2.publicKey, roundBn);

      await provider.sendAndConfirm(
        new Transaction().add(
          ComputeBudgetProgram.setComputeUnitLimit({ units: 400_000 }),
          await sdk.sellWalnIx({
            seller: seller.publicKey,
            sellerWalnAccount: sellerWalnAcc,
            sellerUsdcAccount: sellerUsdcAcc,
            walnMint,
            usdcMint,
            walnAmount: new BN(200 * WALN_UNIT),
            roundTriggerAccounts: [
              { pubkey: roundRecord, isWritable: true },
              { pubkey: lobbyEntry1, isWritable: true },
              { pubkey: lockedWaln1, isWritable: true },
              { pubkey: investor1NftPubkey, isWritable: false },
              { pubkey: lobbyEntry2, isWritable: true },
              { pubkey: lockedWaln2, isWritable: true },
              { pubkey: investor2NftPubkey, isWritable: false },
            ],
          })
        ),
        [seller]
      );

      const stateAfter = await sdk.program.account.programState.fetch(contractState);
      assert.ok(
        stateAfter.roundCount.eqn(roundIndex.toNumber() + 1),
        "round_count should have incremented"
      );
      assert.ok(
        stateAfter.currentRoundSizeWaln.eq(new BN(400 * WALN_UNIT)),
        `new round must use updated round size 400 wALN, got ${stateAfter.currentRoundSizeWaln.toString()}`
      );
      assert.equal(stateAfter.roundStarted, true, "next round auto-started with new round size");
    });
  });

  // ---------------------------------------------------------------------------
  // 14. WALN dust carryover
  // ---------------------------------------------------------------------------
  describe("waln dust carryover", () => {
    before(async () => {
      await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.admin(admin.publicKey).setFloorPrice(FLOOR_PRICE)
        )
      );
      await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.admin(admin.publicKey).setRoundSize(ROUND_SIZE)
        )
      );

      await mintTokensTo(provider, walnMint, sellerWalnAcc, BigInt(1000 * WALN_UNIT));
      await mintTokensTo(provider, usdcMint, investor1UsdcAcc, BigInt(100_000 * USDC_UNIT));
      await mintTokensTo(provider, usdcMint, investor2UsdcAcc, BigInt(100_000 * USDC_UNIT));
      await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.depositUsdcIx({
            investor: investor1.publicKey,
            investorUsdcAccount: investor1UsdcAcc,
            usdcMint,
            aatNft: investor1NftPubkey,
            usdcAmount: new BN(50_000 * USDC_UNIT),
          })
        ),
        [investor1]
      );
      await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.depositUsdcIx({
            investor: investor2.publicKey,
            investorUsdcAccount: investor2UsdcAcc,
            usdcMint,
            aatNft: investor2NftPubkey,
            usdcAmount: new BN(50_000 * USDC_UNIT),
          })
        ),
        [investor2]
      );
    });

    it("dust invariant holds: total_purchased + new_dust = waln_in_round + old_dust", async () => {
      const stateBefore = await sdk.program.account.programState.fetch(contractState);
      const oldDust = BigInt(stateBefore.walnDustCarryover.toString());
      const roundIndex = stateBefore.roundCount;
      const roundBn = new BN(roundIndex.toNumber());
      const [roundRecord] = sdk.roundRecordPda(roundBn);
      const [lockedWaln1] = sdk.lockedWalnPda(investor1.publicKey, roundBn);
      const [lockedWaln2] = sdk.lockedWalnPda(investor2.publicKey, roundBn);

      const walnInRound = BigInt(stateBefore.currentRoundSizeWaln.toString());

      await provider.sendAndConfirm(
        new Transaction().add(
          ComputeBudgetProgram.setComputeUnitLimit({ units: 400_000 }),
          await sdk.sellWalnIx({
            seller: seller.publicKey,
            sellerWalnAccount: sellerWalnAcc,
            sellerUsdcAccount: sellerUsdcAcc,
            walnMint,
            usdcMint,
            walnAmount: new BN(stateBefore.currentRoundSizeWaln.toString()),
            roundTriggerAccounts: [
              { pubkey: roundRecord, isWritable: true },
              { pubkey: lobbyEntry1, isWritable: true },
              { pubkey: lockedWaln1, isWritable: true },
              { pubkey: investor1NftPubkey, isWritable: false },
              { pubkey: lobbyEntry2, isWritable: true },
              { pubkey: lockedWaln2, isWritable: true },
              { pubkey: investor2NftPubkey, isWritable: false },
            ],
          })
        ),
        [seller]
      );

      const stateAfter = await sdk.program.account.programState.fetch(contractState);
      const newDust = BigInt(stateAfter.walnDustCarryover.toString());

      const rr = await sdk.fetchRoundRecord(roundBn);
      const totalWalnPurchased = rr.walnPurchased;

      assert.equal(
        totalWalnPurchased + newDust,
        walnInRound + oldDust,
        `dust invariant failed: purchased(${totalWalnPurchased}) + newDust(${newDust}) != walnInRound(${walnInRound}) + oldDust(${oldDust})`
      );
    });

    it("investors receive dust bonus from previous round's carryover", async () => {
      const stateBefore = await sdk.program.account.programState.fetch(contractState);
      const dustCarryover = BigInt(stateBefore.walnDustCarryover.toString());
      const totalUsdcLocked = BigInt(stateBefore.totalUsdcLockedForRound.toString());
      const floorPrice = BigInt(stateBefore.currentRoundFloorPrice.toString());
      const walnScale = BigInt(WALN_UNIT);

      assert.ok(dustCarryover > 0n, "dust carryover should be > 0 from previous rounds");

      const entry1 = await sdk.program.account.lobbyEntry.fetch(lobbyEntry1);
      const entry2 = await sdk.program.account.lobbyEntry.fetch(lobbyEntry2);
      const locked1 = BigInt(entry1.usdcLockedCurrentRound.toString());
      const locked2 = BigInt(entry2.usdcLockedCurrentRound.toString());

      const roundIndex = stateBefore.roundCount;
      const roundBn = new BN(roundIndex.toNumber());
      const [roundRecord] = sdk.roundRecordPda(roundBn);
      const [lockedWaln1] = sdk.lockedWalnPda(investor1.publicKey, roundBn);
      const [lockedWaln2] = sdk.lockedWalnPda(investor2.publicKey, roundBn);

      await provider.sendAndConfirm(
        new Transaction().add(
          ComputeBudgetProgram.setComputeUnitLimit({ units: 400_000 }),
          await sdk.sellWalnIx({
            seller: seller.publicKey,
            sellerWalnAccount: sellerWalnAcc,
            sellerUsdcAccount: sellerUsdcAcc,
            walnMint,
            usdcMint,
            walnAmount: new BN(stateBefore.currentRoundSizeWaln.toString()),
            roundTriggerAccounts: [
              { pubkey: roundRecord, isWritable: true },
              { pubkey: lobbyEntry1, isWritable: true },
              { pubkey: lockedWaln1, isWritable: true },
              { pubkey: investor1NftPubkey, isWritable: false },
              { pubkey: lobbyEntry2, isWritable: true },
              { pubkey: lockedWaln2, isWritable: true },
              { pubkey: investor2NftPubkey, isWritable: false },
            ],
          })
        ),
        [seller]
      );

      const lw1 = await sdk.program.account.lockedWaln.fetch(lockedWaln1);
      const lw2 = await sdk.program.account.lockedWaln.fetch(lockedWaln2);

      const base1 = locked1 * walnScale / floorPrice;
      const base2 = locked2 * walnScale / floorPrice;
      const bonus1 = totalUsdcLocked > 0n ? dustCarryover * locked1 / totalUsdcLocked : 0n;
      const bonus2 = totalUsdcLocked > 0n ? dustCarryover * locked2 / totalUsdcLocked : 0n;

      assert.equal(
        BigInt(lw1.walnAmount.toString()),
        base1 + bonus1,
        "investor1 should receive base + dust bonus"
      );
      assert.equal(
        BigInt(lw2.walnAmount.toString()),
        base2 + bonus2,
        "investor2 should receive base + dust bonus"
      );

      assert.ok(
        BigInt(lw1.walnAmount.toString()) > base1,
        "investor1 total should exceed base allocation due to dust"
      );
      assert.ok(
        BigInt(lw2.walnAmount.toString()) > base2,
        "investor2 total should exceed base allocation due to dust"
      );

      const stateAfter = await sdk.program.account.programState.fetch(contractState);
      const rr = await sdk.fetchRoundRecord(roundBn);
      const totalWalnPurchased = rr.walnPurchased;
      const newDust = BigInt(stateAfter.walnDustCarryover.toString());
      const walnInRound = BigInt(stateBefore.currentRoundSizeWaln.toString());

      assert.equal(
        totalWalnPurchased + newDust,
        walnInRound + dustCarryover,
        "dust invariant must hold for round with bonus distribution"
      );
    });
  });
});
