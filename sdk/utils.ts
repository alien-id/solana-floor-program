import {AccountInfo, AccountMeta, Connection, PublicKey} from "@solana/web3.js";
import {
    addExtraAccountMetasForExecute,
    createTransferCheckedInstruction,
    getTransferHook,
    TOKEN_2022_PROGRAM_ID,
    unpackMint
} from "@solana/spl-token";

export async function buildHookAccounts(connection: Connection, owner: PublicKey, MintKeypair: PublicKey): Promise<AccountMeta[]> {
    const { accounts } = await buildHookAccountsWithBumps(connection, owner, MintKeypair);
    return accounts;
}

export async function buildHookAccountsWithBumps(
    connection: Connection,
    owner: PublicKey,
    mintAddress: PublicKey,
): Promise<{ accounts: AccountMeta[]; bumps: [number, number, number, number] }> {
    const noHook: { accounts: AccountMeta[]; bumps: [number, number, number, number] } =
        { accounts: [], bumps: [0, 0, 0, 0] };

    const mintAccountInfo = await connection.getAccountInfo(mintAddress);
    if (!mintAccountInfo) throw new Error("mint not found");

    if (!mintAccountInfo.owner.equals(TOKEN_2022_PROGRAM_ID)) return noHook;

    const mintState = unpackMint(mintAddress, mintAccountInfo as AccountInfo<Buffer>, mintAccountInfo.owner);
    const transferHook = getTransferHook(mintState);
    if (!transferHook || transferHook.programId.equals(PublicKey.default)) return noHook;

    const hookProgramId = transferHook.programId;

    const [configPda, configBump] = PublicKey.findProgramAddressSync(
        [Buffer.from("config"), mintAddress.toBytes()],
        hookProgramId,
    );

    const configInfo = await connection.getAccountInfo(configPda);
    if (!configInfo || configInfo.data.length < 136) throw new Error("hook config account not found or too small");
    const configData = Buffer.from(configInfo.data);
    const credential = new PublicKey(configData.subarray(40, 72));
    const schema    = new PublicKey(configData.subarray(72, 104));
    const sasProgram = new PublicKey(configData.subarray(104, 136));

    const [, attestationBump] = PublicKey.findProgramAddressSync(
        [Buffer.from("attestation"), credential.toBytes(), schema.toBytes(), owner.toBytes()],
        sasProgram,
    );
    const [, whitelistBump] = PublicKey.findProgramAddressSync(
        [Buffer.from("whitelist"), mintAddress.toBytes(), owner.toBytes()],
        hookProgramId,
    );
    const [, extraMetaBump] = PublicKey.findProgramAddressSync(
        [Buffer.from("extra-account-metas"), mintAddress.toBytes()],
        hookProgramId,
    );

    const accounts = await getExtraAccountMetasForTransferHook(
        connection,
        mintAddress,
        mintAccountInfo as AccountInfo<Buffer>,
        owner,
    );

    return {
        accounts,
        bumps: [configBump, attestationBump, whitelistBump, extraMetaBump],
    };
}

export async function getExtraAccountMetasForTransferHook(
    connection: Connection,
    mintAddress: PublicKey,
    mintAccountInfo: AccountInfo<Buffer>,
    owner: PublicKey
): Promise<AccountMeta[]> {
    if (
        ![TOKEN_2022_PROGRAM_ID.toBase58()].includes(
            mintAccountInfo.owner.toBase58()
        )
    ) {
        return [];
    }
    const mintState = unpackMint(mintAddress, mintAccountInfo, mintAccountInfo.owner);
    const transferHook = getTransferHook(mintState);
    if (!transferHook || transferHook.programId.equals(PublicKey.default)) {
        return [];
    }

    const instruction = createTransferCheckedInstruction(
        PublicKey.default,
        mintAddress,
        PublicKey.default,
        owner,
        BigInt(0),
        mintState.decimals,
        [],
        mintAccountInfo.owner
    );
    await addExtraAccountMetasForExecute(
        connection,
        instruction,
        transferHook.programId,
        PublicKey.default,
        mintAddress,
        PublicKey.default,
        owner,
        BigInt(0)
    );

    const transferHookAccounts = instruction.keys.slice(4);
    if (transferHookAccounts.length === 0) {
        transferHookAccounts.push({
            pubkey: transferHook.programId,
            isSigner: false,
            isWritable: false,
        });
    }
    return transferHookAccounts;
}