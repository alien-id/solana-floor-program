use anchor_lang::prelude::*;
use anchor_lang::solana_program::{program::invoke_signed, system_instruction};

use crate::errors::FloorError;
use crate::seeds::{
    CONTRACT_STATE_SEED, INVESTOR_POOL_SEED, ROUND_LOCKED_WALN_SEED, ROUND_RECORD_SEED,
    TREASURY_SEED,
};
use crate::state::{InvestorAlloc, InvestorPool, ProgramState, RoundLockedWaln, RoundRecord, MAX_INVESTORS};

#[derive(Accounts)]
pub struct CloseRound<'info> {
    pub admin: Signer<'info>,

    #[account(
        mut,
        seeds = [CONTRACT_STATE_SEED],
        bump,
        constraint = contract_state.load()?.admin == admin.key() @ FloorError::Unauthorized,
    )]
    pub contract_state: AccountLoader<'info, ProgramState>,

    #[account(
        mut,
        seeds = [INVESTOR_POOL_SEED],
        bump,
    )]
    pub investor_pool: AccountLoader<'info, InvestorPool>,

    /// CHECK: Treasury PDA — system-owned, funds round account creation
    #[account(
        mut,
        seeds = [TREASURY_SEED],
        bump,
    )]
    pub treasury: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

fn close_round_usdc_spent(
    usdc_paid_for_round: u64,
    locked: u64,
    total_locked_usdc: u128,
    remaining_usdc_remainder: &mut u64,
) -> Result<u64> {
    let mut usdc_spent = u64::try_from(
        (usdc_paid_for_round as u128)
            .checked_mul(locked as u128)
            .ok_or(FloorError::ArithmeticOverflow)?
            .checked_div(total_locked_usdc)
            .ok_or(FloorError::ArithmeticOverflow)?,
    )
    .map_err(|_| FloorError::ArithmeticOverflow)?;
    if *remaining_usdc_remainder > 0 && usdc_spent < locked {
        usdc_spent = usdc_spent
            .checked_add(1)
            .ok_or(FloorError::ArithmeticOverflow)?;
        *remaining_usdc_remainder = remaining_usdc_remainder
            .checked_sub(1)
            .ok_or(FloorError::ArithmeticOverflow)?;
    }
    Ok(usdc_spent)
}

