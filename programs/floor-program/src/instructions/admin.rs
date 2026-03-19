use anchor_lang::prelude::*;

use crate::errors::FloorError;
use crate::seeds::CONTRACT_STATE_SEED;
use crate::state::ProgramState;

#[derive(Accounts)]
pub struct AdminOnly<'info> {
    pub admin: Signer<'info>,

    #[account(
        mut,
        seeds = [CONTRACT_STATE_SEED],
        bump,
    )]
    pub contract_state: AccountLoader<'info, ProgramState>,
}

pub fn set_floor_price(ctx: Context<AdminOnly>, new_price_usdc: u64) -> Result<()> {
    require!(new_price_usdc > 0, FloorError::InvalidParameter);
    let mut state = ctx.accounts.contract_state.load_mut()?;
    require!(state.admin == ctx.accounts.admin.key(), FloorError::Unauthorized);
    state.floor_price_usdc = new_price_usdc;
    Ok(())
}

pub fn set_round_size(ctx: Context<AdminOnly>, new_round_size_waln: u64) -> Result<()> {
    require!(new_round_size_waln > 0, FloorError::InvalidParameter);
    let mut state = ctx.accounts.contract_state.load_mut()?;
    require!(state.admin == ctx.accounts.admin.key(), FloorError::Unauthorized);
    state.round_size_waln = new_round_size_waln;
    Ok(())
}

pub fn set_lock_period(ctx: Context<AdminOnly>, new_lock_period: i64) -> Result<()> {
    let mut state = ctx.accounts.contract_state.load_mut()?;
    require!(state.admin == ctx.accounts.admin.key(), FloorError::Unauthorized);
    state.lock_period_seconds = new_lock_period;
    Ok(())
}

pub fn set_paused(ctx: Context<AdminOnly>, paused: bool) -> Result<()> {
    let mut state = ctx.accounts.contract_state.load_mut()?;
    require!(state.admin == ctx.accounts.admin.key(), FloorError::Unauthorized);
    state.paused = if paused { 1 } else { 0 };
    Ok(())
}
