import * as anchor from "@coral-xyz/anchor";
import {AnchorProvider, BN} from "@coral-xyz/anchor";
import {
    ComputeBudgetProgram,
    Keypair,
    PublicKey,
    SystemProgram,
    Transaction,
    sendAndConfirmTransaction,
} from "@solana/web3.js";
import {
    createTransferInstruction,
    getAssociatedTokenAddressSync,
    TOKEN_2022_PROGRAM_ID,
} from "@solana/spl-token";
import {assert} from "chai";
import {FloorSdk} from "../sdk/floor-sdk";
import {
    createTestMint,
    createTestTokenAccount,
    mintTokensTo,
    getTokenBalance,
    sleep,
    createToken2022MintWithTransferHook,
} from "./helpers";
import {
    deriveCredentialPda,
    deriveSchemaPda,
    deriveAttestationPda,
    getCreateCredentialInstruction,
    getCreateSchemaInstruction,
    getCreateAttestationInstruction,
    serializeAttestationData,
} from "sas-lib";
import { address } from "@solana/kit";

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

const ALIEN_ID_HOOK_PROGRAM_ID = new PublicKey("BBuax7pfatrjWLx2KLNrKopdQz9eLmtDcC93wughEP7F");
const SAS_PROGRAM_ID = new PublicKey("22zoJMtdu4tQc2PzL74ZUT7FrwgB1Udec8DdW4yw4BdG");

const SAS_CREDENTIAL_NAME = "floor_credential";
const SAS_SCHEMA_NAME = "floor_schema";
const SAS_SCHEMA_LAYOUT = new Uint8Array([12]);
const SAS_SCHEMA_FIELD_NAMES = ["session_address"];

