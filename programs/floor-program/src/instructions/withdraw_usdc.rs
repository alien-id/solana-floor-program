use anchor_lang::prelude::*;
use anchor_spl::token_interface::{
    transfer_checked, Mint, TokenAccount, TokenInterface, TransferChecked,
};

use crate::errors::FloorError;
use crate::seeds::{CONTRACT_STATE_SEED, LOBBY_ENTRY_SEED, USDC_VAULT_SEED};
use crate::state::{ProgramState, LobbyEntry};

#[derive(Accounts)]
pub struct WithdrawUsdc<'info> {
    pub investor: Signer<'info>,

    #[account(
        mut,
        seeds = [CONTRACT_STATE_SEED],
        bump = contract_state.bump,
    )]
    pub contract_state: Account<'info, ProgramState>,

    #[account(
        mut,
        seeds = [LOBBY_ENTRY_SEED, investor.key().as_ref()],
        bump = lobby_entry.bump,
        constraint = lobby_entry.investor == investor.key() @ FloorError::InvalidInvestor,
    )]
    pub lobby_entry: Account<'info, LobbyEntry>,

    #[account(constraint = usdc_mint.key() == contract_state.usdc_mint @ FloorError::InvalidMint)]
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
        bump = contract_state.usdc_vault_bump,
        token::mint = usdc_mint,
        token::authority = contract_state,
        token::token_program = usdc_token_program,
    )]
    pub usdc_vault: InterfaceAccount<'info, TokenAccount>,

    pub usdc_token_program: Interface<'info, TokenInterface>,
}

pub fn handler(ctx: Context<WithdrawUsdc>, amount: u64) -> Result<()> {
    require!(!ctx.accounts.contract_state.paused, FloorError::ContractPaused);

    let available = ctx.accounts.lobby_entry.usdc_deposited;
    let withdraw_amount = if amount == u64::MAX { available } else { amount };

    require!(withdraw_amount > 0, FloorError::ZeroAmount);
    require!(withdraw_amount <= available, FloorError::InsufficientFunds);

    let bump = ctx.accounts.contract_state.bump;
    let seeds: &[&[u8]] = &[CONTRACT_STATE_SEED, &[bump]];
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
        ctx.accounts.usdc_mint.decimals,
    )?;

    ctx.accounts.lobby_entry.usdc_deposited = ctx
        .accounts
        .lobby_entry
        .usdc_deposited
        .checked_sub(withdraw_amount)
        .ok_or(FloorError::ArithmeticOverflow)?;

    ctx.accounts.contract_state.total_usdc_in_lobby = ctx
        .accounts
        .contract_state
        .total_usdc_in_lobby
        .checked_sub(withdraw_amount)
        .ok_or(FloorError::ArithmeticOverflow)?;

    Ok(())
}
