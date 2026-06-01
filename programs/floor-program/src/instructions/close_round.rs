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

/// Force-settles the current, partially-filled round.
pub fn close_round<'info>(ctx: Context<'_, '_, 'info, 'info, CloseRound<'info>>) -> Result<()> {
    let round_index;
    let floor_price;
    let waln_decimals;
    let lock_period;
    let waln_in_round;
    {
        let state = ctx.accounts.contract_state.load()?;
        require!(state.round_started == 1, FloorError::InvalidParameter);
        require!(state.current_round_waln > 0, FloorError::InvalidParameter);

        round_index = state.round_count;
        floor_price = state.current_round_floor_price;
        waln_decimals = state.waln_decimals;
        lock_period = state.lock_period_seconds;
        waln_in_round = state.current_round_waln;
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

    // First pass: total wALN entitlement (T) had the round filled completely.
    let mut total_full_waln: u128 = 0;
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
        }
    }
    require!(total_full_waln > 0, FloorError::NoEligibleInvestors);

    // wALN available for distribution = what sellers actually delivered,
    // capped at the total entitlement.
    let waln_available = (waln_in_round as u128).min(total_full_waln);

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

            let locked = record.usdc_locked_current_round;
            let full_waln = (locked as u128)
                .checked_mul(waln_scale)
                .ok_or(FloorError::ArithmeticOverflow)?
                .checked_div(floor_price as u128)
                .ok_or(FloorError::ArithmeticOverflow)?;

            let waln_alloc = u64::try_from(
                waln_available
                    .checked_mul(full_waln)
                    .ok_or(FloorError::ArithmeticOverflow)?
                    .checked_div(total_full_waln)
                    .ok_or(FloorError::ArithmeticOverflow)?,
            )
            .map_err(|_| FloorError::ArithmeticOverflow)?;

            let usdc_spent = u64::try_from(
                (waln_alloc as u128)
                    .checked_mul(floor_price as u128)
                    .ok_or(FloorError::ArithmeticOverflow)?
                    .checked_div(waln_scale)
                    .ok_or(FloorError::ArithmeticOverflow)?,
            )
            .map_err(|_| FloorError::ArithmeticOverflow)?;

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

    participant_data.sort_unstable_by(|a, b| a.0.to_bytes().cmp(&b.0.to_bytes()));
    let participant_count = participant_data.len() as u32;

    let treasury_info = ctx.accounts.treasury.to_account_info();
    let system_program_info = ctx.accounts.system_program.to_account_info();
    let treasury_bump = ctx.bumps.treasury;

    // --- Create / fund the RoundLockedWaln account and write allocations. ---
    let round_locked_waln_space = 8 + std::mem::size_of::<RoundLockedWaln>();
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

    {
        let mut data = round_locked_waln_info.try_borrow_mut_data()?;
        data[..8].copy_from_slice(&RoundLockedWaln::DISCRIMINATOR);
        let rw: &mut RoundLockedWaln = bytemuck::from_bytes_mut(&mut data[8..]);
        rw.round_index = round_index;
        rw.count = participant_count;
        rw.bump = round_locked_waln_bump;
        rw._pad = [0; 3];
        for (i, (investor, waln_alloc)) in participant_data.iter().enumerate() {
            rw.investors[i] = InvestorAlloc {
                investor: *investor,
                waln_amount: *waln_alloc,
                unlock: unlock_timestamp,
                claimed: 0,
                _pad: [0; 7],
            };
        }
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
    state.total_usdc_in_lobby = state
        .total_usdc_in_lobby
        .checked_sub(total_usdc_spent)
        .ok_or(FloorError::ArithmeticOverflow)?;
    state.total_usdc_locked_for_round = 0;
    state.round_started = 0;
    state.waln_dust_carryover = state
        .waln_dust_carryover
        .checked_add(new_dust)
        .ok_or(FloorError::ArithmeticOverflow)?;

    Ok(())
}
