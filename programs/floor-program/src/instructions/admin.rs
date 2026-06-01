use anchor_lang::prelude::*;
use anchor_lang::solana_program::{program::invoke, system_instruction};

use crate::errors::FloorError;
use crate::seeds::{CONTRACT_STATE_SEED, INVESTOR_POOL_SEED, TREASURY_SEED};
use crate::state::{InvestorPool, ProgramState};

#[derive(Accounts)]
pub struct AdminOnly<'info> {
    pub admin: Signer<'info>,

    #[account(
        mut,
        seeds = [CONTRACT_STATE_SEED],
        bump,
        constraint = contract_state.load()?.admin == admin.key() @ FloorError::Unauthorized,
    )]
    pub contract_state: AccountLoader<'info, ProgramState>,
}

pub fn set_floor_price(ctx: Context<AdminOnly>, new_price_usdc: u64) -> Result<()> {
    require!(new_price_usdc > 0, FloorError::InvalidParameter);
    let mut state = ctx.accounts.contract_state.load_mut()?;
    state.floor_price_usdc = new_price_usdc;
    Ok(())
}

pub fn set_round_size(ctx: Context<AdminOnly>, new_round_size_waln: u64) -> Result<()> {
    require!(new_round_size_waln > 0, FloorError::InvalidParameter);
    let mut state = ctx.accounts.contract_state.load_mut()?;
    state.round_size_waln = new_round_size_waln;
    Ok(())
}

pub fn set_lock_period(ctx: Context<AdminOnly>, new_lock_period: i64) -> Result<()> {
    require!(new_lock_period >= 0, FloorError::InvalidParameter);
    let mut state = ctx.accounts.contract_state.load_mut()?;
    state.lock_period_seconds = new_lock_period;
    Ok(())
}

pub fn set_usdc_withdraw_lock(ctx: Context<AdminOnly>, new_lock_seconds: i64) -> Result<()> {
    require!(new_lock_seconds >= 0, FloorError::InvalidParameter);
    let mut state = ctx.accounts.contract_state.load_mut()?;
    state.usdc_withdraw_lock_seconds = new_lock_seconds;
    Ok(())
}

#[derive(Accounts)]
pub struct SetInvestorUsdcUnlock<'info> {
    pub admin: Signer<'info>,

    #[account(
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
}

pub fn set_investor_usdc_unlock(
    ctx: Context<SetInvestorUsdcUnlock>,
    investor: Pubkey,
    new_unlock_ts: i64,
) -> Result<()> {
    let mut pool = ctx.accounts.investor_pool.load_mut()?;
    let count = pool.count as usize;
    let record = pool.investors[..count]
        .iter_mut()
        .find(|r| r.investor == investor)
        .ok_or(FloorError::InvalidInvestor)?;
    record.usdc_unlock_ts = new_unlock_ts;
    Ok(())
}

pub fn set_sell_paused(ctx: Context<AdminOnly>, paused: bool) -> Result<()> {
    let mut state = ctx.accounts.contract_state.load_mut()?;
    state.sell_paused = if paused { 1 } else { 0 };
    Ok(())
}

pub fn set_frozen(ctx: Context<AdminOnly>, frozen: bool) -> Result<()> {
    let mut state = ctx.accounts.contract_state.load_mut()?;
    state.frozen = if frozen { 1 } else { 0 };
    Ok(())
}

#[derive(Accounts)]
pub struct FundTreasury<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(
        seeds = [CONTRACT_STATE_SEED],
        bump,
        constraint = contract_state.load()?.admin == admin.key() @ FloorError::Unauthorized,
    )]
    pub contract_state: AccountLoader<'info, ProgramState>,

    /// CHECK: Treasury PDA — system-owned, receives SOL for round account rent
    #[account(
        mut,
        seeds = [TREASURY_SEED],
        bump,
    )]
    pub treasury: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CancelRound<'info> {
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
}

pub fn cancel_round(ctx: Context<CancelRound>) -> Result<()> {
    {
        let mut state = ctx.accounts.contract_state.load_mut()?;
        require!(state.round_started == 1, FloorError::InvalidParameter);
        require!(state.current_round_waln == 0, FloorError::InvalidParameter);
        state.round_started = 0;
        state.current_round_waln = 0;
        state.total_usdc_locked_for_round = 0;
    }

    let mut pool = ctx.accounts.investor_pool.load_mut()?;
    let count = pool.count as usize;
    for record in pool.investors[..count].iter_mut() {
        if record.usdc_locked_current_round == 0 {
            continue;
        }
        record.usdc_deposited = record
            .usdc_deposited
            .checked_add(record.usdc_locked_current_round)
            .ok_or(FloorError::ArithmeticOverflow)?;
        record.usdc_locked_current_round = 0;
    }

    Ok(())
}

pub fn fund_treasury(ctx: Context<FundTreasury>, amount: u64) -> Result<()> {
    require!(amount > 0, FloorError::ZeroAmount);

    invoke(
        &system_instruction::transfer(
            &ctx.accounts.admin.key(),
            &ctx.accounts.treasury.key(),
            amount,
        ),
        &[
            ctx.accounts.admin.to_account_info(),
            ctx.accounts.treasury.to_account_info(),
            ctx.accounts.system_program.to_account_info(),
        ],
    )?;

    Ok(())
}
