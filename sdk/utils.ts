import {AccountInfo, AccountMeta, Connection, PublicKey} from "@solana/web3.js";
import {
    addExtraAccountMetasForExecute,
    createTransferCheckedInstruction,
    getTransferHook,
    TOKEN_2022_PROGRAM_ID,
    unpackMint
} from "@solana/spl-token";

export async function buildHookAccounts(connection: Connection, owner: PublicKey, MintKeypair: PublicKey): Promise<AccountMeta[]> {
    const mintAccountInfo = await connection.getAccountInfo(
        MintKeypair
    );
    if (!mintAccountInfo) throw new Error("vault mint not found");
    return await getExtraAccountMetasForTransferHook(
        connection,
        MintKeypair,
        mintAccountInfo as AccountInfo<Buffer>,
        owner
    );
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