/// Force-settles the current, partially-filled round.
pub fn close_round<'info>(ctx: Context<'_, '_, 'info, 'info, CloseRound<'info>>) -> Result<()> {
    let round_index;
    let floor_price;
    let waln_decimals;
    let lock_period;
    let waln_in_round;
    let usdc_paid_for_round;
    {
        let state = ctx.accounts.contract_state.load()?;
        require!(state.round_started == 1, FloorError::InvalidParameter);
        require!(state.current_round_waln > 0, FloorError::InvalidParameter);

        round_index = state.round_count;
        floor_price = state.current_round_floor_price;
        waln_decimals = state.waln_decimals;
        lock_period = state.current_round_lock_period;
        waln_in_round = state.current_round_waln;
        usdc_paid_for_round = state.current_round_usdc_spent;
    }

    require!(
        ctx.remaining_accounts.len() >= 2,
        FloorError::InvalidRemainingAccounts
    );
    let round_record_info = &ctx.remaining_accounts[0];
    let round_locked_waln_info = &ctx.remaining_accounts[1];

    let (round_record_pda, round_record_bump) = Pubkey::find_program_address(
        &[ROUND_RECORD_SEED, &round_index.to_le_bytes()],
        &crate::ID,
    );
    require!(
        round_record_pda == round_record_info.key(),
        FloorError::InvalidRemainingAccounts
    );

    let (round_locked_waln_pda, round_locked_waln_bump) = Pubkey::find_program_address(
        &[ROUND_LOCKED_WALN_SEED, &round_index.to_le_bytes()],
        &crate::ID,
    );
    require!(
        round_locked_waln_pda == round_locked_waln_info.key(),
        FloorError::InvalidRemainingAccounts
    );

    let waln_scale = 10_u128.pow(waln_decimals as u32);
    let clock = Clock::get()?;
    let unlock_timestamp = clock
        .unix_timestamp
        .checked_add(lock_period)
        .ok_or(FloorError::ArithmeticOverflow)?;

    let mut total_full_waln: u128 = 0;
    let mut total_locked_usdc: u128 = 0;
    let mut eligible_count: usize = 0;
    {
        let pool = ctx.accounts.investor_pool.load()?;
        let count = pool.count as usize;
        for record in pool.investors[..count].iter() {
            if record.usdc_locked_current_round == 0 {
                continue;
            }
            let full_waln = (record.usdc_locked_current_round as u128)
                .checked_mul(waln_scale)
                .ok_or(FloorError::ArithmeticOverflow)?
                .checked_div(floor_price as u128)
                .ok_or(FloorError::ArithmeticOverflow)?;
            total_full_waln = total_full_waln
                .checked_add(full_waln)
                .ok_or(FloorError::ArithmeticOverflow)?;
            total_locked_usdc = total_locked_usdc
                .checked_add(record.usdc_locked_current_round as u128)
                .ok_or(FloorError::ArithmeticOverflow)?;
            eligible_count = eligible_count
                .checked_add(1)
                .ok_or(FloorError::ArithmeticOverflow)?;
        }
    }
    require!(total_full_waln > 0, FloorError::NoEligibleInvestors);
    require!(usdc_paid_for_round > 0, FloorError::InvalidParameter);
    require!(
        (usdc_paid_for_round as u128) <= total_locked_usdc,
        FloorError::ArithmeticOverflow
    );

    let mut total_base_usdc_spent: u64 = 0;
    {
        let pool = ctx.accounts.investor_pool.load()?;
        let count = pool.count as usize;
        for record in pool.investors[..count].iter() {
            if record.usdc_locked_current_round == 0 {
                continue;
            }
            let base_usdc_spent = u64::try_from(
                (usdc_paid_for_round as u128)
                    .checked_mul(record.usdc_locked_current_round as u128)
                    .ok_or(FloorError::ArithmeticOverflow)?
                    .checked_div(total_locked_usdc)
                    .ok_or(FloorError::ArithmeticOverflow)?,
            )
            .map_err(|_| FloorError::ArithmeticOverflow)?;
            total_base_usdc_spent = total_base_usdc_spent
                .checked_add(base_usdc_spent)
                .ok_or(FloorError::ArithmeticOverflow)?;
        }
    }

    let waln_available = waln_in_round as u128;
    let mut remaining_waln = waln_available;
    let mut remaining_usdc_remainder = usdc_paid_for_round
        .checked_sub(total_base_usdc_spent)
        .ok_or(FloorError::ArithmeticOverflow)?;
    let mut processed_eligible: usize = 0;

    let mut total_usdc_spent: u64 = 0;
    let mut total_waln_purchased: u64 = 0;
    let mut total_aat_volume_at_trigger: u64 = 0;
    let mut participant_data: Vec<(Pubkey, u64)> = Vec::with_capacity(MAX_INVESTORS);

    // Second pass: pro-rata wALN allocation + refund of unused locked USDC.
    {
        let mut pool = ctx.accounts.investor_pool.load_mut()?;
        let count = pool.count as usize;
        for record in pool.investors[..count].iter_mut() {
            if record.usdc_locked_current_round == 0 {
                continue;
            }

            processed_eligible = processed_eligible
                .checked_add(1)
                .ok_or(FloorError::ArithmeticOverflow)?;

            let locked = record.usdc_locked_current_round;
            let full_waln = (locked as u128)
                .checked_mul(waln_scale)
                .ok_or(FloorError::ArithmeticOverflow)?
                .checked_div(floor_price as u128)
                .ok_or(FloorError::ArithmeticOverflow)?;

            let is_last_eligible = processed_eligible == eligible_count;
            let waln_alloc = if is_last_eligible {
                u64::try_from(remaining_waln).map_err(|_| FloorError::ArithmeticOverflow)?
            } else {
                u64::try_from(
                    waln_available
                        .checked_mul(full_waln)
                        .ok_or(FloorError::ArithmeticOverflow)?
                        .checked_div(total_full_waln)
                        .ok_or(FloorError::ArithmeticOverflow)?,
                )
                .map_err(|_| FloorError::ArithmeticOverflow)?
            };
            remaining_waln = remaining_waln
                .checked_sub(waln_alloc as u128)
                .ok_or(FloorError::ArithmeticOverflow)?;

            let usdc_spent = close_round_usdc_spent(
                usdc_paid_for_round,
                locked,
                total_locked_usdc,
                &mut remaining_usdc_remainder,
            )?;

            let refund = locked
                .checked_sub(usdc_spent)
                .ok_or(FloorError::ArithmeticOverflow)?;

            record.usdc_locked_current_round = 0;
            record.usdc_deposited = record
                .usdc_deposited
                .checked_add(refund)
                .ok_or(FloorError::ArithmeticOverflow)?;
            record.usdc_committed = record
                .usdc_committed
                .checked_add(usdc_spent)
                .ok_or(FloorError::ArithmeticOverflow)?;
            record.waln_purchased_total = record
                .waln_purchased_total
                .checked_add(waln_alloc)
                .ok_or(FloorError::ArithmeticOverflow)?;

            total_usdc_spent = total_usdc_spent
                .checked_add(usdc_spent)
                .ok_or(FloorError::ArithmeticOverflow)?;
            total_waln_purchased = total_waln_purchased
                .checked_add(waln_alloc)
                .ok_or(FloorError::ArithmeticOverflow)?;
            total_aat_volume_at_trigger = total_aat_volume_at_trigger
                .checked_add(record.aat_volume)
                .ok_or(FloorError::ArithmeticOverflow)?;

            if waln_alloc > 0 {
                participant_data.push((record.investor, waln_alloc));
            }
        }
    }

    require!(remaining_usdc_remainder == 0, FloorError::ArithmeticOverflow);
    require!(total_usdc_spent == usdc_paid_for_round, FloorError::ArithmeticOverflow);

    participant_data.sort_unstable_by(|a, b| a.0.to_bytes().cmp(&b.0.to_bytes()));
    let participant_count = participant_data.len() as u32;

    let treasury_info = ctx.accounts.treasury.to_account_info();
    let system_program_info = ctx.accounts.system_program.to_account_info();
    let treasury_bump = ctx.bumps.treasury;

    // --- Create / fund the RoundLockedWaln account and write allocations. ---
    if !participant_data.is_empty() {
        let round_locked_waln_space = RoundLockedWaln::space(participant_data.len());
        let round_locked_waln_rent = Rent::get()?.minimum_balance(round_locked_waln_space);

        let existing_locked_waln_lamports = round_locked_waln_info.lamports();
        if existing_locked_waln_lamports == 0 {
            invoke_signed(
                &system_instruction::create_account(
                    treasury_info.key,
                    round_locked_waln_info.key,
                    round_locked_waln_rent,
                    round_locked_waln_space as u64,
                    &crate::ID,
                ),
                &[treasury_info.clone(), round_locked_waln_info.clone(), system_program_info.clone()],
                &[
                    &[TREASURY_SEED, &[treasury_bump]],
                    &[ROUND_LOCKED_WALN_SEED, &round_index.to_le_bytes(), &[round_locked_waln_bump]],
                ],
            )?;
        } else {
            if existing_locked_waln_lamports < round_locked_waln_rent {
                invoke_signed(
                    &system_instruction::transfer(
                        treasury_info.key,
                        round_locked_waln_info.key,
                        round_locked_waln_rent - existing_locked_waln_lamports,
                    ),
                    &[treasury_info.clone(), round_locked_waln_info.clone(), system_program_info.clone()],
                    &[&[TREASURY_SEED, &[treasury_bump]]],
                )?;
            }
            invoke_signed(
                &system_instruction::allocate(round_locked_waln_info.key, round_locked_waln_space as u64),
                &[round_locked_waln_info.clone(), system_program_info.clone()],
                &[&[ROUND_LOCKED_WALN_SEED, &round_index.to_le_bytes(), &[round_locked_waln_bump]]],
            )?;
            invoke_signed(
                &system_instruction::assign(round_locked_waln_info.key, &crate::ID),
                &[round_locked_waln_info.clone(), system_program_info.clone()],
                &[&[ROUND_LOCKED_WALN_SEED, &round_index.to_le_bytes(), &[round_locked_waln_bump]]],
            )?;
        }

        let entries: Vec<InvestorAlloc> = participant_data
            .iter()
            .map(|(investor, waln_amount)| InvestorAlloc {
                investor: *investor,
                waln_amount: *waln_amount,
            })
            .collect();
        let rlw = RoundLockedWaln {
            round_index,
            bump: round_locked_waln_bump,
            unlock: unlock_timestamp,
            remaining_to_claim: participant_count,
            investors: entries,
        };
        let mut data = round_locked_waln_info.try_borrow_mut_data()?;
        let mut writer: &mut [u8] = &mut data;
        rlw.try_serialize(&mut writer)?;
    }

    // --- Create / fund the RoundRecord account and write the round summary. ---
    let round_record_space = 8 + RoundRecord::INIT_SPACE;
    let round_record_rent = Rent::get()?.minimum_balance(round_record_space);

    let existing_round_record_lamports = round_record_info.lamports();
    if existing_round_record_lamports == 0 {
        invoke_signed(
            &system_instruction::create_account(
                treasury_info.key,
                round_record_info.key,
                round_record_rent,
                round_record_space as u64,
                &crate::ID,
            ),
            &[treasury_info.clone(), round_record_info.clone(), system_program_info.clone()],
            &[
                &[TREASURY_SEED, &[treasury_bump]],
                &[ROUND_RECORD_SEED, &round_index.to_le_bytes(), &[round_record_bump]],
            ],
        )?;
    } else {
        if existing_round_record_lamports < round_record_rent {
            invoke_signed(
                &system_instruction::transfer(
                    treasury_info.key,
                    round_record_info.key,
                    round_record_rent - existing_round_record_lamports,
                ),
                &[treasury_info.clone(), round_record_info.clone(), system_program_info.clone()],
                &[&[TREASURY_SEED, &[treasury_bump]]],
            )?;
        }
        invoke_signed(
            &system_instruction::allocate(round_record_info.key, round_record_space as u64),
            &[round_record_info.clone(), system_program_info.clone()],
            &[&[ROUND_RECORD_SEED, &round_index.to_le_bytes(), &[round_record_bump]]],
        )?;
        invoke_signed(
            &system_instruction::assign(round_record_info.key, &crate::ID),
            &[round_record_info.clone(), system_program_info.clone()],
            &[&[ROUND_RECORD_SEED, &round_index.to_le_bytes(), &[round_record_bump]]],
        )?;
    }

    {
        let mut rr_data = round_record_info.try_borrow_mut_data()?;
        rr_data[..8].copy_from_slice(&RoundRecord::DISCRIMINATOR);
        let record = RoundRecord {
            round_index,
            triggered_at: clock.unix_timestamp,
            waln_purchased: total_waln_purchased,
            usdc_spent: total_usdc_spent,
            total_aat_volume_at_trigger,
            participant_count,
            bump: round_record_bump,
        };
        use anchor_lang::AnchorSerialize;
        record.serialize(&mut &mut rr_data[8..])?;
    }

    // --- Finalize state: no auto-start; next round starts on next sell. ---
    let new_dust = waln_in_round
        .checked_sub(total_waln_purchased)
        .ok_or(FloorError::ArithmeticOverflow)?;

    let mut state = ctx.accounts.contract_state.load_mut()?;
    state.round_count = state
        .round_count
        .checked_add(1)
        .ok_or(FloorError::ArithmeticOverflow)?;
    state.current_round_waln = 0;
    state.current_round_usdc_spent = 0;
    state.total_usdc_in_lobby = state
        .total_usdc_in_lobby
        .checked_sub(total_usdc_spent)
        .ok_or(FloorError::ArithmeticOverflow)?;
    state.total_usdc_locked_for_round = 0;
    state.round_started = 0;
    // Forced close preserves existing carryover; only full-round settlement consumes prior dust.
    state.waln_dust_carryover = state
        .waln_dust_carryover
        .checked_add(new_dust)
        .ok_or(FloorError::ArithmeticOverflow)?;

    Ok(())
}

