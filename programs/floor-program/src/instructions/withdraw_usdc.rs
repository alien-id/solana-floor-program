use anchor_lang::prelude::*;
use anchor_spl::token_interface::{
    transfer_checked, Mint, TokenAccount, TokenInterface, TransferChecked,
};

use crate::errors::FloorError;
use crate::seeds::{CONTRACT_STATE_SEED, INVESTOR_POOL_SEED, USDC_VAULT_SEED};
use crate::state::{InvestorPool, ProgramState};

#[derive(Accounts)]
pub struct WithdrawUsdc<'info> {
    pub investor: Signer<'info>,

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

pub fn handler(ctx: Context<WithdrawUsdc>, amount: u64) -> Result<()> {
    let state_bump;
    let usdc_decimals;
    let withdraw_amount;
    {
        let state = ctx.accounts.contract_state.load()?;
        require!(ctx.accounts.usdc_mint.key() == state.usdc_mint, FloorError::InvalidMint);
        state_bump = state.bump;
        usdc_decimals = ctx.accounts.usdc_mint.decimals;
    }

    {
        let investor_key = ctx.accounts.investor.key();
        let mut pool = ctx.accounts.investor_pool.load_mut()?;
        let count = pool.count as usize;

        let record = pool.investors[..count]
            .iter_mut()
            .find(|r| r.investor == investor_key)
            .ok_or(FloorError::InvalidInvestor)?;

        let now = Clock::get()?.unix_timestamp;
        require!(now >= record.usdc_unlock_ts, FloorError::UsdcLocked);

        let available = record.usdc_deposited;
        withdraw_amount = if amount == u64::MAX { available } else { amount };

        require!(withdraw_amount > 0, FloorError::ZeroAmount);
        require!(withdraw_amount <= available, FloorError::InsufficientFunds);

        record.usdc_deposited = record
            .usdc_deposited
            .checked_sub(withdraw_amount)
            .ok_or(FloorError::ArithmeticOverflow)?;
    }

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
        withdraw_amount,
        usdc_decimals,
    )?;

    let mut state = ctx.accounts.contract_state.load_mut()?;
    state.total_usdc_in_lobby = state
        .total_usdc_in_lobby
        .checked_sub(withdraw_amount)
        .ok_or(FloorError::ArithmeticOverflow)?;

    Ok(())
}
