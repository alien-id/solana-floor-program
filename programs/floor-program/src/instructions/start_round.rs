use anchor_lang::prelude::*;

use crate::errors::FloorError;
use crate::instructions::mint_aat_nft::MAX_TOTAL_AAT_VOLUME;
use crate::state::InvestorRecord;

pub fn execute_round_start(
    investors: &mut [InvestorRecord],
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

    let mut total_aat_volume: u64 = 0;

    for record in investors.iter() {
        if record.usdc_deposited == 0 || record.aat_volume == 0 {
            continue;
        }

        let alloc = record.aat_volume;
        let proportional_share = round_cap_usdc
            .checked_mul(alloc as u128)
            .ok_or(FloorError::ArithmeticOverflow)?
            .checked_div(MAX_TOTAL_AAT_VOLUME as u128)
            .ok_or(FloorError::ArithmeticOverflow)?;
        let min_deposit = proportional_share / 2;
        if (record.usdc_deposited as u128) < min_deposit {
            continue;
        }

        total_aat_volume = total_aat_volume
            .checked_add(alloc)
            .ok_or(FloorError::ArithmeticOverflow)?;
    }

    require!(total_aat_volume > 0, FloorError::NoEligibleInvestors);

    let mut total_usdc_locked: u64 = 0;

    for record in investors.iter_mut() {
        if record.usdc_deposited == 0 || record.aat_volume == 0 {
            continue;
        }

        let alloc = record.aat_volume;
        let proportional_share = round_cap_usdc
            .checked_mul(alloc as u128)
            .ok_or(FloorError::ArithmeticOverflow)?
            .checked_div(MAX_TOTAL_AAT_VOLUME as u128)
            .ok_or(FloorError::ArithmeticOverflow)?;
        let min_deposit = proportional_share / 2;
        if (record.usdc_deposited as u128) < min_deposit {
            continue;
        }

        let usdc_locked_u128 = round_cap_usdc
            .checked_mul(alloc as u128)
            .ok_or(FloorError::ArithmeticOverflow)?
            .checked_div(total_aat_volume as u128)
            .ok_or(FloorError::ArithmeticOverflow)?;

        let usdc_locked = usdc_locked_u128.min(record.usdc_deposited as u128) as u64;

        if usdc_locked == 0 {
            continue;
        }

        record.usdc_deposited = record
            .usdc_deposited
            .checked_sub(usdc_locked)
            .ok_or(FloorError::ArithmeticOverflow)?;

        record.usdc_locked_current_round = usdc_locked;

        total_usdc_locked = total_usdc_locked
            .checked_add(usdc_locked)
            .ok_or(FloorError::ArithmeticOverflow)?;
    }

    Ok((total_aat_volume, total_usdc_locked))
}
