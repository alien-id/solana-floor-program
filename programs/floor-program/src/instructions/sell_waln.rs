use anchor_lang::prelude::*;
use anchor_lang::solana_program::{program::invoke_signed, system_instruction};
use anchor_spl::token_interface::{
    transfer_checked, Mint, TokenAccount, TokenInterface, TransferChecked,
};

use crate::errors::FloorError;
use crate::instructions::start_round::execute_round_start;
use crate::seeds::{
    CONTRACT_STATE_SEED, INVESTOR_POOL_SEED, LOCKED_WALN_SEED, ROUND_RECORD_SEED, USDC_VAULT_SEED,
    WALN_VAULT_SEED,
};
use crate::state::{InvestorPool, LockedWaln, ProgramState, RoundRecord};

#[derive(Accounts)]
pub struct SellWaln<'info> {
    #[account(mut)]
    pub seller: Signer<'info>,

    #[account(
        mut,
        seeds = [CONTRACT_STATE_SEED],
        bump,
    )]
    pub contract_state: AccountLoader<'info, ProgramState>,

    #[account(
        mut,
        seeds = [INVESTOR_POOL_SEED],
        bump,
    )]
    pub investor_pool: AccountLoader<'info, InvestorPool>,

    pub waln_mint: Box<InterfaceAccount<'info, Mint>>,
    pub usdc_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        mut,
        token::mint = waln_mint,
        token::authority = seller,
        token::token_program = waln_token_program,
    )]
    pub seller_waln_account: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        token::mint = usdc_mint,
        token::authority = seller,
        token::token_program = usdc_token_program,
    )]
    pub seller_usdc_account: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        seeds = [WALN_VAULT_SEED],
        bump,
        token::mint = waln_mint,
        token::authority = contract_state,
        token::token_program = waln_token_program,
    )]
    pub waln_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        seeds = [USDC_VAULT_SEED],
        bump,
        token::mint = usdc_mint,
        token::authority = contract_state,
        token::token_program = usdc_token_program,
    )]
    pub usdc_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    pub waln_token_program: Interface<'info, TokenInterface>,
    pub usdc_token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

