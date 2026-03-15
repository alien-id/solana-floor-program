use anchor_lang::prelude::*;

use crate::errors::FloorError;
use crate::seeds::LOBBY_ENTRY_SEED;
use crate::state::LobbyEntry;

pub fn execute_round_start<'info>(
    accounts: &'info [AccountInfo<'info>],
    stride: usize,
    total_aat_staked: u64,
    round_size_waln: u64,
    floor_price_usdc: u64,
) -> Result<()> {
    let round_cap_usdc = (round_size_waln as u128)
        .checked_mul(floor_price_usdc as u128)
        .ok_or(FloorError::ArithmeticOverflow)?;

    let mut i = 0;
    while i < accounts.len() {
        let entry_info = &accounts[i];
        i += stride;
        let mut lobby_entry: Account<LobbyEntry> = Account::try_from(entry_info)?;

        let expected_pda = Pubkey::create_program_address(
            &[LOBBY_ENTRY_SEED, lobby_entry.investor.as_ref(), &[lobby_entry.bump]],
            &crate::ID,
        ).map_err(|_| error!(FloorError::InvalidRemainingAccounts))?;
        require!(expected_pda == entry_info.key(), FloorError::InvalidRemainingAccounts);

        if lobby_entry.aat_staked == 0 || lobby_entry.usdc_deposited == 0 {
            continue;
        }

        let usdc_locked_u128 = round_cap_usdc
            .checked_mul(lobby_entry.aat_staked as u128)
            .ok_or(FloorError::ArithmeticOverflow)?
            .checked_div(total_aat_staked as u128)
            .ok_or(FloorError::ArithmeticOverflow)?;

        let usdc_locked = usdc_locked_u128.min(lobby_entry.usdc_deposited as u128) as u64;

        if usdc_locked == 0 {
            continue;
        }

        lobby_entry.usdc_deposited = lobby_entry
            .usdc_deposited
            .checked_sub(usdc_locked)
            .ok_or(FloorError::ArithmeticOverflow)?;

        lobby_entry.usdc_locked_current_round = usdc_locked;

        lobby_entry.exit(&crate::ID)?;
    }

    Ok(())
}
