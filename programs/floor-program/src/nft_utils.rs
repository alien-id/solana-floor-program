use anchor_lang::prelude::*;
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
