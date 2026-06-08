use anchor_lang::prelude::*;
use spl_token_2022::extension::transfer_hook::TransferHook;
use spl_token_2022::extension::{BaseStateWithExtensions, StateWithExtensions};
use spl_token_2022::state::Mint;
use spl_token_metadata_interface::state::TokenMetadata;

use crate::errors::FloorError;
use crate::seeds::AAT_NFT_SEED;

/// Verifies that `aat_nft_mint_info` is the deterministic PDA mint for `investor`
/// (owned by Token-2022), then returns the `aat_volume` value from its
/// TokenMetadata additional_metadata field.
pub fn verify_aat_nft_and_get_allocation(
    aat_nft_mint_info: &AccountInfo,
    investor: &Pubkey,
) -> Result<u64> {
    let (expected_pda, _) =
        Pubkey::find_program_address(&[AAT_NFT_SEED, investor.as_ref()], &crate::ID);
    require!(
        expected_pda == aat_nft_mint_info.key(),
        FloorError::InvalidAatNft
    );

    require!(
        aat_nft_mint_info.owner == &spl_token_2022::ID,
        FloorError::InvalidAatNft
    );

    let data = aat_nft_mint_info.data.borrow();
    let state = StateWithExtensions::<Mint>::unpack(&data)
        .map_err(|_| error!(FloorError::InvalidAatNft))?;
    let metadata = state
        .get_variable_len_extension::<TokenMetadata>()
        .map_err(|_| error!(FloorError::InvalidAatNft))?;

    for (key, value) in &metadata.additional_metadata {
        if key == "aat_volume" {
            return value
                .parse::<u64>()
                .map_err(|_| error!(FloorError::InvalidAatNft));
        }
    }

    Err(error!(FloorError::InvalidAatNft))
}

/// Closes a program-owned account by draining its lamports into `treasury` and
/// reassigning it to the System Program. Idempotent: a no-op when the account is not
/// owned by this program or already has zero lamports (e.g. it was closed earlier),
/// so callers can invoke it unconditionally on the round-settlement path.
pub fn close_to_treasury(account: &AccountInfo, treasury: &AccountInfo) -> Result<()> {
    if account.owner != &crate::ID || account.lamports() == 0 {
        return Ok(());
    }
    let dest_starting = treasury.lamports();
    **treasury.try_borrow_mut_lamports()? = dest_starting
        .checked_add(account.lamports())
        .ok_or(FloorError::ArithmeticOverflow)?;
    **account.try_borrow_mut_lamports()? = 0;
    account.assign(&System::id());
    account.resize(0)?;
    Ok(())
}

pub fn get_hook_program_id(mint_info: &AccountInfo) -> Result<Pubkey> {
    let data = mint_info.try_borrow_data()?;
    let mint = StateWithExtensions::<spl_token_2022::state::Mint>::unpack(&data)?;
    let hook = mint.get_extension::<TransferHook>()?;
    Option::<Pubkey>::from(hook.program_id).ok_or(FloorError::InvalidHookAccounts.into())
}

pub fn validate_hook_accounts(
    remaining: &[AccountInfo],
    mint: &Pubkey,
    owner: &Pubkey,
    hook_program_id: &Pubkey,
) -> Result<()> {
    require!(remaining.len() == 8, FloorError::InvalidHookAccountsCount);

    let (expected_config, _) =
        Pubkey::find_program_address(&[b"config", mint.as_ref()], hook_program_id);
    require!(
        remaining[0].key() == expected_config,
        FloorError::InvalidHookAccounts
    );

    let config_data = remaining[0].try_borrow_data()?;
    require!(config_data.len() >= 136, FloorError::InvalidHookAccounts);
    let credential = Pubkey::from(<[u8; 32]>::try_from(&config_data[40..72]).unwrap());
    let schema = Pubkey::from(<[u8; 32]>::try_from(&config_data[72..104]).unwrap());
    let sas_program = Pubkey::from(<[u8; 32]>::try_from(&config_data[104..136]).unwrap());
    drop(config_data);

    require!(
        remaining[1].key() == credential,
        FloorError::InvalidHookAccounts
    );
    require!(
        remaining[2].key() == schema,
        FloorError::InvalidHookAccounts
    );
    require!(
        remaining[3].key() == sas_program,
        FloorError::InvalidHookAccounts
    );

    let (expected_attestation, _) = Pubkey::find_program_address(
        &[
            b"attestation",
            credential.as_ref(),
            schema.as_ref(),
            owner.as_ref(),
        ],
        &sas_program,
    );
    require!(
        remaining[4].key() == expected_attestation,
        FloorError::InvalidHookAccounts
    );

    let (expected_whitelist, _) = Pubkey::find_program_address(
        &[b"whitelist", mint.as_ref(), owner.as_ref()],
        hook_program_id,
    );
    require!(
        remaining[5].key() == expected_whitelist,
        FloorError::InvalidHookAccounts
    );

    require!(
        remaining[6].key() == *hook_program_id,
        FloorError::InvalidHookAccounts
    );

    let (expected_extra_meta, _) =
        Pubkey::find_program_address(&[b"extra-account-metas", mint.as_ref()], hook_program_id);
    require!(
        remaining[7].key() == expected_extra_meta,
        FloorError::InvalidHookAccounts
    );

    Ok(())
}
