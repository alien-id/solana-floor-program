use crate::errors::FloorError;
use crate::instructions::start_round::require_viable_round_params;
use crate::seeds::{CONTRACT_STATE_SEED, INVESTOR_POOL_SEED, TREASURY_SEED, USDC_VAULT_SEED};
use crate::state::{InvestorPool, InvestorRecord, ProgramState};
use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    program::{invoke, invoke_signed},
    system_instruction,
};
use anchor_spl::token_interface::{
    transfer_checked, Mint, TokenAccount, TokenInterface, TransferChecked,
};

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
    require_viable_round_params(state.round_size_waln, new_price_usdc, state.waln_decimals)?;
    state.floor_price_usdc = new_price_usdc;
    Ok(())
}

pub fn set_round_size(ctx: Context<AdminOnly>, new_round_size_waln: u64) -> Result<()> {
    require!(new_round_size_waln > 0, FloorError::InvalidParameter);
    let mut state = ctx.accounts.contract_state.load_mut()?;
    require_viable_round_params(
        new_round_size_waln,
        state.floor_price_usdc,
        state.waln_decimals,
    )?;
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
pub struct WithdrawTreasury<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(
        seeds = [CONTRACT_STATE_SEED],
        bump,
        constraint = contract_state.load()?.admin == admin.key() @ FloorError::Unauthorized,
    )]
    pub contract_state: AccountLoader<'info, ProgramState>,

    /// CHECK: Treasury PDA — system-owned, holds SOL for round account rent
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
        state.current_round_usdc_spent = 0;
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

#[derive(Accounts)]
pub struct RemoveInvestorFromPool<'info> {
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

    /// CHECK: investor wallet whose pool slot is being reclaimed; used only as a key
    /// and to authorize the refund destination token account.
    pub investor: UncheckedAccount<'info>,

    pub usdc_mint: InterfaceAccount<'info, Mint>,

    #[account(
        mut,
        token::mint = usdc_mint,
        token::authority = investor,
        token::token_program = usdc_token_program,
    )]
    pub investor_usdc_account: InterfaceAccount<'info, TokenAccount>,

    #[account(
        mut,
        seeds = [USDC_VAULT_SEED],
        bump,
        token::mint = usdc_mint,
        token::authority = contract_state,
        token::token_program = usdc_token_program,
    )]
    pub usdc_vault: InterfaceAccount<'info, TokenAccount>,

    pub usdc_token_program: Interface<'info, TokenInterface>,
}

pub fn remove_investor_from_pool(ctx: Context<RemoveInvestorFromPool>) -> Result<()> {
    let investor = ctx.accounts.investor.key();

    let state_bump;
    let usdc_decimals;
    {
        let state = ctx.accounts.contract_state.load()?;
        require!(
            ctx.accounts.usdc_mint.key() == state.usdc_mint,
            FloorError::InvalidMint
        );
        state_bump = state.bump;
        usdc_decimals = ctx.accounts.usdc_mint.decimals;
    }

    let refund;
    {
        let pool = ctx.accounts.investor_pool.load()?;
        let count = pool.count as usize;
        let idx = pool.investors[..count]
            .iter()
            .position(|r| r.investor == investor)
            .ok_or(FloorError::InvalidInvestor)?;
        let record = &pool.investors[idx];
        require!(
            record.aat_volume == 0 && record.usdc_locked_current_round == 0,
            FloorError::InvestorNotInactive
        );
        refund = record.usdc_deposited;
    }

    // The withdraw-lock is intentionally bypassed: this is a privileged admin teardown
    // that returns the funds to their rightful owner.
    if refund > 0 {
        let seeds: &[&[u8]] = &[CONTRACT_STATE_SEED, &[state_bump]];
        let signer = &[seeds];
        transfer_checked(
            CpiContext::new_with_signer(
                ctx.accounts.usdc_token_program.to_account_info(),
                TransferChecked {
                    from: ctx.accounts.usdc_vault.to_account_info(),
                    mint: ctx.accounts.usdc_mint.to_account_info(),
                    to: ctx.accounts.investor_usdc_account.to_account_info(),
                    authority: ctx.accounts.contract_state.to_account_info(),
                },
                signer,
            ),
            refund,
            usdc_decimals,
        )?;

        let mut state = ctx.accounts.contract_state.load_mut()?;
        state.total_usdc_in_lobby = state
            .total_usdc_in_lobby
            .checked_sub(refund)
            .ok_or(FloorError::ArithmeticOverflow)?;
    }

    {
        let mut pool = ctx.accounts.investor_pool.load_mut()?;
        let count = pool.count as usize;
        let idx = pool.investors[..count]
            .iter()
            .position(|r| r.investor == investor)
            .ok_or(FloorError::InvalidInvestor)?;
        let last = count - 1;
        let moved = pool.investors[last];
        pool.investors[idx] = moved;
        pool.investors[last] = InvestorRecord {
            investor: Pubkey::default(),
            usdc_deposited: 0,
            usdc_locked_current_round: 0,
            usdc_committed: 0,
            waln_purchased_total: 0,
            aat_volume: 0,
            usdc_unlock_ts: 0,
        };
        pool.count -= 1;
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

pub fn withdraw_treasury(ctx: Context<WithdrawTreasury>, amount: u64) -> Result<()> {
    require!(amount > 0, FloorError::ZeroAmount);
    require!(
        ctx.accounts.treasury.lamports() >= amount,
        FloorError::InsufficientFunds
    );

    let treasury_bump = ctx.bumps.treasury;
    invoke_signed(
        &system_instruction::transfer(
            &ctx.accounts.treasury.key(),
            &ctx.accounts.admin.key(),
            amount,
        ),
        &[
            ctx.accounts.treasury.to_account_info(),
            ctx.accounts.admin.to_account_info(),
            ctx.accounts.system_program.to_account_info(),
        ],
        &[&[TREASURY_SEED, &[treasury_bump]]],
    )?;

    Ok(())
}
