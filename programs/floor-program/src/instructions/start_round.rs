use anchor_lang::prelude::*;

use crate::errors::FloorError;
use crate::state::{InvestorRecord, MAX_INVESTORS};

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

    require!(round_cap_usdc > 0, FloorError::InvalidParameter);

    let min_deposit = round_cap_usdc / 2;

    let mut eligible = [false; MAX_INVESTORS];
    for (i, record) in investors.iter().enumerate() {
        eligible[i] = record.usdc_deposited > 0
            && record.aat_volume > 0
            && (record.usdc_deposited as u128) >= min_deposit;
    }

    let mut total_aat_volume: u64;
    loop {
        total_aat_volume = 0;
        for (i, record) in investors.iter().enumerate() {
            if eligible[i] {
                total_aat_volume = total_aat_volume
                    .checked_add(record.aat_volume)
                    .ok_or(FloorError::ArithmeticOverflow)?;
            }
        }

        require!(total_aat_volume > 0, FloorError::NoEligibleInvestors);

        let mut changed = false;
        for (i, record) in investors.iter().enumerate() {
            if !eligible[i] {
                continue;
            }
            let required_lock = round_cap_usdc
                .checked_mul(record.aat_volume as u128)
                .ok_or(FloorError::ArithmeticOverflow)?
                .checked_div(total_aat_volume as u128)
                .ok_or(FloorError::ArithmeticOverflow)?;
            if required_lock == 0 || (record.usdc_deposited as u128) < required_lock {
                eligible[i] = false;
                changed = true;
            }
        }

        if !changed {
            break;
        }
    }

    let mut total_usdc_locked: u64 = 0;

    for (i, record) in investors.iter_mut().enumerate() {
        if !eligible[i] {
            continue;
        }

        let usdc_locked_u128 = round_cap_usdc
            .checked_mul(record.aat_volume as u128)
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

    let gap_u128 = round_cap_usdc.saturating_sub(total_usdc_locked as u128);
    if gap_u128 > 0 {
        let gap = u64::try_from(gap_u128).map_err(|_| FloorError::ArithmeticOverflow)?;
        for (i, record) in investors.iter_mut().enumerate() {
            if !eligible[i] {
                continue;
            }
            if record.usdc_deposited >= gap {
                record.usdc_deposited = record
                    .usdc_deposited
                    .checked_sub(gap)
                    .ok_or(FloorError::ArithmeticOverflow)?;
                record.usdc_locked_current_round = record
                    .usdc_locked_current_round
                    .checked_add(gap)
                    .ok_or(FloorError::ArithmeticOverflow)?;
                total_usdc_locked = total_usdc_locked
                    .checked_add(gap)
                    .ok_or(FloorError::ArithmeticOverflow)?;
                break;
            }
        }
    }

    Ok((total_aat_volume, total_usdc_locked))
}
