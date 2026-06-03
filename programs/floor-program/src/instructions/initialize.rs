use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

use crate::errors::FloorError;
use crate::instructions::start_round::require_viable_round_params;
use crate::seeds::{CONTRACT_STATE_SEED, INVESTOR_POOL_SEED, USDC_VAULT_SEED, WALN_VAULT_SEED};
use crate::state::{InvestorPool, ProgramState};

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(
        init,
        payer = admin,
        space = 8 + std::mem::size_of::<ProgramState>(),
        seeds = [CONTRACT_STATE_SEED],
        bump,
    )]
    pub contract_state: AccountLoader<'info, ProgramState>,

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

    #[account(
        init,
        payer = admin,
        space = 8 + std::mem::size_of::<InvestorPool>(),
        seeds = [INVESTOR_POOL_SEED],
        bump,
    )]
    pub investor_pool: AccountLoader<'info, InvestorPool>,

    pub system_program: Program<'info, System>,
    pub usdc_token_program: Interface<'info, TokenInterface>,
    #[account(address = anchor_spl::token_2022::ID)]
    pub waln_token_program: Interface<'info, TokenInterface>,

    #[account(constraint = program.programdata_address()? == Some(program_data.key()) @ FloorError::Unauthorized)]
    pub program: Program<'info, crate::program::FloorProgram>,

    #[account(constraint = program_data.upgrade_authority_address == Some(admin.key()) @ FloorError::Unauthorized)]
    pub program_data: Account<'info, ProgramData>,
}

pub fn handler(
    ctx: Context<Initialize>,
    floor_price_usdc: u64,
    round_size_waln: u64,
    lock_period_seconds: i64,
) -> Result<()> {
    use crate::errors::FloorError;
    require!(floor_price_usdc > 0, FloorError::InvalidParameter);
    require!(round_size_waln > 0, FloorError::InvalidParameter);
    require_viable_round_params(round_size_waln, floor_price_usdc, ctx.accounts.waln_mint.decimals)?;
    require!(lock_period_seconds >= 0, FloorError::InvalidParameter);

    let mut state = ctx.accounts.contract_state.load_init()?;
    state.admin = ctx.accounts.admin.key();
    state.usdc_mint = ctx.accounts.usdc_mint.key();
    state.waln_mint = ctx.accounts.waln_mint.key();
    state.usdc_vault = ctx.accounts.usdc_vault.key();
    state.waln_vault = ctx.accounts.waln_vault.key();
    state.floor_price_usdc = floor_price_usdc;
    state.current_round_floor_price = floor_price_usdc;
    state.current_round_size_waln = round_size_waln;
    state.round_size_waln = round_size_waln;
    state.lock_period_seconds = lock_period_seconds;
    state.current_round_lock_period = lock_period_seconds;
    state.current_round_waln = 0;
    state.total_usdc_in_lobby = 0;
    state.round_count = 0;
    state.sell_paused = 0;
    state.frozen = 0;
    state.round_started = 0;
    state.bump = ctx.bumps.contract_state;
    state.usdc_vault_bump = ctx.bumps.usdc_vault;
    state.waln_vault_bump = ctx.bumps.waln_vault;
    state.waln_decimals = ctx.accounts.waln_mint.decimals;
    state.usdc_decimals = ctx.accounts.usdc_mint.decimals;
    state.waln_dust_carryover = 0;
    state.total_usdc_locked_for_round = 0;

    let mut pool = ctx.accounts.investor_pool.load_init()?;
    pool.bump = ctx.bumps.investor_pool;
    pool.count = 0;

    Ok(())
}
