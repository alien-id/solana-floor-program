import * as anchor from "@coral-xyz/anchor";
import { AnchorProvider } from "@coral-xyz/anchor";
import {
    Keypair,
    PublicKey,
    SystemProgram,
    Transaction,
    sendAndConfirmTransaction,
} from "@solana/web3.js";
import {
    createMint,
    createAccount,
    mintTo,
    getAccount,
    TOKEN_2022_PROGRAM_ID,
    TOKEN_PROGRAM_ID,
    ExtensionType,
    getMintLen,
    createInitializeMintInstruction,
    createInitializeTransferHookInstruction,
} from "@solana/spl-token";

const CONFIRM_OPTIONS = {
    commitment: "confirmed" as const,
    preflightCommitment: "processed" as const,
};

export async function createTestMint(
    provider: AnchorProvider,
    decimals: number = 0
): Promise<PublicKey> {
    return createMint(
        provider.connection,
        (provider.wallet as anchor.Wallet).payer,
        provider.wallet.publicKey,
        null,
        decimals,
        undefined,
        CONFIRM_OPTIONS
    );
}

export async function createTestTokenAccount(
    provider: AnchorProvider,
    mint: PublicKey,
    owner: PublicKey,
    tokenProgram: PublicKey = TOKEN_PROGRAM_ID
): Promise<PublicKey> {
    return createAccount(
        provider.connection,
        (provider.wallet as anchor.Wallet).payer,
        mint,
        owner,
        undefined,
        CONFIRM_OPTIONS,
        tokenProgram
    );
}

export async function mintTokensTo(
    provider: AnchorProvider,
    mint: PublicKey,
    destination: PublicKey,
    amount: bigint,
    tokenProgram: PublicKey = TOKEN_PROGRAM_ID
): Promise<void> {
    await mintTo(
        provider.connection,
        (provider.wallet as anchor.Wallet).payer,
        mint,
        destination,
        provider.wallet.publicKey,
        amount,
        [],
        CONFIRM_OPTIONS,
        tokenProgram
    );
}

export async function getTokenBalance(
    provider: AnchorProvider,
    account: PublicKey,
    tokenProgram: PublicKey = TOKEN_PROGRAM_ID
): Promise<bigint> {
    const info = await getAccount(provider.connection, account, undefined, tokenProgram);
    return info.amount;
}

export async function sleep(ms: number): Promise<void> {
    return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * Creates a Token-2022 mint with the alien_id_transfer_hook TransferHook extension,
 * initializes the hook config with the given credential/schema/SAS program,
 * and initializes the ExtraAccountMetaList.
 */
export async function createToken2022MintWithTransferHook(
    provider: AnchorProvider,
    mintKeypair: Keypair,
    decimals: number,
    hookProgramId: PublicKey,
    credentialPda: PublicKey,
    schemaPda: PublicKey,
    sasProgramId: PublicKey
): Promise<PublicKey> {
    const payer = (provider.wallet as anchor.Wallet).payer;
    const connection = provider.connection;

    const extensions = [ExtensionType.TransferHook];
    const mintLen = getMintLen(extensions);
    const lamports = await connection.getMinimumBalanceForRentExemption(mintLen);

    const createMintTx = new Transaction().add(
        SystemProgram.createAccount({
            fromPubkey: payer.publicKey,
            newAccountPubkey: mintKeypair.publicKey,
            space: mintLen,
            lamports,
            programId: TOKEN_2022_PROGRAM_ID,
        }),
        createInitializeTransferHookInstruction(
            mintKeypair.publicKey,
            payer.publicKey,
            hookProgramId,
            TOKEN_2022_PROGRAM_ID
        ),
        createInitializeMintInstruction(
            mintKeypair.publicKey,
            decimals,
            payer.publicKey,
            null,
            TOKEN_2022_PROGRAM_ID
        )
    );

    await sendAndConfirmTransaction(
        connection,
        createMintTx,
        [payer, mintKeypair],
        CONFIRM_OPTIONS
    );

    const hookProgram = new anchor.Program(
        require("../idl/alien_id_transfer_hook.json"),
        provider
    );

    const [hookConfigPda] = PublicKey.findProgramAddressSync(
        [Buffer.from("config"), mintKeypair.publicKey.toBuffer()],
        hookProgramId
    );

    await (hookProgram.methods as any)
        .initializeConfig(credentialPda, schemaPda, sasProgramId)
        .accounts({
            authority: payer.publicKey,
            config: hookConfigPda,
            mint: mintKeypair.publicKey,
            systemProgram: SystemProgram.programId,
        })
        .rpc(CONFIRM_OPTIONS);

    const [extraAccountMetaListPda] = PublicKey.findProgramAddressSync(
        [Buffer.from("extra-account-metas"), mintKeypair.publicKey.toBuffer()],
        hookProgramId
    );

    await (hookProgram.methods as any)
        .initializeExtraAccountMetaList()
        .accounts({
            payer: payer.publicKey,
            extraAccountMetaList: extraAccountMetaListPda,
            mint: mintKeypair.publicKey,
            config: hookConfigPda,
            systemProgram: SystemProgram.programId,
        })
        .rpc(CONFIRM_OPTIONS);

    return mintKeypair.publicKey;
}
