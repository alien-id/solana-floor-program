use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    log::sol_log_data,
    program::{invoke, invoke_signed},
    system_instruction,
};
use anchor_lang::Discriminator;
use anchor_spl::token_2022::spl_token_2022::instruction::transfer_checked as build_transfer_checked_ix;
use anchor_spl::token_interface::{
    transfer_checked, Mint, TokenAccount, TokenInterface, TransferChecked,
};

use crate::errors::FloorError;
use crate::seeds::{
    CONTRACT_STATE_SEED, INVESTOR_POOL_SEED, ROUND_LOCKED_WALN_SEED, ROUND_RECORD_SEED,
    TREASURY_SEED, USDC_VAULT_SEED, WALN_VAULT_SEED,
};
use crate::state::{
    InvestorAllocated, InvestorPool, ProgramState, RoundClosed, RoundLockedWaln, RoundRecord,
    MIN_SELL_WALN,
};
use crate::utils::{get_hook_program_id, validate_hook_accounts};

#[derive(Accounts)]
#[instruction(round_index: u64)]
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
    pub investor_pool: Account<'info, InvestorPool>,

    #[account(
        mut,
        seeds = [ROUND_LOCKED_WALN_SEED, &round_index.to_le_bytes()],
        bump = round_locked_waln.bump,
    )]
    pub round_locked_waln: Account<'info, RoundLockedWaln>,

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

    /// CHECK: Treasury PDA — system-owned, funds round record creation
    #[account(
        mut,
        seeds = [TREASURY_SEED],
        bump,
    )]
    pub treasury: UncheckedAccount<'info>,

    #[account(address = anchor_spl::token_2022::ID)]
    pub waln_token_program: Interface<'info, TokenInterface>,
    pub usdc_token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

pub fn handler<'info>(
    mut ctx: Context<'_, '_, 'info, 'info, SellWaln<'info>>,
    round_index: u64,
    waln_amount: u64,
    _hook_bumps: [u8; 4],
) -> Result<()> {
    let waln_decimals = ctx.accounts.waln_mint.decimals;
    let usdc_decimals = ctx.accounts.usdc_mint.decimals;

    let state_bump;
    let floor_price_usdc;
    let current_round_size_waln;

    {
        let state = ctx.accounts.contract_state.load()?;
        require!(state.frozen == 0, FloorError::ContractFrozen);
        require!(state.sell_paused == 0, FloorError::SellPaused);
        require!(waln_amount > 0, FloorError::ZeroAmount);
        require!(state.round_started == 1, FloorError::InvalidParameter);
        require!(state.round_count == round_index, FloorError::InvalidParameter);
        require!(
            ctx.accounts.waln_mint.key() == state.waln_mint,
            FloorError::InvalidMint
        );
        require!(
            ctx.accounts.usdc_mint.key() == state.usdc_mint,
            FloorError::InvalidMint
        );

        state_bump = state.bump;
        floor_price_usdc = state.current_round_floor_price;
        current_round_size_waln = state.current_round_size_waln;

        let remaining_in_round = current_round_size_waln
            .checked_sub(state.current_round_waln)
            .ok_or(FloorError::ArithmeticOverflow)?;
        require!(
            waln_amount <= remaining_in_round,
            FloorError::SellAmountExceedsRound
        );

        let is_completing = waln_amount == remaining_in_round;
        require!(
            waln_amount >= MIN_SELL_WALN || is_completing,
            FloorError::SellAmountTooSmall
        );

        let post_sale_remaining = remaining_in_round
            .checked_sub(waln_amount)
            .ok_or(FloorError::ArithmeticOverflow)?;
        if post_sale_remaining > 0 {
            let local_waln_scale = 10_u128.pow(waln_decimals as u32);
            let post_sale_usdc = (post_sale_remaining as u128)
                .checked_mul(floor_price_usdc as u128)
                .ok_or(FloorError::ArithmeticOverflow)?
                .checked_div(local_waln_scale)
                .ok_or(FloorError::ArithmeticOverflow)?;
            require!(post_sale_usdc > 0, FloorError::SellLeavesUnpayableDust);
        }
    }

    let waln_scale = 10_u128.pow(waln_decimals as u32);

    let usdc_out_u128 = (waln_amount as u128)
        .checked_mul(floor_price_usdc as u128)
        .ok_or(FloorError::ArithmeticOverflow)?
        .checked_div(waln_scale)
        .ok_or(FloorError::ArithmeticOverflow)?;
    let usdc_out = u64::try_from(usdc_out_u128).map_err(|_| FloorError::ArithmeticOverflow)?;

    require!(usdc_out > 0, FloorError::ZeroAmount);

    let hook_offset: usize;
    if let Ok(hook_program_id) = get_hook_program_id(&ctx.accounts.waln_mint.to_account_info()) {
        validate_hook_accounts(
            &ctx.remaining_accounts[..8],
            &ctx.accounts.waln_mint.key(),
            &ctx.accounts.seller.key(),
            &hook_program_id,
        )?;
        hook_offset = 8;
    } else {
        hook_offset = 0;
    }

    let hook_accounts = &ctx.remaining_accounts[..hook_offset];

    let mut ix = build_transfer_checked_ix(
        &ctx.accounts.waln_token_program.key(),
        &ctx.accounts.seller_waln_account.key(),
        &ctx.accounts.waln_mint.key(),
        &ctx.accounts.waln_vault.key(),
        &ctx.accounts.seller.key(),
        &[],
        waln_amount,
        waln_decimals,
    )?;

    for acc in hook_accounts.iter() {
        ix.accounts.push(AccountMeta {
            pubkey: acc.key(),
            is_signer: acc.is_signer,
            is_writable: acc.is_writable,
        });
    }

    let mut account_infos = vec![
        ctx.accounts.seller_waln_account.to_account_info(),
        ctx.accounts.waln_mint.to_account_info(),
        ctx.accounts.waln_vault.to_account_info(),
        ctx.accounts.seller.to_account_info(),
    ];
    for acc in hook_accounts.iter() {
        account_infos.push(acc.clone());
    }

    invoke(&ix, &account_infos)?;

    let seeds: &[&[u8]] = &[CONTRACT_STATE_SEED, &[state_bump]];
    let signer = &[seeds];

    transfer_checked(
        CpiContext::new_with_signer(
            ctx.accounts.usdc_token_program.to_account_info(),
            TransferChecked {
                from: ctx.accounts.usdc_vault.to_account_info(),
                mint: ctx.accounts.usdc_mint.to_account_info(),
                to: ctx.accounts.seller_usdc_account.to_account_info(),
                authority: ctx.accounts.contract_state.to_account_info(),
            },
            signer,
        ),
        usdc_out,
        usdc_decimals,
    )?;

    let close_round = {
        let mut state = ctx.accounts.contract_state.load_mut()?;
        state.current_round_usdc_spent = state
            .current_round_usdc_spent
            .checked_add(usdc_out)
            .ok_or(FloorError::ArithmeticOverflow)?;
        state.current_round_waln = state
            .current_round_waln
            .checked_add(waln_amount)
            .ok_or(FloorError::ArithmeticOverflow)?;
        state.current_round_waln >= current_round_size_waln
    };

    if close_round {
        require!(
            ctx.remaining_accounts.len() >= hook_offset + 1,
            FloorError::InvalidRemainingAccounts
        );
        let round_record_info = &ctx.remaining_accounts[hook_offset];
        finalize_round(
            &mut ctx.accounts,
            round_index,
            round_record_info,
            waln_decimals,
        )?;
    }

    Ok(())
}

