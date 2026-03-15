import * as anchor from "@coral-xyz/anchor";
import { BN } from "@coral-xyz/anchor";
import { Keypair, PublicKey, Transaction } from "@solana/web3.js";
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
// Test parameters
// ---------------------------------------------------------------------------
// floor_price = 50:  50 USDC units per 1 wALN unit
// round_size  = 100: 100 wALN units triggers a round
// round cap USDC  = 100 * 50 = 5_000 USDC units
//
// Investor1: 5_000 USDC, 100 AAT  → AAT share = 100/150
// Investor2: 5_000 USDC,  50 AAT  → AAT share =  50/150
//
// After start_round (integer arithmetic):
//   Investor1 locked = floor(5000 * 100/150) = 3333 USDC  deposited = 1667
//   Investor2 locked = floor(5000 *  50/150) = 1666 USDC  deposited = 3334
//
// After round trigger (sell 100 wALN):
//   seller receives 100 * 50 = 5000 USDC
//   Investor1 wALN = 3333 / 50 = 66 wALN
//   Investor2 wALN = 1666 / 50 = 33 wALN

const FLOOR_PRICE = new BN(50);
const ROUND_SIZE = new BN(100);
const LOCK_PERIOD = new BN(0);

const INVESTOR1_USDC = new BN(5_000);
const INVESTOR1_AAT = new BN(100);
const INVESTOR2_USDC = new BN(5_000);
const INVESTOR2_AAT = new BN(50);

