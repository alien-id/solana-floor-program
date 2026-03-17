use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

use crate::seeds::{CONTRACT_STATE_SEED, USDC_VAULT_SEED, WALN_VAULT_SEED};
use crate::state::ProgramState;

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(
        init,
        payer = admin,
        space = 8 + ProgramState::INIT_SPACE,
        seeds = [CONTRACT_STATE_SEED],
        bump,
    )]
    pub contract_state: Account<'info, ProgramState>,

    pub usdc_mint: InterfaceAccount<'info, Mint>,
    pub waln_mint: InterfaceAccount<'info, Mint>,

    #[account(
        init,
        payer = admin,
        token::mint = usdc_mint,
        token::authority = contract_state,
        token::token_program = usdc_token_program,
        seeds = [USDC_VAULT_SEED],
        bump,
    )]
    pub usdc_vault: InterfaceAccount<'info, TokenAccount>,

    #[account(
        init,
        payer = admin,
        token::mint = waln_mint,
        token::authority = contract_state,
        token::token_program = waln_token_program,
        seeds = [WALN_VAULT_SEED],
        bump,
    )]
    pub waln_vault: InterfaceAccount<'info, TokenAccount>,

    pub system_program: Program<'info, System>,
    pub usdc_token_program: Interface<'info, TokenInterface>,
    pub waln_token_program: Interface<'info, TokenInterface>,
}

pub fn handler(
    ctx: Context<Initialize>,
    floor_price_usdc: u64,
    round_size_waln: u64,
    lock_period_seconds: i64,
) -> Result<()> {
    let state = &mut ctx.accounts.contract_state;
    state.admin = ctx.accounts.admin.key();
    state.usdc_mint = ctx.accounts.usdc_mint.key();
    state.waln_mint = ctx.accounts.waln_mint.key();
    state.usdc_vault = ctx.accounts.usdc_vault.key();
    state.waln_vault = ctx.accounts.waln_vault.key();
    state.floor_price_usdc = floor_price_usdc;
    state.round_size_waln = round_size_waln;
    state.lock_period_seconds = lock_period_seconds;
    state.current_round_waln = 0;
    state.total_usdc_in_lobby = 0;
    state.round_count = 0;
    state.paused = false;
    state.round_started = false;
    state.bump = ctx.bumps.contract_state;
    state.usdc_vault_bump = ctx.bumps.usdc_vault;
    state.waln_vault_bump = ctx.bumps.waln_vault;
    Ok(())
}