fn finalize_round<'info>(
    accounts: &mut SellWaln<'info>,
    round_index: u64,
    round_record_info: &AccountInfo<'info>,
    waln_decimals: u8,
) -> Result<()> {
    let clock = Clock::get()?;
    let waln_scale = 10_u128.pow(waln_decimals as u32);

    let (lock_period, dust_pool, waln_in_round, floor_price_usdc_val) = {
        let state = accounts.contract_state.load()?;
        (
            state.lock_period_seconds,
            state.waln_dust_carryover,
            state.current_round_waln,
            state.current_round_floor_price,
        )
    };

    let unlock_timestamp = clock
        .unix_timestamp
        .checked_add(lock_period)
        .ok_or(FloorError::ArithmeticOverflow)?;

    let mut total_usdc_spent: u64 = 0;
    let mut total_waln_purchased: u64 = 0;
    let mut total_aat_volume_at_trigger: u64 = 0;

    {
        let pool = &mut accounts.investor_pool;
        for record in pool.investors.iter_mut() {
            if record.usdc_locked_current_round == 0 {
                continue;
            }

            total_aat_volume_at_trigger = total_aat_volume_at_trigger
                .checked_add(record.aat_volume)
                .ok_or(FloorError::ArithmeticOverflow)?;

            let usdc_locked = record.usdc_locked_current_round;
            let base_waln = u64::try_from(
                (usdc_locked as u128)
                    .checked_mul(waln_scale)
                    .ok_or(FloorError::ArithmeticOverflow)?
                    .checked_div(floor_price_usdc_val as u128)
                    .ok_or(FloorError::ArithmeticOverflow)?,
            )
            .map_err(|_| FloorError::ArithmeticOverflow)?;

            record.usdc_committed = record
                .usdc_committed
                .checked_add(usdc_locked)
                .ok_or(FloorError::ArithmeticOverflow)?;
            record.usdc_locked_current_round = 0;
            record.waln_purchased_total = record
                .waln_purchased_total
                .checked_add(base_waln)
                .ok_or(FloorError::ArithmeticOverflow)?;

            total_usdc_spent = total_usdc_spent
                .checked_add(usdc_locked)
                .ok_or(FloorError::ArithmeticOverflow)?;
            total_waln_purchased = total_waln_purchased
                .checked_add(base_waln)
                .ok_or(FloorError::ArithmeticOverflow)?;
        }
    }

    let rlw = &mut accounts.round_locked_waln;
    let participant_count = rlw.investors.len() as u32;

    if dust_pool > 0 && participant_count > 0 {
        let dust_idx =
            (clock.unix_timestamp as u64).wrapping_rem(participant_count as u64) as usize;
        rlw.investors[dust_idx].waln_amount = rlw.investors[dust_idx]
            .waln_amount
            .checked_add(dust_pool)
            .ok_or(FloorError::ArithmeticOverflow)?;
        total_waln_purchased = total_waln_purchased
            .checked_add(dust_pool)
            .ok_or(FloorError::ArithmeticOverflow)?;

        let winner_key = rlw.investors[dust_idx].investor;
        if let Some(record) = accounts
            .investor_pool
            .investors
            .iter_mut()
            .find(|r| r.investor == winner_key)
        {
            record.waln_purchased_total = record
                .waln_purchased_total
                .checked_add(dust_pool)
                .ok_or(FloorError::ArithmeticOverflow)?;
        }
    }

    rlw.unlock = unlock_timestamp;
    rlw.remaining_to_claim = participant_count;
    rlw.finalized = true;

    const ALLOC_LOG_LEN: usize = 8 + 8 + 32 + 8 + 8 + 8;
    let alloc_disc = InvestorAllocated::DISCRIMINATOR;
    let round_index_le = round_index.to_le_bytes();
    let unlock_le = unlock_timestamp.to_le_bytes();

    for alloc in rlw.investors.iter() {
        let usdc_spent_for = u64::try_from(
            (alloc.waln_amount as u128)
                .checked_mul(floor_price_usdc_val as u128)
                .ok_or(FloorError::ArithmeticOverflow)?
                .checked_div(waln_scale)
                .ok_or(FloorError::ArithmeticOverflow)?,
        )
        .unwrap_or(0);

        let mut buf = [0u8; ALLOC_LOG_LEN];
        buf[0..8].copy_from_slice(&alloc_disc);
        buf[8..16].copy_from_slice(&round_index_le);
        buf[16..48].copy_from_slice(alloc.investor.as_ref());
        buf[48..56].copy_from_slice(&alloc.waln_amount.to_le_bytes());
        buf[56..64].copy_from_slice(&usdc_spent_for.to_le_bytes());
        buf[64..72].copy_from_slice(&unlock_le);

        sol_log_data(&[&buf]);
    }

    emit!(RoundClosed {
        round_index,
        waln_purchased: total_waln_purchased,
        usdc_spent: total_usdc_spent,
        participant_count,
        unlock: unlock_timestamp,
    });

    // Create RoundRecord
    let (round_record_pda, round_record_bump) =
        Pubkey::find_program_address(&[ROUND_RECORD_SEED, &round_index.to_le_bytes()], &crate::ID);
    require!(
        round_record_pda == *round_record_info.key,
        FloorError::InvalidRemainingAccounts
    );

    let treasury_bump = {
        let (pda, b) = Pubkey::find_program_address(&[TREASURY_SEED], &crate::ID);
        require!(pda == *accounts.treasury.key, FloorError::InvalidParameter);
        b
    };

    let space = 8 + RoundRecord::INIT_SPACE;
    let rent = Rent::get()?.minimum_balance(space);

    if round_record_info.data_is_empty() {
        invoke_signed(
            &system_instruction::create_account(
                accounts.treasury.key,
                round_record_info.key,
                rent,
                space as u64,
                &crate::ID,
            ),
            &[
                accounts.treasury.to_account_info(),
                round_record_info.clone(),
                accounts.system_program.to_account_info(),
            ],
            &[
                &[TREASURY_SEED, &[treasury_bump]],
                &[
                    ROUND_RECORD_SEED,
                    &round_index.to_le_bytes(),
                    &[round_record_bump],
                ],
            ],
        )?;
    } else {
        require!(
            round_record_info.owner == &crate::ID,
            FloorError::InvalidRemainingAccounts
        );
    }

    let record = RoundRecord {
        round_index,
        triggered_at: clock.unix_timestamp,
        waln_purchased: total_waln_purchased,
        usdc_spent: total_usdc_spent,
        total_aat_volume_at_trigger,
        participant_count,
        bump: round_record_bump,
    };
    {
        let mut data = round_record_info.try_borrow_mut_data()?;
        let mut writer: &mut [u8] = &mut data;
        record.try_serialize(&mut writer)?;
    }

    let mut state = accounts.contract_state.load_mut()?;
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
    state.round_started = 0;
    state.waln_dust_carryover = u64::try_from(
        (waln_in_round as u128)
            .checked_add(dust_pool as u128)
            .ok_or(FloorError::ArithmeticOverflow)?
            .checked_sub(total_waln_purchased as u128)
            .ok_or(FloorError::ArithmeticOverflow)?,
    )
    .map_err(|_| FloorError::ArithmeticOverflow)?;
    state.current_round_floor_price = state.floor_price_usdc;
    state.current_round_size_waln = state.round_size_waln;
    state.total_usdc_locked_for_round = 0;

    Ok(())
}