pub fn handler<'info>(
    ctx: Context<'_, '_, 'info, 'info, SellWaln<'info>>,
    waln_amount: u64,
) -> Result<()> {
    let waln_decimals = ctx.accounts.waln_mint.decimals;
    let usdc_decimals = ctx.accounts.usdc_mint.decimals;

    let round_index;
    let floor_price_usdc;
    let state_bump;
    let current_round_size_waln;
    let round_size_waln_val;
    let floor_price_usdc_val;

    {
        let mut state = ctx.accounts.contract_state.load_mut()?;
        require!(state.paused == 0, FloorError::ContractPaused);
        require!(waln_amount > 0, FloorError::ZeroAmount);
        require!(ctx.accounts.waln_mint.key() == state.waln_mint, FloorError::InvalidMint);
        require!(ctx.accounts.usdc_mint.key() == state.usdc_mint, FloorError::InvalidMint);

        round_index = state.round_count;

        if state.round_started == 0 {
            let mut pool = ctx.accounts.investor_pool.load_mut()?;
            let count = pool.count as usize;
            let snapshot_price = state.floor_price_usdc;
            let snapshot_size = state.round_size_waln;
            let (_aat_vol, usdc_locked) = execute_round_start(
                &mut pool.investors[..count],
                snapshot_size,
                snapshot_price,
                waln_decimals,
            )?;
            state.current_round_floor_price = snapshot_price;
            state.current_round_size_waln = snapshot_size;
            state.total_usdc_locked_for_round = usdc_locked;
            state.round_started = 1;
        }

        let remaining_in_round = state
            .current_round_size_waln
            .checked_sub(state.current_round_waln)
            .ok_or(FloorError::ArithmeticOverflow)?;
        require!(
            waln_amount <= remaining_in_round,
            FloorError::SellAmountExceedsRound
        );

        floor_price_usdc = state.current_round_floor_price;
        state_bump = state.bump;
        current_round_size_waln = state.current_round_size_waln;
        round_size_waln_val = state.round_size_waln;
        floor_price_usdc_val = state.floor_price_usdc;
    }

    let (round_record_pda, round_record_bump) = Pubkey::find_program_address(
        &[ROUND_RECORD_SEED, &round_index.to_le_bytes()],
        &crate::ID,
    );

    let waln_scale = 10_u128.pow(waln_decimals as u32);

    let usdc_out_u128 = (waln_amount as u128)
        .checked_mul(floor_price_usdc as u128)
        .ok_or(FloorError::ArithmeticOverflow)?
        .checked_div(waln_scale)
        .ok_or(FloorError::ArithmeticOverflow)?;
    let usdc_out = u64::try_from(usdc_out_u128).map_err(|_| FloorError::ArithmeticOverflow)?;

    let seller_info = ctx.accounts.seller.to_account_info();
    let system_program_info = ctx.accounts.system_program.to_account_info();
    let contract_state_info = ctx.accounts.contract_state.to_account_info();

    transfer_checked(
        CpiContext::new(
            ctx.accounts.waln_token_program.to_account_info(),
            TransferChecked {
                from: ctx.accounts.seller_waln_account.to_account_info(),
                mint: ctx.accounts.waln_mint.to_account_info(),
                to: ctx.accounts.waln_vault.to_account_info(),
                authority: seller_info.clone(),
            },
        ),
        waln_amount,
        waln_decimals,
    )?;

    let seeds: &[&[u8]] = &[CONTRACT_STATE_SEED, &[state_bump]];
    let signer = &[seeds];

    transfer_checked(
        CpiContext::new_with_signer(
            ctx.accounts.usdc_token_program.to_account_info(),
            TransferChecked {
                from: ctx.accounts.usdc_vault.to_account_info(),
                mint: ctx.accounts.usdc_mint.to_account_info(),
                to: ctx.accounts.seller_usdc_account.to_account_info(),
                authority: contract_state_info.clone(),
            },
            signer,
        ),
        usdc_out,
        usdc_decimals,
    )?;

    let mut state = ctx.accounts.contract_state.load_mut()?;
    state.current_round_waln = state
        .current_round_waln
        .checked_add(waln_amount)
        .ok_or(FloorError::ArithmeticOverflow)?;

    if state.current_round_waln >= current_round_size_waln {
        let remaining = ctx.remaining_accounts;
        require!(
            !remaining.is_empty(),
            FloorError::InvalidRemainingAccounts
        );

        let clock = Clock::get()?;
        let lock_period = state.lock_period_seconds;
        let dust_pool = state.waln_dust_carryover;
        let waln_in_round = state.current_round_waln;

        let unlock_timestamp = clock
            .unix_timestamp
            .checked_add(lock_period)
            .ok_or(FloorError::ArithmeticOverflow)?;

        let round_record_info = &remaining[0];
        let locked_waln_accounts = &remaining[1..];

        require!(
            round_record_pda == round_record_info.key(),
            FloorError::InvalidRemainingAccounts
        );

        let mut total_usdc_spent: u64 = 0;
        let mut total_waln_purchased: u64 = 0;
        let mut participant_count: u32 = 0;
        let mut total_aat_volume_at_trigger: u64 = 0;
        let mut dust_given = false;
        let mut locked_waln_idx: usize = 0;

        let locked_waln_space = 8 + LockedWaln::INIT_SPACE;
        let locked_waln_rent = Rent::get()?.minimum_balance(locked_waln_space);

        let mut pool = ctx.accounts.investor_pool.load_mut()?;
        let pool_count = pool.count as usize;

        for record in pool.investors[..pool_count].iter_mut() {
            if record.usdc_locked_current_round == 0 {
                continue;
            }

            let investor = record.investor;

            let alloc = record.aat_volume;
            total_aat_volume_at_trigger = total_aat_volume_at_trigger
                .checked_add(alloc)
                .ok_or(FloorError::ArithmeticOverflow)?;

            let usdc_locked = record.usdc_locked_current_round;
            let base_waln = u64::try_from(
                (usdc_locked as u128)
                    .checked_mul(waln_scale)
                    .ok_or(FloorError::ArithmeticOverflow)?
                    .checked_div(floor_price_usdc as u128)
                    .ok_or(FloorError::ArithmeticOverflow)?,
            )
            .map_err(|_| FloorError::ArithmeticOverflow)?;

            let bonus = if !dust_given && dust_pool > 0 {
                dust_given = true;
                dust_pool
            } else {
                0
            };

            let waln_alloc = base_waln
                .checked_add(bonus)
                .ok_or(FloorError::ArithmeticOverflow)?;

            record.usdc_committed = record
                .usdc_committed
                .checked_add(usdc_locked)
                .ok_or(FloorError::ArithmeticOverflow)?;
            record.usdc_locked_current_round = 0;
            record.waln_purchased_total = record
                .waln_purchased_total
                .checked_add(waln_alloc)
                .ok_or(FloorError::ArithmeticOverflow)?;

            require!(
                locked_waln_idx < locked_waln_accounts.len(),
                FloorError::InvalidRemainingAccounts
            );
            let locked_waln_info = &locked_waln_accounts[locked_waln_idx];
            locked_waln_idx += 1;

            let (locked_waln_pda, locked_waln_bump) = Pubkey::find_program_address(
                &[
                    LOCKED_WALN_SEED,
                    investor.as_ref(),
                    &round_index.to_le_bytes(),
                ],
                &crate::ID,
            );
            require!(
                locked_waln_pda == locked_waln_info.key(),
                FloorError::InvalidRemainingAccounts
            );

            invoke_signed(
                &system_instruction::create_account(
                    seller_info.key,
                    locked_waln_info.key,
                    locked_waln_rent,
                    locked_waln_space as u64,
                    &crate::ID,
                ),
                &[seller_info.clone(), locked_waln_info.clone(), system_program_info.clone()],
                &[&[
                    LOCKED_WALN_SEED,
                    investor.as_ref(),
                    &round_index.to_le_bytes(),
                    &[locked_waln_bump],
                ]],
            )?;

            {
                let mut data = locked_waln_info.try_borrow_mut_data()?;
                data[..8].copy_from_slice(&LockedWaln::DISCRIMINATOR);
                let locked_record = LockedWaln {
                    investor,
                    round_index,
                    waln_amount: waln_alloc,
                    unlock: unlock_timestamp,
                    claimed: false,
                    bump: locked_waln_bump,
                };
                use anchor_lang::AnchorSerialize;
                locked_record.serialize(&mut &mut data[8..])?;
            }

            total_usdc_spent = total_usdc_spent
                .checked_add(usdc_locked)
                .ok_or(FloorError::ArithmeticOverflow)?;
            total_waln_purchased = total_waln_purchased
                .checked_add(waln_alloc)
                .ok_or(FloorError::ArithmeticOverflow)?;
            participant_count = participant_count
                .checked_add(1)
                .ok_or(FloorError::ArithmeticOverflow)?;
        }

        let round_record_space = 8 + RoundRecord::INIT_SPACE;
        let round_record_rent = Rent::get()?.minimum_balance(round_record_space);

        invoke_signed(
            &system_instruction::create_account(
                seller_info.key,
                round_record_info.key,
                round_record_rent,
                round_record_space as u64,
                &crate::ID,
            ),
            &[seller_info.clone(), round_record_info.clone(), system_program_info.clone()],
            &[&[
                ROUND_RECORD_SEED,
                &round_index.to_le_bytes(),
                &[round_record_bump],
            ]],
        )?;

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

        state.round_count = state
            .round_count
            .checked_add(1)
            .ok_or(FloorError::ArithmeticOverflow)?;
        state.current_round_waln = 0;
        state.total_usdc_in_lobby = state
            .total_usdc_in_lobby
            .checked_sub(total_usdc_spent)
            .ok_or(FloorError::ArithmeticOverflow)?;
        state.round_started = 0;
        state.waln_dust_carryover = u64::try_from(
            (waln_in_round as u128)
                .checked_add(dust_pool as u128)
                .ok_or(FloorError::ArithmeticOverflow)?
                .checked_sub(total_waln_purchased as u128)
                .ok_or(FloorError::ArithmeticOverflow)?,
        )
        .map_err(|_| FloorError::ArithmeticOverflow)?;

        let pool_count = pool.count as usize;
        if let Ok((_aat_vol, usdc_locked)) = execute_round_start(
            &mut pool.investors[..pool_count],
            round_size_waln_val,
            floor_price_usdc_val,
            waln_decimals,
        ) {
            state.current_round_floor_price = floor_price_usdc_val;
            state.current_round_size_waln = round_size_waln_val;
            state.total_usdc_locked_for_round = usdc_locked;
            state.round_started = 1;
        }
    }

    Ok(())
}
