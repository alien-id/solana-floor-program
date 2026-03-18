use anchor_lang::prelude::*;
use std::collections::BTreeSet;

use crate::errors::FloorError;
use crate::nft_utils::verify_aat_nft_and_get_allocation;
use crate::seeds::LOBBY_ENTRY_SEED;
use crate::state::LobbyEntry;

/// Executes Round Start over a slice of investor triplets
/// (LobbyEntry, LockedWaln placeholder, Core/Token-2022 mint Asset).
///
/// Returns (total_aat_volume, total_usdc_locked) summed across all eligible investors.
pub fn execute_round_start<'info>(
    triplets: &'info [AccountInfo<'info>],
    round_size_waln: u64,
    floor_price_usdc: u64,
    waln_decimals: u8,
) -> Result<(u64, u64)> {
    let waln_scale = 10_u128.pow(waln_decimals as u32);
    let round_cap_usdc = (round_size_waln as u128)
        .checked_mul(floor_price_usdc as u128)
        .ok_or(FloorError::ArithmeticOverflow)?
        .checked_div(waln_scale)
        .ok_or(FloorError::ArithmeticOverflow)?;

    // Pass 1: compute total_aat_volume across all eligible investors.
    // Each investor must appear exactly once; duplicate triplets are rejected.
    let mut total_aat_volume: u64 = 0;
    let mut seen_investors: BTreeSet<Pubkey> = BTreeSet::new();
    let mut i = 0;
    while i + 2 < triplets.len() {
        let lobby_entry_info = &triplets[i];
        let core_asset_info = &triplets[i + 2];
        i += 3;

        let lobby_entry: Account<LobbyEntry> = match Account::try_from(lobby_entry_info) {
            Ok(e) => e,
            Err(_) => continue,
        };

        let expected_pda = Pubkey::create_program_address(
            &[LOBBY_ENTRY_SEED, lobby_entry.investor.as_ref(), &[lobby_entry.bump]],
            &crate::ID,
        )
        .map_err(|_| error!(FloorError::InvalidRemainingAccounts))?;
        require!(
            expected_pda == lobby_entry_info.key(),
            FloorError::InvalidRemainingAccounts
        );

        require!(
            seen_investors.insert(lobby_entry.investor),
            FloorError::InvalidRemainingAccounts
        );

        if lobby_entry.usdc_deposited == 0 {
            continue;
        }

        let alloc = match verify_aat_nft_and_get_allocation(
            core_asset_info,
            &lobby_entry.investor,
        ) {
            Ok(a) if a > 0 => a,
            _ => continue,
        };

        total_aat_volume = total_aat_volume
            .checked_add(alloc)
            .ok_or(FloorError::ArithmeticOverflow)?;
    }

    require!(total_aat_volume > 0, FloorError::NoEligibleInvestors);

    // Pass 2: compute and persist usdc_locked_current_round for each eligible investor.
    let mut total_usdc_locked: u64 = 0;
    let mut i = 0;
    while i + 2 < triplets.len() {
        let lobby_entry_info = &triplets[i];
        let core_asset_info = &triplets[i + 2];
        i += 3;

        let mut lobby_entry: Account<LobbyEntry> = match Account::try_from(lobby_entry_info) {
            Ok(e) => e,
            Err(_) => continue,
        };

        let expected_pda = Pubkey::create_program_address(
            &[LOBBY_ENTRY_SEED, lobby_entry.investor.as_ref(), &[lobby_entry.bump]],
            &crate::ID,
        )
        .map_err(|_| error!(FloorError::InvalidRemainingAccounts))?;
        require!(
            expected_pda == lobby_entry_info.key(),
            FloorError::InvalidRemainingAccounts
        );

        if lobby_entry.usdc_deposited == 0 {
            continue;
        }

        let alloc = match verify_aat_nft_and_get_allocation(
            core_asset_info,
            &lobby_entry.investor,
        ) {
            Ok(a) if a > 0 => a,
            _ => continue,
        };

        let usdc_locked_u128 = round_cap_usdc
            .checked_mul(alloc as u128)
            .ok_or(FloorError::ArithmeticOverflow)?
            .checked_div(total_aat_volume as u128)
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

        total_usdc_locked = total_usdc_locked
            .checked_add(usdc_locked)
            .ok_or(FloorError::ArithmeticOverflow)?;

        lobby_entry.exit(&crate::ID)?;
    }

    Ok((total_aat_volume, total_usdc_locked))
}