function encodeFieldNames(names: string[]): Uint8Array {
    const parts: Buffer[] = [];
    for (const name of names) {
        const b = Buffer.from(name, "utf8");
        const len = Buffer.allocUnsafe(4);
        len.writeUInt32LE(b.length, 0);
        parts.push(len, b);
    }
    return Buffer.concat(parts);
}

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
    let investorPool: PublicKey;

    // Token-2022 walnMint keypair (generated once, used throughout)
    const walnMintKeypair = Keypair.generate();

    // SAS credential / schema PDAs (set in before())
    let credentialPda: PublicKey;
    let schemaPda: PublicKey;

    // ---------------------------------------------------------------------------
    // Global setup
    // ---------------------------------------------------------------------------
    before(async () => {
        [contractState] = sdk.contractStatePda();
        [usdcVault] = sdk.usdcVaultPda();
        [walnVault] = sdk.walnVaultPda();
        [investorPool] = sdk.investorPoolPda();

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

        // ------------------------------------------------------------------
        // SAS setup: derive PDAs via sas-lib, create credential + schema
        // ------------------------------------------------------------------
        const adminAddr = address(admin.publicKey.toBase58());

        const [credentialPdaAddr] = await deriveCredentialPda({
            authority: adminAddr,
            name: SAS_CREDENTIAL_NAME,
        });
        credentialPda = new PublicKey(credentialPdaAddr);

        const [schemaPdaAddr] = await deriveSchemaPda({
            credential: credentialPdaAddr,
            name: SAS_SCHEMA_NAME,
            version: 1,
        });
        schemaPda = new PublicKey(schemaPdaAddr);

        const createCredIx = getCreateCredentialInstruction({
            payer: adminAddr as any,
            authority: adminAddr as any,
            credential: credentialPdaAddr,
            signers: [adminAddr],
            name: SAS_CREDENTIAL_NAME,
        } as any);
        await sendAndConfirmTransaction(
            provider.connection,
            new Transaction().add(new anchor.web3.TransactionInstruction({
                keys: [
                    { pubkey: admin.publicKey, isSigner: true, isWritable: true },
                    { pubkey: credentialPda, isSigner: false, isWritable: true },
                    { pubkey: admin.publicKey, isSigner: true, isWritable: false },
                    { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
                ],
                programId: SAS_PROGRAM_ID,
                data: Buffer.from(createCredIx.data),
            })),
            [admin],
            { commitment: "confirmed" }
        );

        const createSchemaIx = getCreateSchemaInstruction({
            payer: adminAddr as any,
            authority: adminAddr as any,
            credential: credentialPdaAddr,
            schema: schemaPdaAddr,
            name: SAS_SCHEMA_NAME,
            description: "Schema for floor program seller verification",
            layout: SAS_SCHEMA_LAYOUT,
            fieldNames: SAS_SCHEMA_FIELD_NAMES,
        } as any);
        await sendAndConfirmTransaction(
            provider.connection,
            new Transaction().add(new anchor.web3.TransactionInstruction({
                keys: [
                    { pubkey: admin.publicKey, isSigner: true, isWritable: true },
                    { pubkey: admin.publicKey, isSigner: true, isWritable: false },
                    { pubkey: credentialPda, isSigner: false, isWritable: false },
                    { pubkey: schemaPda, isSigner: false, isWritable: true },
                    { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
                ],
                programId: SAS_PROGRAM_ID,
                data: Buffer.from(createSchemaIx.data),
            })),
            [admin],
            { commitment: "confirmed" }
        );

        // ------------------------------------------------------------------
        // Create Token-2022 walnMint with alien_id transfer hook
        // ------------------------------------------------------------------
        walnMint = await createToken2022MintWithTransferHook(
            provider,
            walnMintKeypair,
            WALN_DECIMALS,
            ALIEN_ID_HOOK_PROGRAM_ID,
            credentialPda,
            schemaPda,
            SAS_PROGRAM_ID
        );

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
            investor1.publicKey,
            TOKEN_2022_PROGRAM_ID
        );
        investor2WalnAcc = await createTestTokenAccount(
            provider,
            walnMint,
            investor2.publicKey,
            TOKEN_2022_PROGRAM_ID
        );
        sellerWalnAcc = await createTestTokenAccount(
            provider,
            walnMint,
            seller.publicKey,
            TOKEN_2022_PROGRAM_ID
        );
        sellerUsdcAcc = await createTestTokenAccount(
            provider,
            usdcMint,
            seller.publicKey
        );

        await mintTokensTo(provider, usdcMint, investor1UsdcAcc, BigInt(10_000 * USDC_UNIT));
        await mintTokensTo(provider, usdcMint, investor2UsdcAcc, BigInt(10_000 * USDC_UNIT));
        await mintTokensTo(provider, walnMint, sellerWalnAcc, BigInt(200 * WALN_UNIT), TOKEN_2022_PROGRAM_ID);

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
                    walnTokenProgram: TOKEN_2022_PROGRAM_ID,
                })
            )
        );

        const hookProgram = new anchor.Program(
            require("../idl/alien_id_transfer_hook.json"),
            provider
        );
        const [hookConfigPda] = PublicKey.findProgramAddressSync(
            [Buffer.from("config"), walnMint.toBuffer()],
            ALIEN_ID_HOOK_PROGRAM_ID
        );
        const [whitelistEntryPda] = PublicKey.findProgramAddressSync(
            [Buffer.from("whitelist"), walnMint.toBuffer(), contractState.toBuffer()],
            ALIEN_ID_HOOK_PROGRAM_ID
        );
        await (hookProgram.methods as any)
            .addToWhitelist(contractState)
            .accounts({
                authority: admin.publicKey,
                config: hookConfigPda,
                whitelistEntry: whitelistEntryPda,
                mint: walnMint,
                systemProgram: SystemProgram.programId,
            })
            .rpc({ commitment: "confirmed" });

        const sellerAddr = address(seller.publicKey.toBase58());
        const [sellerAttestationPdaAddr] = await deriveAttestationPda({
            credential: credentialPdaAddr,
            schema: schemaPdaAddr,
            nonce: sellerAddr,
        });
        const sellerAttestationPda = new PublicKey(sellerAttestationPdaAddr);

        const attestationExpiry = BigInt(Math.floor(Date.now() / 1000) + 365 * 24 * 60 * 60);
        const attestationData = serializeAttestationData(
            { layout: SAS_SCHEMA_LAYOUT, fieldNames: encodeFieldNames(SAS_SCHEMA_FIELD_NAMES) } as any,
            { session_address: "000000010100000000000550ddb1afe5" }
        );
        const createAttestIx = getCreateAttestationInstruction({
            payer: adminAddr as any,
            authority: adminAddr as any,
            credential: credentialPdaAddr,
            schema: schemaPdaAddr,
            attestation: sellerAttestationPdaAddr,
            nonce: sellerAddr,
            data: attestationData,
            expiry: attestationExpiry,
        } as any);
        await sendAndConfirmTransaction(
            provider.connection,
            new Transaction().add(new anchor.web3.TransactionInstruction({
                keys: [
                    { pubkey: admin.publicKey, isSigner: true, isWritable: true },
                    { pubkey: admin.publicKey, isSigner: true, isWritable: false },
                    { pubkey: credentialPda, isSigner: false, isWritable: false },
                    { pubkey: schemaPda, isSigner: false, isWritable: false },
                    { pubkey: sellerAttestationPda, isSigner: false, isWritable: true },
                    { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
                ],
                programId: SAS_PROGRAM_ID,
                data: Buffer.from(createAttestIx.data),
            })),
            [admin],
            { commitment: "confirmed" }
        );

        // ------------------------------------------------------------------
        // Mint AAT NFTs via the floor program (Token-2022, single transaction).
        // The mint keypair co-signs so it can be registered as the new account address.
        // ------------------------------------------------------------------
        await provider.sendAndConfirm(
            new Transaction().add(
                ComputeBudgetProgram.setComputeUnitLimit({units: 400_000}),
                await sdk.mintAatNftIx({
                    admin: admin.publicKey,
                    investor: investor1.publicKey,
                    aatVolume: new BN(100000),
                })
            )
        );

        await provider.sendAndConfirm(
            new Transaction().add(
                ComputeBudgetProgram.setComputeUnitLimit({units: 400_000}),
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
            assert.equal(state.paused, 0);
            assert.equal(state.roundStarted, 0);
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
                            walnTokenProgram: TOKEN_2022_PROGRAM_ID,
                        })
                    )
                );
                assert.fail("should have thrown");
            } catch (_) {
            }
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
                        ComputeBudgetProgram.setComputeUnitLimit({units: 400_000}),
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
                    ComputeBudgetProgram.setComputeUnitLimit({units: 400_000}),
                    await sdk.mintAatNftIx({
                        admin: admin.publicKey,
                        investor: investor4.publicKey,
                        aatVolume: new BN(749_999),
                    })
                )
            );

            const state = await sdk.program.account.programState.fetch(contractState);
            assert.ok(
                state.totalAatVolume.eqn(899_999),
                `expected totalAatVolume=899_999, got ${state.totalAatVolume}`
            );
        });

        it("rejects mint when total would exceed 1_000_000", async () => {
            const investor5 = Keypair.generate();
            const sig = await provider.connection.requestAirdrop(
                investor5.publicKey,
                2_000_000_000
            );
            await provider.connection.confirmTransaction(sig);

            try {
                await provider.sendAndConfirm(
                    new Transaction().add(
                        ComputeBudgetProgram.setComputeUnitLimit({units: 400_000}),
                        await sdk.mintAatNftIx({
                            admin: admin.publicKey,
                            investor: investor5.publicKey,
                            aatVolume: new BN(100_002),
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
            assert.equal(state.paused, 1);

            await provider.sendAndConfirm(
                new Transaction().add(await sdk.admin(admin.publicKey).setPaused(false))
            );
            state = await sdk.program.account.programState.fetch(contractState);
            assert.equal(state.paused, 0);
        });

        it("fund treasury by admin", async () => {
            await provider.sendAndConfirm(
                new Transaction().add(
                    await sdk.admin(admin.publicKey).fundTreasury(new BN(1_000_000_000))
                )
            )
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

            const entry = await sdk.fetchInvestorRecord(investor1.publicKey);
            assert.ok(entry!.investor.equals(investor1.publicKey));
            assert.ok(entry!.usdcDeposited.eq(INVESTOR1_USDC));

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

            const entry = await sdk.fetchInvestorRecord(investor2.publicKey);
            assert.ok(entry!.investor.equals(investor2.publicKey));
            assert.ok(entry!.usdcDeposited.eq(INVESTOR2_USDC));

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

            const entry = await sdk.fetchInvestorRecord(investor1.publicKey);
            assert.ok(entry!.usdcDeposited.eq(INVESTOR1_USDC));
        });

        it("allows deposit when contract is paused", async () => {
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
            } finally {
                await provider.sendAndConfirm(
                    new Transaction().add(await sdk.admin(admin.publicKey).setPaused(false))
                );
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

            const entry = await sdk.fetchInvestorRecord(investor1.publicKey);
            assert.ok(entry!.usdcDeposited.eq(new BN(4_900 * USDC_UNIT)));

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

        it("allows withdrawal when contract is paused", async () => {
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
            } finally {
                await provider.sendAndConfirm(
                    new Transaction().add(await sdk.admin(admin.publicKey).setPaused(false))
                );
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
            }
        });

        it("investor cannot withdraw more than deposited (unlocked)", async () => {
            const entry = await sdk.fetchInvestorRecord(investor1.publicKey);
            const tooMuch = entry!.usdcDeposited.addn(1);

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
            // investor2 signs the tx — the pool handler looks up by investor.key(),
            // so investor2 can only withdraw their own record, never investor1's.
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

            const entry = await sdk.fetchInvestorRecord(investor2.publicKey);
            assert.ok(entry!.investor.equals(investor2.publicKey));

            // investor1's record must be untouched
            const entry1 = await sdk.fetchInvestorRecord(investor1.publicKey);
            assert.ok(entry1!.usdcDeposited.eq(INVESTOR1_USDC));

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
            assert.equal(stateBefore.roundStarted, 0);

            const sellerUsdcBefore = await getTokenBalance(provider, sellerUsdcAcc);
            const walnVaultBefore = await getTokenBalance(provider, walnVault, TOKEN_2022_PROGRAM_ID);

            const partialSig = await provider.sendAndConfirm(
                new Transaction().add(
                    await sdk.sellWalnIx({
                        seller: seller.publicKey,
                        sellerWalnAccount: sellerWalnAcc,
                        sellerUsdcAccount: sellerUsdcAcc,
                        walnMint,
                        usdcMint,
                        walnTokenProgram: TOKEN_2022_PROGRAM_ID,
                        walnAmount: SELL_AMOUNT_PARTIAL,
                    })
                ),
                [seller]
            );

            await sleep(1000);
            const partialTx = await provider.connection.getTransaction(partialSig, {
                commitment: "confirmed",
                maxSupportedTransactionVersion: 0
            });
            console.log(`    [CU] sell_waln PARTIAL (round-start, 2 investors): ${partialTx?.meta?.computeUnitsConsumed}`);

            const sellerUsdcAfter = await getTokenBalance(provider, sellerUsdcAcc);
            const walnVaultAfter = await getTokenBalance(provider, walnVault, TOKEN_2022_PROGRAM_ID);

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
            assert.equal(state.roundStarted, 1);

            // investor1: min(5e9, floor(20e6 * 100000 / 150000)) = 13_333_333
            const entry1 = await sdk.fetchInvestorRecord(investor1.publicKey);
            assert.ok(entry1!.usdcLockedCurrentRound.eq(new BN(13_333_333)));
            assert.ok(entry1!.usdcDeposited.eq(new BN(4_986_666_667)));

            // investor2: min(5e9, floor(20e6 * 50000 / 150000)) = 6_666_666
            const entry2 = await sdk.fetchInvestorRecord(investor2.publicKey);
            assert.ok(entry2!.usdcLockedCurrentRound.eq(new BN(6_666_666)));
            assert.ok(entry2!.usdcDeposited.eq(new BN(4_993_333_334)));
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
                            walnTokenProgram: TOKEN_2022_PROGRAM_ID,
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
            const [roundLockedWaln0] = sdk.roundLockedWalnPda(round0);

            const sellerUsdcBefore = await getTokenBalance(provider, sellerUsdcAcc);

            const triggerSig = await provider.sendAndConfirm(
                new Transaction().add(
                    await sdk.sellWalnIx({
                        seller: seller.publicKey,
                        sellerWalnAccount: sellerWalnAcc,
                        sellerUsdcAccount: sellerUsdcAcc,
                        walnMint,
                        usdcMint,
                        walnTokenProgram: TOKEN_2022_PROGRAM_ID,
                        walnAmount: SELL_AMOUNT_TRIGGER,
                        roundTriggerAccounts: [
                            {pubkey: roundRecord0, isWritable: true},
                            {pubkey: roundLockedWaln0, isWritable: true},
                        ],
                    })
                ),
                [seller]
            );

            await sleep(1000);
            const triggerTx = await provider.connection.getTransaction(triggerSig, {
                commitment: "confirmed",
                maxSupportedTransactionVersion: 0
            });
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
            assert.equal(state.roundStarted, 1); // auto-started next round

            // ---- verify RoundLockedWaln records ----
            const lw1 = await sdk.fetchInvestorAlloc(round0, investor1.publicKey);
            assert.ok(lw1!.investor.equals(investor1.publicKey));
            assert.equal(lw1!.walnAmount, 133333330000n);
            assert.equal(lw1!.claimed, false);

            const lw2 = await sdk.fetchInvestorAlloc(round0, investor2.publicKey);
            assert.ok(lw2!.investor.equals(investor2.publicKey));
            assert.equal(lw2!.walnAmount, 66666660000n);
            assert.equal(lw2!.claimed, false);

            const entry1 = await sdk.fetchInvestorRecord(investor1.publicKey);
            assert.ok(entry1!.usdcLockedCurrentRound.eq(new BN(13_333_333)));
            assert.ok(entry1!.usdcDeposited.eq(new BN(4_973_333_334)));

            const entry2 = await sdk.fetchInvestorRecord(investor2.publicKey);
            assert.ok(entry2!.usdcLockedCurrentRound.eq(new BN(6_666_666)));
            assert.ok(entry2!.usdcDeposited.eq(new BN(4_986_666_668)));

            assert.ok(entry1!.usdcCommitted.eq(new BN(13_333_333)));
            assert.ok(entry1!.walnPurchasedTotal.eq(new BN("133333330000")));
            assert.ok(entry2!.usdcCommitted.eq(new BN(6_666_666)));
            assert.ok(entry2!.walnPurchasedTotal.eq(new BN("66666660000")));
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
            const walnBefore = await getTokenBalance(provider, investor1WalnAcc, TOKEN_2022_PROGRAM_ID);

            await provider.sendAndConfirm(
                new Transaction().add(
                    await sdk.claimWalnIx({
                        investor: investor1.publicKey,
                        investorWalnAccount: investor1WalnAcc,
                        walnMint,
                        walnTokenProgram: TOKEN_2022_PROGRAM_ID,
                        roundIndex: round0,
                    })
                ),
                [investor1]
            );

            const walnAfter = await getTokenBalance(provider, investor1WalnAcc, TOKEN_2022_PROGRAM_ID);
            assert.equal(walnAfter - walnBefore, 133_333_330_000n);

            const lw = await sdk.fetchInvestorAlloc(round0, investor1.publicKey);
            assert.equal(lw!.claimed, true);
        });

        it("allows claim attempt when contract is paused (fails for other reason, not ContractPaused)", async () => {
            await provider.sendAndConfirm(
                new Transaction().add(await sdk.admin(admin.publicKey).setPaused(true))
            );
            try {
                await provider.sendAndConfirm(
                    new Transaction().add(
                        await sdk.claimWalnIx({
                            investor: investor1.publicKey,
                            investorWalnAccount: investor1WalnAcc,
                            walnMint,
                            walnTokenProgram: TOKEN_2022_PROGRAM_ID,
                            roundIndex: new BN(0),
                        })
                    ),
                    [investor1]
                );
                assert.fail("should have thrown");
            } catch (e: any) {
                assert.include(e.toString(), "AlreadyClaimed");
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
                            walnTokenProgram: TOKEN_2022_PROGRAM_ID,
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
            const walnBefore = await getTokenBalance(provider, investor2WalnAcc, TOKEN_2022_PROGRAM_ID);

            await provider.sendAndConfirm(
                new Transaction().add(
                    await sdk.claimWalnIx({
                        investor: investor2.publicKey,
                        investorWalnAccount: investor2WalnAcc,
                        walnMint,
                        walnTokenProgram: TOKEN_2022_PROGRAM_ID,
                        roundIndex: round0,
                    })
                ),
                [investor2]
            );

            const walnAfter = await getTokenBalance(provider, investor2WalnAcc, TOKEN_2022_PROGRAM_ID);
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
            const entry1 = await sdk.fetchInvestorRecord(investor1.publicKey);
            const entry2 = await sdk.fetchInvestorRecord(investor2.publicKey);
            const sumPositions = BigInt(
                entry1!.usdcDeposited.toNumber() +
                entry1!.usdcLockedCurrentRound.toNumber() +
                entry2!.usdcDeposited.toNumber() +
                entry2!.usdcLockedCurrentRound.toNumber()
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
            const entry1 = await sdk.fetchInvestorRecord(investor1.publicKey);
            const entry2 = await sdk.fetchInvestorRecord(investor2.publicKey);

            const sumLobby =
                entry1!.usdcDeposited.toNumber() +
                entry1!.usdcLockedCurrentRound.toNumber() +
                entry2!.usdcDeposited.toNumber() +
                entry2!.usdcLockedCurrentRound.toNumber();

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

    describe("sell cap — no overshoot allowed", () => {
        before(async () => {
            await mintTokensTo(provider, walnMint, sellerWalnAcc, BigInt(500 * WALN_UNIT), TOKEN_2022_PROGRAM_ID);

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
                            walnTokenProgram: TOKEN_2022_PROGRAM_ID,
                            walnAmount: new BN(201 * WALN_UNIT),
                            roundTriggerAccounts: [
                                {pubkey: sdk.roundRecordPda(new BN(2))[0], isWritable: true},
                                {pubkey: sdk.roundLockedWalnPda(new BN(2))[0], isWritable: true},
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
            assert.ok(state.roundCount.eqn(1));
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
                        walnTokenProgram: TOKEN_2022_PROGRAM_ID,
                        walnAmount: new BN(100 * WALN_UNIT),
                    })
                ),
                [seller]
            );

            const state = await sdk.program.account.programState.fetch(contractState);
            assert.ok(state.currentRoundWaln.eq(new BN(100 * WALN_UNIT)), "100 wALN accumulated");
            assert.ok(state.roundCount.eqn(1), "round not triggered yet");
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
                            walnTokenProgram: TOKEN_2022_PROGRAM_ID,
                            walnAmount: new BN(101 * WALN_UNIT),
                            roundTriggerAccounts: [
                                {pubkey: sdk.roundRecordPda(new BN(2))[0], isWritable: true},
                                {pubkey: sdk.roundLockedWalnPda(new BN(2))[0], isWritable: true},
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
            const [roundLockedWaln] = sdk.roundLockedWalnPda(roundBn);

            await provider.sendAndConfirm(
                new Transaction().add(
                    await sdk.sellWalnIx({
                        seller: seller.publicKey,
                        sellerWalnAccount: sellerWalnAcc,
                        sellerUsdcAccount: sellerUsdcAcc,
                        walnMint,
                        usdcMint,
                        walnTokenProgram: TOKEN_2022_PROGRAM_ID,
                        walnAmount: new BN(100 * WALN_UNIT),
                        roundTriggerAccounts: [
                            {pubkey: roundRecord, isWritable: true},
                            {pubkey: roundLockedWaln, isWritable: true},
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
            assert.equal(stateAfter.roundStarted, 1, "next round auto-started");
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
            assert.equal(state.roundStarted, 1, "round 3 is active");
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
                        walnTokenProgram: TOKEN_2022_PROGRAM_ID,
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
            const [roundLockedWaln] = sdk.roundLockedWalnPda(roundBn);

            await provider.sendAndConfirm(
                new Transaction().add(
                    ComputeBudgetProgram.setComputeUnitLimit({units: 400_000}),
                    await sdk.sellWalnIx({
                        seller: seller.publicKey,
                        sellerWalnAccount: sellerWalnAcc,
                        sellerUsdcAccount: sellerUsdcAcc,
                        walnMint,
                        usdcMint,
                        walnTokenProgram: TOKEN_2022_PROGRAM_ID,
                        walnAmount: new BN(100 * WALN_UNIT),
                        roundTriggerAccounts: [
                            {pubkey: roundRecord, isWritable: true},
                            {pubkey: roundLockedWaln, isWritable: true},
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
            assert.equal(stateAfter.roundStarted, 1, "next round auto-started with new price");
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

            await mintTokensTo(provider, walnMint, sellerWalnAcc, BigInt(200 * WALN_UNIT), TOKEN_2022_PROGRAM_ID);
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
            assert.equal(state.roundStarted, 1, "round 4 is active");
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
                            walnTokenProgram: TOKEN_2022_PROGRAM_ID,
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
            const [roundLockedWaln] = sdk.roundLockedWalnPda(roundBn);

            await provider.sendAndConfirm(
                new Transaction().add(
                    ComputeBudgetProgram.setComputeUnitLimit({units: 400_000}),
                    await sdk.sellWalnIx({
                        seller: seller.publicKey,
                        sellerWalnAccount: sellerWalnAcc,
                        sellerUsdcAccount: sellerUsdcAcc,
                        walnMint,
                        usdcMint,
                        walnTokenProgram: TOKEN_2022_PROGRAM_ID,
                        walnAmount: new BN(200 * WALN_UNIT),
                        roundTriggerAccounts: [
                            {pubkey: roundRecord, isWritable: true},
                            {pubkey: roundLockedWaln, isWritable: true},
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
            assert.equal(stateAfter.roundStarted, 1, "next round auto-started with new round size");
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

            await mintTokensTo(provider, walnMint, sellerWalnAcc, BigInt(1000 * WALN_UNIT), TOKEN_2022_PROGRAM_ID);
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
            const [roundLockedWaln] = sdk.roundLockedWalnPda(roundBn);

            const walnInRound = BigInt(stateBefore.currentRoundSizeWaln.toString());

            await provider.sendAndConfirm(
                new Transaction().add(
                    ComputeBudgetProgram.setComputeUnitLimit({units: 400_000}),
                    await sdk.sellWalnIx({
                        seller: seller.publicKey,
                        sellerWalnAccount: sellerWalnAcc,
                        sellerUsdcAccount: sellerUsdcAcc,
                        walnMint,
                        usdcMint,
                        walnTokenProgram: TOKEN_2022_PROGRAM_ID,
                        walnAmount: new BN(stateBefore.currentRoundSizeWaln.toString()),
                        roundTriggerAccounts: [
                            {pubkey: roundRecord, isWritable: true},
                            {pubkey: roundLockedWaln, isWritable: true},
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

            const entry1 = await sdk.fetchInvestorRecord(investor1.publicKey);
            const entry2 = await sdk.fetchInvestorRecord(investor2.publicKey);
            const locked1 = BigInt(entry1!.usdcLockedCurrentRound.toString());
            const locked2 = BigInt(entry2!.usdcLockedCurrentRound.toString());

            const roundIndex = stateBefore.roundCount;
            const roundBn = new BN(roundIndex.toNumber());
            const [roundRecord] = sdk.roundRecordPda(roundBn);
            const [roundLockedWaln] = sdk.roundLockedWalnPda(roundBn);

            await provider.sendAndConfirm(
                new Transaction().add(
                    ComputeBudgetProgram.setComputeUnitLimit({units: 400_000}),
                    await sdk.sellWalnIx({
                        seller: seller.publicKey,
                        sellerWalnAccount: sellerWalnAcc,
                        sellerUsdcAccount: sellerUsdcAcc,
                        walnMint,
                        usdcMint,
                        walnTokenProgram: TOKEN_2022_PROGRAM_ID,
                        walnAmount: new BN(stateBefore.currentRoundSizeWaln.toString()),
                        roundTriggerAccounts: [
                            {pubkey: roundRecord, isWritable: true},
                            {pubkey: roundLockedWaln, isWritable: true},
                        ],
                    })
                ),
                [seller]
            );

            const lw1 = await sdk.fetchInvestorAlloc(roundBn, investor1.publicKey);
            const lw2 = await sdk.fetchInvestorAlloc(roundBn, investor2.publicKey);

            const base1 = locked1 * walnScale / floorPrice;
            const base2 = locked2 * walnScale / floorPrice;

            const inv1GotDust = lw1!.walnAmount > base1;
            const inv2GotDust = lw2!.walnAmount > base2;

            assert.ok(
                inv1GotDust !== inv2GotDust,
                "exactly one investor should receive the dust bonus"
            );

            if (inv1GotDust) {
                assert.equal(
                    lw1!.walnAmount,
                    base1 + dustCarryover,
                    "dust winner (investor1) should receive base + full dust"
                );
                assert.equal(
                    lw2!.walnAmount,
                    base2,
                    "investor2 should receive only base allocation"
                );
            } else {
                assert.equal(
                    lw2!.walnAmount,
                    base2 + dustCarryover,
                    "dust winner (investor2) should receive base + full dust"
                );
                assert.equal(
                    lw1!.walnAmount,
                    base1,
                    "investor1 should receive only base allocation"
                );
            }

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

    // ---------------------------------------------------------------------------
    // 15. cancel_round
    // ---------------------------------------------------------------------------
    describe("cancel_round", () => {
        it("non-admin cannot cancel a round", async () => {
            const state = await sdk.program.account.programState.fetch(contractState);
            if (state.roundStarted !== 1) {
                return;
            }
            try {
                await provider.sendAndConfirm(
                    new Transaction().add(
                        await sdk.admin(investor1.publicKey).cancelRound()
                    ),
                    [investor1]
                );
                assert.fail("should have rejected non-admin cancel");
            } catch (e: any) {
                assert.ok(
                    e.message.includes("Unauthorized") || e.logs?.some((l: string) => l.includes("Unauthorized")),
                    "expected Unauthorized error"
                );
            }
        });

        it("cancel_round fails when no round is active", async () => {
            const state = await sdk.program.account.programState.fetch(contractState);
            if (state.roundStarted === 1) {
                return;
            }
            try {
                await provider.sendAndConfirm(
                    new Transaction().add(
                        await sdk.admin(admin.publicKey).cancelRound()
                    )
                );
                assert.fail("should have rejected cancel when no round active");
            } catch (e: any) {
                assert.ok(
                    e.message.includes("InvalidParameter") || e.logs?.some((l: string) => l.includes("InvalidParameter")),
                    "expected InvalidParameter error"
                );
            }
        });

        it("admin can cancel an active round and investors get funds unlocked", async () => {
            const stateBefore = await sdk.program.account.programState.fetch(contractState);
            if (stateBefore.roundStarted !== 1) {
                return;
            }

            const entry1Before = await sdk.fetchInvestorRecord(investor1.publicKey);
            const entry2Before = await sdk.fetchInvestorRecord(investor2.publicKey);

            const locked1 = BigInt(entry1Before!.usdcLockedCurrentRound.toString());
            const locked2 = BigInt(entry2Before!.usdcLockedCurrentRound.toString());
            const deposited1Before = BigInt(entry1Before!.usdcDeposited.toString());
            const deposited2Before = BigInt(entry2Before!.usdcDeposited.toString());

            assert.ok(locked1 > 0n || locked2 > 0n, "at least one investor should have locked funds");

            await provider.sendAndConfirm(
                new Transaction().add(
                    await sdk.admin(admin.publicKey).cancelRound()
                )
            );

            const stateAfter = await sdk.program.account.programState.fetch(contractState);
            assert.equal(stateAfter.roundStarted, 0, "round_started must be 0 after cancel");
            assert.equal(stateAfter.currentRoundWaln.toNumber(), 0, "current_round_waln must be 0 after cancel");
            assert.equal(stateAfter.totalUsdcLockedForRound.toNumber(), 0, "total_usdc_locked_for_round must be 0 after cancel");

            const entry1After = await sdk.fetchInvestorRecord(investor1.publicKey);
            const entry2After = await sdk.fetchInvestorRecord(investor2.publicKey);

            assert.equal(
                BigInt(entry1After!.usdcLockedCurrentRound.toString()),
                0n,
                "investor1 usdc_locked_current_round must be 0 after cancel"
            );
            assert.equal(
                BigInt(entry2After!.usdcLockedCurrentRound.toString()),
                0n,
                "investor2 usdc_locked_current_round must be 0 after cancel"
            );
            assert.equal(
                BigInt(entry1After!.usdcDeposited.toString()),
                deposited1Before + locked1,
                "investor1 usdc_deposited must be restored by locked amount"
            );
            assert.equal(
                BigInt(entry2After!.usdcDeposited.toString()),
                deposited2Before + locked2,
                "investor2 usdc_deposited must be restored by locked amount"
            );
        });

        it("investors can withdraw after round is cancelled", async () => {
            const entry1 = await sdk.fetchInvestorRecord(investor1.publicKey);
            const available = BigInt(entry1!.usdcDeposited.toString());
            if (available === 0n) {
                return;
            }

            const balanceBefore = await getTokenBalance(provider, investor1UsdcAcc);

            await provider.sendAndConfirm(
                new Transaction().add(
                    await sdk.withdrawUsdcIx({
                        investor: investor1.publicKey,
                        investorUsdcAccount: investor1UsdcAcc,
                        usdcMint,
                        amount: new BN(available.toString()),
                    })
                ),
                [investor1]
            );

            const balanceAfter = await getTokenBalance(provider, investor1UsdcAcc);
            assert.equal(
                balanceAfter - balanceBefore,
                available,
                "investor1 should receive their full available USDC balance"
            );
        });
    });

    // ---------------------------------------------------------------------------
    // 100-investor scale test
    // ---------------------------------------------------------------------------
    describe.skip("100-investor pool scale test", () => {
        const NUM_NEW = 100;

        interface NewInvestor {
            keypair: Keypair;
            usdcAcc: PublicKey;
            walnAcc: PublicKey;
            nftPubkey: PublicKey;
        }

        const newInvestors: NewInvestor[] = [];

        before(async () => {
            // 1. Create 100 new investor keypairs
            for (let i = 0; i < NUM_NEW; i++) {
                newInvestors.push({
                    keypair: Keypair.generate(),
                    usdcAcc: null!,
                    walnAcc: null!,
                    nftPubkey: null!,
                });
            }

            // 2. Airdrop SOL to each new investor
            await Promise.all(
                newInvestors.map(async ({keypair}) => {
                    const sig = await provider.connection.requestAirdrop(
                        keypair.publicKey,
                        2_000_000_000
                    );
                    await provider.connection.confirmTransaction(sig, "confirmed");
                })
            );

            // 3. Create token accounts and mint USDC to each
            for (const inv of newInvestors) {
                inv.usdcAcc = await createTestTokenAccount(
                    provider,
                    usdcMint,
                    inv.keypair.publicKey
                );
                inv.walnAcc = await createTestTokenAccount(
                    provider,
                    walnMint,
                    inv.keypair.publicKey
                );
                await mintTokensTo(
                    provider,
                    usdcMint,
                    inv.usdcAcc,
                    BigInt(5_000 * USDC_UNIT)
                );
            }

            // 4. Mint AAT NFTs for each new investor (aatVolume=1000 each → 100*1000=100000 extra)
            for (const inv of newInvestors) {
                [inv.nftPubkey] = sdk.aatNftMintPda(inv.keypair.publicKey);
                await provider.sendAndConfirm(
                    new Transaction().add(
                        ComputeBudgetProgram.setComputeUnitLimit({units: 400_000}),
                        await sdk.mintAatNftIx({
                            admin: admin.publicKey,
                            investor: inv.keypair.publicKey,
                            aatVolume: new BN(1_000),
                        })
                    )
                );
            }

            // 5. Each new investor deposits USDC
            for (const inv of newInvestors) {
                await provider.sendAndConfirm(
                    new Transaction().add(
                        await sdk.depositUsdcIx({
                            investor: inv.keypair.publicKey,
                            investorUsdcAccount: inv.usdcAcc,
                            usdcMint,
                            aatNft: inv.nftPubkey,
                            usdcAmount: new BN(2_000 * USDC_UNIT),
                        })
                    ),
                    [inv.keypair]
                );
            }

            // 6. Mint extra wALN to seller
            await mintTokensTo(
                provider,
                walnMint,
                sellerWalnAcc,
                BigInt(600 * WALN_UNIT)
            );

            // 7. Complete the current active round (only investor1 and investor2 are locked).
            //    After this trigger, round N+1 auto-starts and locks all 102 investors.
            const stateNow = await sdk.program.account.programState.fetch(contractState);
            const currentRoundIdx = new BN(stateNow.roundCount.toString());
            const [roundRecordNow] = sdk.roundRecordPda(currentRoundIdx);
            const [roundLockedWalnNow] = sdk.roundLockedWalnPda(currentRoundIdx);

            const remainingToSell = new BN(
                stateNow.currentRoundSizeWaln.toString()
            ).sub(new BN(stateNow.currentRoundWaln.toString()));

            await provider.sendAndConfirm(
                new Transaction().add(
                    ComputeBudgetProgram.setComputeUnitLimit({units: 400_000}),
                    await sdk.sellWalnIx({
                        seller: seller.publicKey,
                        sellerWalnAccount: sellerWalnAcc,
                        sellerUsdcAccount: sellerUsdcAcc,
                        walnMint,
                        usdcMint,
                        walnTokenProgram: TOKEN_2022_PROGRAM_ID,
                        walnAmount: remainingToSell,
                        roundTriggerAccounts: [
                            {pubkey: roundRecordNow, isWritable: true},
                            {pubkey: roundLockedWalnNow, isWritable: true},
                        ],
                    })
                ),
                [seller]
            );
        });

        it("triggers round end with all 101 investors (100 new + 1 original)", async () => {
            const stateBefore = await sdk.program.account.programState.fetch(contractState);
            assert.equal(stateBefore.roundStarted, 1, "round should be auto-started");

            const roundIdx = new BN(stateBefore.roundCount.toString());
            const [roundRecord] = sdk.roundRecordPda(roundIdx);
            const [roundLockedWaln] = sdk.roundLockedWalnPda(roundIdx);

            const roundSizeWaln = new BN(stateBefore.currentRoundSizeWaln.toString());

            const sig = await provider.sendAndConfirm(
                new Transaction().add(
                    ComputeBudgetProgram.setComputeUnitLimit({units: 300_000}),
                    await sdk.sellWalnIx({
                        seller: seller.publicKey,
                        sellerWalnAccount: sellerWalnAcc,
                        sellerUsdcAccount: sellerUsdcAcc,
                        walnMint,
                        usdcMint,
                        walnTokenProgram: TOKEN_2022_PROGRAM_ID,
                        walnAmount: roundSizeWaln,
                        roundTriggerAccounts: [
                            {pubkey: roundRecord, isWritable: true},
                            {pubkey: roundLockedWaln, isWritable: true},
                        ],
                    })
                ),
                [seller]
            );

            await sleep(1000);
            const txInfo = await provider.connection.getTransaction(sig, {
                commitment: "confirmed",
                maxSupportedTransactionVersion: 0,
            });
            console.log(
                `    [CU] sell_waln 101-investor round trigger: ${txInfo?.meta?.computeUnitsConsumed}`
            );

            // Verify round record was created with 102 participants
            const rr = await sdk.fetchRoundRecord(roundIdx);
            assert.equal(
                rr.participantCount,
                101,
                "all 101 investors (1 original + 100 new) should participate"
            );

            // Verify round count incremented
            const stateAfter = await sdk.program.account.programState.fetch(contractState);
            const expectedRoundCount = new BN(stateBefore.roundCount.toString()).addn(1);
            assert.ok(
                new BN(stateAfter.roundCount.toString()).eq(expectedRoundCount),
                "round_count should be incremented"
            );

            // Verify each new investor received a walnAmount > 0 in the RoundLockedWaln account
            for (let i = 0; i < newInvestors.length; i++) {
                const inv = newInvestors[i];
                const alloc = await sdk.fetchInvestorAlloc(roundIdx, inv.keypair.publicKey);
                assert.ok(alloc !== null, `new investor[${i}] has an allocation`);
                assert.ok(alloc!.walnAmount > 0n, `new investor[${i}] has walnAmount > 0`);
                assert.equal(alloc!.claimed, false, `new investor[${i}] not yet claimed`);
            }

            // Verify pool state via InvestorRecord entries
            const pool = await sdk.fetchInvestorPool();
            assert.ok(pool.count >= 101, "pool should hold at least 101 investors");
        });
    });
});
