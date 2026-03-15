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
        bump = contract_state.bump,
        constraint = contract_state.admin == admin.key() @ FloorError::Unauthorized,
    )]
    pub contract_state: Account<'info, ProgramState>,
}

pub fn set_floor_price(ctx: Context<AdminOnly>, new_price_usdc: u64) -> Result<()> {
    ctx.accounts.contract_state.floor_price_usdc = new_price_usdc;
    Ok(())
}

pub fn set_round_size(ctx: Context<AdminOnly>, new_round_size_waln: u64) -> Result<()> {
    ctx.accounts.contract_state.round_size_waln = new_round_size_waln;
    Ok(())
}

pub fn set_lock_period(ctx: Context<AdminOnly>, new_lock_period: i64) -> Result<()> {
    ctx.accounts.contract_state.lock_period_seconds = new_lock_period;
    Ok(())
}

pub fn set_paused(ctx: Context<AdminOnly>, paused: bool) -> Result<()> {
    ctx.accounts.contract_state.paused = paused;
    Ok(())
}