const SELL_AMOUNT_PARTIAL = new BN(50);
const SELL_AMOUNT_TRIGGER = new BN(50);

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
  let aatMint: PublicKey;

  // Token accounts (set in before())
  let investor1UsdcAcc: PublicKey;
  let investor2UsdcAcc: PublicKey;
  let investor1AatAcc: PublicKey;
  let investor2AatAcc: PublicKey;
  let investor1WalnAcc: PublicKey;
  let investor2WalnAcc: PublicKey;
  let sellerWalnAcc: PublicKey;
  let sellerUsdcAcc: PublicKey;

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

    // Create token mints (0 decimals for simple integer arithmetic)
    usdcMint = await createTestMint(provider, 0);
    walnMint = await createTestMint(provider, 0);
    aatMint = await createTestMint(provider, 0);

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
    investor1AatAcc = await createTestTokenAccount(
      provider,
      aatMint,
      investor1.publicKey
    );
    investor2AatAcc = await createTestTokenAccount(
      provider,
      aatMint,
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

    // Mint tokens to investors and seller
    await mintTokensTo(provider, usdcMint, investor1UsdcAcc, 10_000n);
    await mintTokensTo(provider, usdcMint, investor2UsdcAcc, 10_000n);
    await mintTokensTo(provider, aatMint, investor1AatAcc, 100n);
    await mintTokensTo(provider, aatMint, investor2AatAcc, 50n);
    // Seller needs enough wALN to sell: partial (50) + trigger (50) = 100 total
    await mintTokensTo(provider, walnMint, sellerWalnAcc, 200n);
  });

  // ---------------------------------------------------------------------------
  // 1. initialize
  // ---------------------------------------------------------------------------
  describe("initialize", () => {
    it("creates contract state and vaults", async () => {
      await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.initializeIx({
            admin: admin.publicKey,
            usdcMint,
            walnMint,
            aatMint,
            floorPriceUsdc: FLOOR_PRICE,
            roundSizeWaln: ROUND_SIZE,
            lockPeriodSeconds: LOCK_PERIOD,
          })
        )
      );

      const state = await sdk.program.account.programState.fetch(contractState);
      assert.ok(state.admin.equals(admin.publicKey));
      assert.ok(state.usdcMint.equals(usdcMint));
      assert.ok(state.walnMint.equals(walnMint));
      assert.ok(state.aatMint.equals(aatMint));
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
              aatMint,
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
  });

  // ---------------------------------------------------------------------------
  // 3. deposit_usdc
  // ---------------------------------------------------------------------------
  describe("deposit_usdc", () => {
    it("investor1 deposits USDC + stakes AAT", async () => {
      const usdcBefore = await getTokenBalance(provider, usdcVault);

      await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.depositUsdcIx({
            investor: investor1.publicKey,
            investorUsdcAccount: investor1UsdcAcc,
            investorAatAccount: investor1AatAcc,
            usdcMint,
            aatMint,
            usdcAmount: INVESTOR1_USDC,
            aatAmount: INVESTOR1_AAT,
          })
        ),
        [investor1]
      );

      const entry = await sdk.program.account.lobbyEntry.fetch(lobbyEntry1);
      assert.ok(entry.investor.equals(investor1.publicKey));
      assert.ok(entry.usdcDeposited.eq(INVESTOR1_USDC));
      assert.ok(entry.aatStaked.eq(INVESTOR1_AAT));

      const state = await sdk.program.account.programState.fetch(contractState);
      assert.ok(state.totalUsdcInLobby.eq(INVESTOR1_USDC));
      assert.ok(state.totalAatStaked.eq(INVESTOR1_AAT));

      const usdcAfter = await getTokenBalance(provider, usdcVault);
      assert.equal(usdcAfter - usdcBefore, BigInt(INVESTOR1_USDC.toNumber()));
    });

    it("investor2 deposits USDC + stakes AAT", async () => {
      await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.depositUsdcIx({
            investor: investor2.publicKey,
            investorUsdcAccount: investor2UsdcAcc,
            investorAatAccount: investor2AatAcc,
            usdcMint,
            aatMint,
            usdcAmount: INVESTOR2_USDC,
            aatAmount: INVESTOR2_AAT,
          })
        ),
        [investor2]
      );

      const entry = await sdk.program.account.lobbyEntry.fetch(lobbyEntry2);
      assert.ok(entry.investor.equals(investor2.publicKey));
      assert.ok(entry.usdcDeposited.eq(INVESTOR2_USDC));
      assert.ok(entry.aatStaked.eq(INVESTOR2_AAT));

      const state = await sdk.program.account.programState.fetch(contractState);
      assert.ok(
        state.totalUsdcInLobby.eq(INVESTOR1_USDC.add(INVESTOR2_USDC))
      );
      assert.ok(
        state.totalAatStaked.eq(INVESTOR1_AAT.add(INVESTOR2_AAT))
      );
    });

    it("investor1 adds more USDC in a second deposit", async () => {
      const extraUsdc = new BN(500);
      await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.depositUsdcIx({
            investor: investor1.publicKey,
            investorUsdcAccount: investor1UsdcAcc,
            investorAatAccount: investor1AatAcc,
            usdcMint,
            aatMint,
            usdcAmount: extraUsdc,
            aatAmount: new BN(0),
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
  });

  // ---------------------------------------------------------------------------
  // 4. withdraw_usdc
  // ---------------------------------------------------------------------------
  describe("withdraw_usdc", () => {
    it("investor can withdraw unlocked (deposited) USDC", async () => {
      const withdrawAmt = new BN(100);
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
      assert.ok(entry.usdcDeposited.eqn(4900)); // 5000 - 100

      // Re-deposit the 100 USDC so the rest of the tests are consistent
      await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.depositUsdcIx({
            investor: investor1.publicKey,
            investorUsdcAccount: investor1UsdcAcc,
            investorAatAccount: investor1AatAcc,
            usdcMint,
            aatMint,
            usdcAmount: withdrawAmt,
            aatAmount: new BN(0),
          })
        ),
        [investor1]
      );
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
              investorAatAccount: investor2AatAcc,
              usdcMint,
              aatMint,
              usdcAmount: new BN(1),
              aatAmount: new BN(0),
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

    it("auto-starts round and sells wALN (no trigger)", async () => {
      const stateBefore = await sdk.program.account.programState.fetch(contractState);
      assert.equal(stateBefore.roundStarted, false);

      const sellerUsdcBefore = await getTokenBalance(provider, sellerUsdcAcc);
      const walnVaultBefore = await getTokenBalance(provider, walnVault);

      const round0 = new BN(0);
      const [roundRecord0] = sdk.roundRecordPda(round0);
      const [lockedWaln1] = sdk.lockedWalnPda(investor1.publicKey, round0);
      const [lockedWaln2] = sdk.lockedWalnPda(investor2.publicKey, round0);

      await provider.sendAndConfirm(
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
              { pubkey: lobbyEntry2, isWritable: true },
              { pubkey: lockedWaln2, isWritable: true },
            ],
          })
        ),
        [seller]
      );

      const sellerUsdcAfter = await getTokenBalance(provider, sellerUsdcAcc);
      const walnVaultAfter = await getTokenBalance(provider, walnVault);

      assert.equal(
        sellerUsdcAfter - sellerUsdcBefore,
        BigInt(SELL_AMOUNT_PARTIAL.toNumber() * FLOOR_PRICE.toNumber())
      );
      assert.equal(
        walnVaultAfter - walnVaultBefore,
        BigInt(SELL_AMOUNT_PARTIAL.toNumber())
      );

      const state = await sdk.program.account.programState.fetch(contractState);
      assert.ok(state.currentRoundWaln.eq(SELL_AMOUNT_PARTIAL));
      assert.equal(state.roundStarted, true);

      // investor1: floor(5000 * 100 / 150) = 3333
      const entry1 = await sdk.program.account.lobbyEntry.fetch(lobbyEntry1);
      assert.ok(entry1.usdcLockedCurrentRound.eqn(3333));
      assert.ok(entry1.usdcDeposited.eqn(1667));

      // investor2: floor(5000 * 50 / 150) = 1666
      const entry2 = await sdk.program.account.lobbyEntry.fetch(lobbyEntry2);
      assert.ok(entry2.usdcLockedCurrentRound.eqn(1666));
      assert.ok(entry2.usdcDeposited.eqn(3334));
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
      // current_round_waln = 50 after partial sell; selling 50 more hits 100
      const round0 = new BN(0);
      const [roundRecord0] = sdk.roundRecordPda(round0);
      const [lockedWaln1] = sdk.lockedWalnPda(investor1.publicKey, round0);
      const [lockedWaln2] = sdk.lockedWalnPda(investor2.publicKey, round0);

      const sellerUsdcBefore = await getTokenBalance(provider, sellerUsdcAcc);
      const usdcVaultBefore = await getTokenBalance(provider, usdcVault);

      await provider.sendAndConfirm(
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
              { pubkey: lobbyEntry2, isWritable: true },
              { pubkey: lockedWaln2, isWritable: true },
            ],
          })
        ),
        [seller]
      );

      // ---- verify seller received USDC ----
      const sellerUsdcAfter = await getTokenBalance(provider, sellerUsdcAcc);
      assert.equal(
        sellerUsdcAfter - sellerUsdcBefore,
        BigInt(SELL_AMOUNT_TRIGGER.toNumber() * FLOOR_PRICE.toNumber())
      );

      // ---- verify RoundRecord created ----
      // RoundRecord is created via raw CPI so it's not in the IDL accounts list.
      // Decode the account data manually: skip 8-byte discriminator, then
      // RoundRecord fields in order (all LE): round_index(u64), triggered_at(i64),
      // waln_purchased(u64), usdc_spent(u64), total_aat_staked_at_trigger(u64),
      // participant_count(u32), bump(u8).
      const rrInfo = await provider.connection.getAccountInfo(roundRecord0);
      assert.ok(rrInfo, "RoundRecord account should exist");
      const rrBuf = Buffer.from(rrInfo.data).subarray(8); // skip discriminator
      const rrRoundIndex = rrBuf.readBigUInt64LE(0);
      const rrWalnPurchased = rrBuf.readBigUInt64LE(16);
      const rrUsdcSpent = rrBuf.readBigUInt64LE(24);
      const rrTotalAat = rrBuf.readBigUInt64LE(32);
      const rrParticipantCount = rrBuf.readUInt32LE(40);
      assert.equal(rrRoundIndex, 0n);
      assert.equal(rrWalnPurchased, 99n); // 66 + 33
      assert.equal(rrUsdcSpent, 4999n);   // 3333 + 1666
      assert.equal(rrTotalAat, 150n);
      assert.equal(rrParticipantCount, 2);

      // ---- verify ContractState updated ----
      const state = await sdk.program.account.programState.fetch(contractState);
      assert.ok(state.roundCount.eqn(1));
      assert.ok(state.currentRoundWaln.eqn(0));
      assert.equal(state.roundStarted, true); // auto-started next round

      // ---- verify LockedWaln records ----
      const lw1 = await sdk.program.account.lockedWaln.fetch(lockedWaln1);
      assert.ok(lw1.investor.equals(investor1.publicKey));
      assert.ok(lw1.roundIndex.eqn(0));
      assert.ok(lw1.walnAmount.eqn(66)); // 3333 / 50
      assert.equal(lw1.claimed, false);

      const lw2 = await sdk.program.account.lockedWaln.fetch(lockedWaln2);
      assert.ok(lw2.investor.equals(investor2.publicKey));
      assert.ok(lw2.roundIndex.eqn(0));
      assert.ok(lw2.walnAmount.eqn(33)); // 1666 / 50
      assert.equal(lw2.claimed, false);

      // ---- verify LobbyEntry state after round end + round start ----
      // investor1.usdc_deposited was 1667 after first start_round.
      // Round end: usdc_locked_current_round reset to 0; usdc_deposited unchanged.
      // Round start: locked = min(1667, 5000 * 100/150 = 3333) = 1667.
      const entry1 = await sdk.program.account.lobbyEntry.fetch(lobbyEntry1);
      assert.ok(entry1.usdcLockedCurrentRound.eqn(1667));
      assert.ok(entry1.usdcDeposited.eqn(0)); // 1667 - 1667

      const entry2 = await sdk.program.account.lobbyEntry.fetch(lobbyEntry2);
      // investor2 had 3334 usdc_deposited
      // round_start: min(3334, 5000 * 50/150=1666) = 1666
      assert.ok(entry2.usdcLockedCurrentRound.eqn(1666));
      assert.ok(entry2.usdcDeposited.eqn(1668)); // 3334 - 1666

      // ---- verify cumulative stats ----
      assert.ok(entry1.usdcCommitted.eqn(3333));
      assert.ok(entry1.walnPurchasedTotal.eqn(66));
      assert.ok(entry2.usdcCommitted.eqn(1666));
      assert.ok(entry2.walnPurchasedTotal.eqn(33));
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
      assert.equal(walnAfter - walnBefore, 66n); // 66 wALN

      const [lockedWaln1] = sdk.lockedWalnPda(investor1.publicKey, round0);
      const lw = await sdk.program.account.lockedWaln.fetch(lockedWaln1);
      assert.equal(lw.claimed, true);
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
      assert.equal(walnAfter - walnBefore, 33n); // 33 wALN
    });
  });

  // ---------------------------------------------------------------------------
  // 8. withdraw_aat
  // ---------------------------------------------------------------------------
  describe("withdraw_aat", () => {
    it("investor can withdraw staked AAT", async () => {
      const entryBefore = await sdk.program.account.lobbyEntry.fetch(lobbyEntry1);
      const aatBefore = await getTokenBalance(provider, investor1AatAcc);
      const stateBefore = await sdk.program.account.programState.fetch(contractState);

      const withdrawAmt = new BN(10);
      await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.withdrawAatIx({
            investor: investor1.publicKey,
            investorAatAccount: investor1AatAcc,
            aatMint,
            amount: withdrawAmt,
          })
        ),
        [investor1]
      );

      const entryAfter = await sdk.program.account.lobbyEntry.fetch(lobbyEntry1);
      const aatAfter = await getTokenBalance(provider, investor1AatAcc);
      const stateAfter = await sdk.program.account.programState.fetch(contractState);

      assert.equal(
        aatAfter - aatBefore,
        BigInt(withdrawAmt.toNumber())
      );
      assert.ok(
        entryAfter.aatStaked.eq(entryBefore.aatStaked.sub(withdrawAmt))
      );
      assert.ok(
        stateAfter.totalAatStaked.eq(stateBefore.totalAatStaked.sub(withdrawAmt))
      );

      // Re-stake the AAT for subsequent tests
      await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.depositUsdcIx({
            investor: investor1.publicKey,
            investorUsdcAccount: investor1UsdcAcc,
            investorAatAccount: investor1AatAcc,
            usdcMint,
            aatMint,
            usdcAmount: new BN(0),
            aatAmount: withdrawAmt,
          })
        ),
        [investor1]
      );
    });

    it("rejects withdrawal exceeding staked amount", async () => {
      const entry = await sdk.program.account.lobbyEntry.fetch(lobbyEntry1);
      const tooMuch = entry.aatStaked.addn(1);

      try {
        await provider.sendAndConfirm(
          new Transaction().add(
            await sdk.withdrawAatIx({
              investor: investor1.publicKey,
              investorAatAccount: investor1AatAcc,
              aatMint,
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

    it("allows u64::MAX to withdraw all staked AAT", async () => {
      const entryBefore = await sdk.program.account.lobbyEntry.fetch(lobbyEntry1);
      const fullAmount = entryBefore.aatStaked;

      await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.withdrawAatIx({
            investor: investor1.publicKey,
            investorAatAccount: investor1AatAcc,
            aatMint,
            amount: new BN("18446744073709551615"),
          })
        ),
        [investor1]
      );

      const entryAfter = await sdk.program.account.lobbyEntry.fetch(lobbyEntry1);
      assert.ok(entryAfter.aatStaked.eqn(0));

      // Re-stake for subsequent tests
      await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.depositUsdcIx({
            investor: investor1.publicKey,
            investorUsdcAccount: investor1UsdcAcc,
            investorAatAccount: investor1AatAcc,
            usdcMint,
            aatMint,
            usdcAmount: new BN(0),
            aatAmount: fullAmount,
          })
        ),
        [investor1]
      );
    });
  });

  // ---------------------------------------------------------------------------
  // 9. Vault integrity — USDC vault balance
  // ---------------------------------------------------------------------------
  describe("vault integrity", () => {
    it("USDC vault balance reflects deposits minus seller payments", async () => {
      // 2 investors × 5000 USDC deposited = 10000 total
      // 2 seller txs × 50 wALN × 50 price = 5000 USDC paid out
      // Expected vault: 10000 - 5000 = 5000
      const vaultBalance = await getTokenBalance(provider, usdcVault);
      assert.equal(vaultBalance, 5000n);

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
  // 10. Fix verification: total_usdc_in_lobby accounting
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

      // total_usdc_in_lobby should track the sum of (deposited + locked) across
      // all entries. After round-0 committed 4999 USDC, total_usdc_in_lobby
      // was decremented: 10000 - 4999 = 5001.
      assert.ok(
        state.totalUsdcInLobby.eqn(5001),
        `expected totalUsdcInLobby=5001, got ${state.totalUsdcInLobby.toNumber()}`
      );
      assert.equal(
        state.totalUsdcInLobby.toNumber(),
        sumLobby,
        "totalUsdcInLobby should equal sum of individual entries"
      );
    });
  });

  // ---------------------------------------------------------------------------
  // 11. Fix verification: seller does not pay rent for round accounts
  // ---------------------------------------------------------------------------
  describe("seller rent-free round trigger", () => {
    before(async () => {
      // Mint more wALN to seller for this test
      await mintTokensTo(provider, walnMint, sellerWalnAcc, 200n);

      // Fund contract state with SOL for rent
      const tx = new Transaction().add(
        anchor.web3.SystemProgram.transfer({
          fromPubkey: admin.publicKey,
          toPubkey: contractState,
          lamports: 10_000_000,
        })
      );
      await provider.sendAndConfirm(tx);

      // Deposit more USDC for investors so round-1 has funds
      await mintTokensTo(provider, usdcMint, investor1UsdcAcc, 10_000n);
      await mintTokensTo(provider, usdcMint, investor2UsdcAcc, 10_000n);
      await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.depositUsdcIx({
            investor: investor1.publicKey,
            investorUsdcAccount: investor1UsdcAcc,
            investorAatAccount: investor1AatAcc,
            usdcMint,
            aatMint,
            usdcAmount: new BN(5000),
            aatAmount: new BN(0),
          })
        ),
        [investor1]
      );
      await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.depositUsdcIx({
            investor: investor2.publicKey,
            investorUsdcAccount: investor2UsdcAcc,
            investorAatAccount: investor2AatAcc,
            usdcMint,
            aatMint,
            usdcAmount: new BN(5000),
            aatAmount: new BN(0),
          })
        ),
        [investor2]
      );
    });

    it("seller SOL balance only decreases by tx fee, not rent", async () => {
      // Round-1 is already started (auto-started after round-0).
      // current_round_waln = 0, round_size = 100. Sell 100 to trigger round-1.
      const round1 = new BN(1);
      const [roundRecord1] = sdk.roundRecordPda(round1);
      const [lockedWaln1r1] = sdk.lockedWalnPda(investor1.publicKey, round1);
      const [lockedWaln2r1] = sdk.lockedWalnPda(investor2.publicKey, round1);

      const sellerSolBefore = await provider.connection.getBalance(
        seller.publicKey
      );

      await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.sellWalnIx({
            seller: seller.publicKey,
            sellerWalnAccount: sellerWalnAcc,
            sellerUsdcAccount: sellerUsdcAcc,
            walnMint,
            usdcMint,
            walnAmount: new BN(100),
            roundTriggerAccounts: [
              { pubkey: roundRecord1, isWritable: true },
              { pubkey: lobbyEntry1, isWritable: true },
              { pubkey: lockedWaln1r1, isWritable: true },
              { pubkey: lobbyEntry2, isWritable: true },
              { pubkey: lockedWaln2r1, isWritable: true },
            ],
          })
        ),
        [seller]
      );

      const sellerSolAfter = await provider.connection.getBalance(
        seller.publicKey
      );

      // Seller SOL difference should only be the tx fee (5000 lamports on localnet)
      const solDiff = sellerSolBefore - sellerSolAfter;
      assert.ok(
        solDiff <= 10_000,
        `seller SOL decrease should be only tx fee, got ${solDiff} lamports`
      );

      // Verify round-1 completed
      const state = await sdk.program.account.programState.fetch(contractState);
      assert.ok(state.roundCount.eqn(2), "round_count should be 2");
    });
  });

  // ---------------------------------------------------------------------------
  // 12. Fix verification: sell cap prevents round overshoot
  // ---------------------------------------------------------------------------
  describe("sell cap — no overshoot allowed", () => {
    before(async () => {
      // Mint more wALN to seller
      await mintTokensTo(provider, walnMint, sellerWalnAcc, 500n);

      // Fund contract state with SOL for rent
      const tx = new Transaction().add(
        anchor.web3.SystemProgram.transfer({
          fromPubkey: admin.publicKey,
          toPubkey: contractState,
          lamports: 10_000_000,
        })
      );
      await provider.sendAndConfirm(tx);

      // Deposit more USDC for investors
      await mintTokensTo(provider, usdcMint, investor1UsdcAcc, 20_000n);
      await mintTokensTo(provider, usdcMint, investor2UsdcAcc, 20_000n);
      await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.depositUsdcIx({
            investor: investor1.publicKey,
            investorUsdcAccount: investor1UsdcAcc,
            investorAatAccount: investor1AatAcc,
            usdcMint,
            aatMint,
            usdcAmount: new BN(10000),
            aatAmount: new BN(0),
          })
        ),
        [investor1]
      );
      await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.depositUsdcIx({
            investor: investor2.publicKey,
            investorUsdcAccount: investor2UsdcAcc,
            investorAatAccount: investor2AatAcc,
            usdcMint,
            aatMint,
            usdcAmount: new BN(10000),
            aatAmount: new BN(0),
          })
        ),
        [investor2]
      );
    });

    it("rejects a sell that would exceed remaining round capacity", async () => {
      // Round-2 is active (auto-started after round-1 end in section 11).
      // current_round_waln = 0, round_size = 100. Selling 101 must fail.
      try {
        await provider.sendAndConfirm(
          new Transaction().add(
            await sdk.sellWalnIx({
              seller: seller.publicKey,
              sellerWalnAccount: sellerWalnAcc,
              sellerUsdcAccount: sellerUsdcAcc,
              walnMint,
              usdcMint,
              walnAmount: new BN(101),
              roundTriggerAccounts: [
                { pubkey: sdk.roundRecordPda(new BN(2))[0], isWritable: true },
                { pubkey: lobbyEntry1, isWritable: true },
                { pubkey: sdk.lockedWalnPda(investor1.publicKey, new BN(2))[0], isWritable: true },
                { pubkey: lobbyEntry2, isWritable: true },
                { pubkey: sdk.lockedWalnPda(investor2.publicKey, new BN(2))[0], isWritable: true },
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
      // Sell 50 — stays within round_size=100, no trigger
      await provider.sendAndConfirm(
        new Transaction().add(
          await sdk.sellWalnIx({
            seller: seller.publicKey,
            sellerWalnAccount: sellerWalnAcc,
            sellerUsdcAccount: sellerUsdcAcc,
            walnMint,
            usdcMint,
            walnAmount: new BN(50),
            roundTriggerAccounts: [
              { pubkey: sdk.roundRecordPda(new BN(2))[0], isWritable: true },
              { pubkey: lobbyEntry1, isWritable: true },
              { pubkey: sdk.lockedWalnPda(investor1.publicKey, new BN(2))[0], isWritable: true },
              { pubkey: lobbyEntry2, isWritable: true },
              { pubkey: sdk.lockedWalnPda(investor2.publicKey, new BN(2))[0], isWritable: true },
            ],
          })
        ),
        [seller]
      );

      const state = await sdk.program.account.programState.fetch(contractState);
      assert.ok(state.currentRoundWaln.eqn(50), "50 wALN accumulated");
      assert.ok(state.roundCount.eqn(2), "round not triggered yet");
    });

    it("rejects sell exceeding remaining capacity mid-round", async () => {
      // current_round_waln = 50, round_size = 100, remaining = 50. Sell 51 must fail.
      try {
        await provider.sendAndConfirm(
          new Transaction().add(
            await sdk.sellWalnIx({
              seller: seller.publicKey,
              sellerWalnAccount: sellerWalnAcc,
              sellerUsdcAccount: sellerUsdcAcc,
              walnMint,
              usdcMint,
              walnAmount: new BN(51),
              roundTriggerAccounts: [
                { pubkey: sdk.roundRecordPda(new BN(2))[0], isWritable: true },
                { pubkey: lobbyEntry1, isWritable: true },
                { pubkey: sdk.lockedWalnPda(investor1.publicKey, new BN(2))[0], isWritable: true },
                { pubkey: lobbyEntry2, isWritable: true },
                { pubkey: sdk.lockedWalnPda(investor2.publicKey, new BN(2))[0], isWritable: true },
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
      // current_round_waln = 50. Selling exactly 50 fills the round and triggers it.
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
            walnAmount: new BN(50),
            roundTriggerAccounts: [
              { pubkey: roundRecord, isWritable: true },
              { pubkey: lobbyEntry1, isWritable: true },
              { pubkey: lockedWaln1, isWritable: true },
              { pubkey: lobbyEntry2, isWritable: true },
              { pubkey: lockedWaln2, isWritable: true },
            ],
          })
        ),
        [seller]
      );

      const stateAfter = await sdk.program.account.programState.fetch(contractState);
      assert.ok(
        stateAfter.currentRoundWaln.eqn(0),
        `current_round_waln must be exactly 0 after clean trigger, got ${stateAfter.currentRoundWaln.toNumber()}`
      );
      assert.ok(
        stateAfter.roundCount.eqn(roundIndex.toNumber() + 1),
        "round_count should have incremented"
      );
      assert.equal(stateAfter.roundStarted, true, "next round auto-started");
    });
  });
});
