use anchor_lang::prelude::*;
use anchor_spl::token_interface::{
    transfer_checked, Mint, TokenAccount, TokenInterface, TransferChecked,
};

use crate::errors::FloorError;
use crate::seeds::{AAT_VAULT_SEED, CONTRACT_STATE_SEED, LOBBY_ENTRY_SEED};
use crate::state::{ProgramState, LobbyEntry};

#[derive(Accounts)]
pub struct WithdrawAat<'info> {
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

    pub aat_mint: InterfaceAccount<'info, Mint>,

    #[account(
        mut,
        token::mint = aat_mint,
        token::authority = investor,
        token::token_program = aat_token_program,
    )]
    pub investor_aat_account: InterfaceAccount<'info, TokenAccount>,

    #[account(
        mut,
        seeds = [AAT_VAULT_SEED],
        bump = contract_state.aat_vault_bump,
        token::mint = aat_mint,
        token::authority = contract_state,
        token::token_program = aat_token_program,
    )]
    pub aat_vault: InterfaceAccount<'info, TokenAccount>,

    pub aat_token_program: Interface<'info, TokenInterface>,
}

pub fn handler(ctx: Context<WithdrawAat>, amount: u64) -> Result<()> {
    let available = ctx.accounts.lobby_entry.aat_staked;
    let withdraw_amount = if amount == u64::MAX { available } else { amount };

    require!(withdraw_amount > 0, FloorError::ZeroAmount);
    require!(withdraw_amount <= available, FloorError::InsufficientFunds);

    let bump = ctx.accounts.contract_state.bump;
    let seeds: &[&[u8]] = &[CONTRACT_STATE_SEED, &[bump]];
    let signer = &[seeds];

    transfer_checked(
        CpiContext::new_with_signer(
            ctx.accounts.aat_token_program.to_account_info(),
            TransferChecked {
                from: ctx.accounts.aat_vault.to_account_info(),
                mint: ctx.accounts.aat_mint.to_account_info(),
                to: ctx.accounts.investor_aat_account.to_account_info(),
                authority: ctx.accounts.contract_state.to_account_info(),
            },
            signer,
        ),
        withdraw_amount,
        ctx.accounts.aat_mint.decimals,
    )?;

    ctx.accounts.lobby_entry.aat_staked = ctx
        .accounts
        .lobby_entry
        .aat_staked
        .checked_sub(withdraw_amount)
        .ok_or(FloorError::ArithmeticOverflow)?;

    ctx.accounts.contract_state.total_aat_staked = ctx
        .accounts
        .contract_state
        .total_aat_staked
        .checked_sub(withdraw_amount)
        .ok_or(FloorError::ArithmeticOverflow)?;

    Ok(())
}