#[derive(Accounts)]
#[instruction(round_index: u64)]
pub struct CloseRoundRecord<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(
        seeds = [CONTRACT_STATE_SEED],
        bump,
        constraint = contract_state.load()?.admin == admin.key() @ FloorError::Unauthorized,
    )]
    pub contract_state: AccountLoader<'info, ProgramState>,

    #[account(
        mut,
        close = treasury,
        seeds = [ROUND_RECORD_SEED, &round_index.to_le_bytes()],
        bump = round_record.bump,
    )]
    pub round_record: Account<'info, RoundRecord>,

    /// CHECK: Treasury PDA — receives the rent refund when the round record closes
    #[account(
        mut,
        seeds = [TREASURY_SEED],
        bump,
    )]
    pub treasury: UncheckedAccount<'info>,
}

pub fn close_round_record(_ctx: Context<CloseRoundRecord>, _round_index: u64) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_round_usdc_remainder_is_capped_by_locked_amount() {
        let locks = [1u64; 100];
        let usdc_paid_for_round = 99u64;
        let total_locked_usdc = locks.iter().map(|v| *v as u128).sum::<u128>();
        let total_base_usdc_spent = locks
            .iter()
            .map(|locked| {
                ((usdc_paid_for_round as u128) * (*locked as u128) / total_locked_usdc) as u64
            })
            .sum::<u64>();
        let mut remainder = usdc_paid_for_round - total_base_usdc_spent;
        let mut total_spent = 0u64;

        for locked in locks {
            let spent = close_round_usdc_spent(
                usdc_paid_for_round,
                locked,
                total_locked_usdc,
                &mut remainder,
            )
            .unwrap();
            assert!(spent <= locked);
            total_spent += spent;
        }

        assert_eq!(remainder, 0);
        assert_eq!(total_spent, usdc_paid_for_round);
    }
}
