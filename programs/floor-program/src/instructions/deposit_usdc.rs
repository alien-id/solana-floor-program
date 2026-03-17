use anchor_lang::prelude::*;
use anchor_spl::token_interface::{
    transfer_checked, Mint, TokenAccount, TokenInterface, TransferChecked,
};

use crate::errors::FloorError;
use crate::seeds::{AAT_VAULT_SEED, CONTRACT_STATE_SEED, LOBBY_ENTRY_SEED, USDC_VAULT_SEED};
use crate::state::{ProgramState, LobbyEntry};

#[derive(Accounts)]
pub struct DepositUsdc<'info> {
    #[account(mut)]
    pub investor: Signer<'info>,

    #[account(
        mut,
        seeds = [CONTRACT_STATE_SEED],
        bump = contract_state.bump,
    )]
    pub contract_state: Account<'info, ProgramState>,

    #[account(
        init_if_needed,
        payer = investor,
        space = 8 + LobbyEntry::INIT_SPACE,
        seeds = [LOBBY_ENTRY_SEED, investor.key().as_ref()],
        bump,
    )]
    pub lobby_entry: Account<'info, LobbyEntry>,

    pub usdc_mint: Box<InterfaceAccount<'info, Mint>>,
    pub aat_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        mut,
        token::mint = usdc_mint,
        token::authority = investor,
        token::token_program = usdc_token_program,
    )]
    pub investor_usdc_account: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        seeds = [USDC_VAULT_SEED],
        bump = contract_state.usdc_vault_bump,
        token::mint = usdc_mint,
        token::authority = contract_state,
        token::token_program = usdc_token_program,
    )]
    pub usdc_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        token::mint = aat_mint,
        token::authority = investor,
        token::token_program = aat_token_program,
    )]
    pub investor_aat_account: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        seeds = [AAT_VAULT_SEED],
        bump = contract_state.aat_vault_bump,
        token::mint = aat_mint,
        token::authority = contract_state,
        token::token_program = aat_token_program,
    )]
    pub aat_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    pub usdc_token_program: Interface<'info, TokenInterface>,
    pub aat_token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<DepositUsdc>, usdc_amount: u64, aat_amount: u64) -> Result<()> {
    require!(usdc_amount > 0 || aat_amount > 0, FloorError::ZeroAmount);

    let lobby_entry = &mut ctx.accounts.lobby_entry;

    if lobby_entry.investor == Pubkey::default() {
        lobby_entry.investor = ctx.accounts.investor.key();
        lobby_entry.bump = ctx.bumps.lobby_entry;
    }

    require!(
        lobby_entry.investor == ctx.accounts.investor.key(),
        FloorError::InvalidInvestor
    );

    if usdc_amount > 0 {
        transfer_checked(
            CpiContext::new(
                ctx.accounts.usdc_token_program.to_account_info(),
                TransferChecked {
                    from: ctx.accounts.investor_usdc_account.to_account_info(),
                    mint: ctx.accounts.usdc_mint.to_account_info(),
                    to: ctx.accounts.usdc_vault.to_account_info(),
                    authority: ctx.accounts.investor.to_account_info(),
                },
            ),
            usdc_amount,
            ctx.accounts.usdc_mint.decimals,
        )?;

        lobby_entry.usdc_deposited = lobby_entry
            .usdc_deposited
            .checked_add(usdc_amount)
            .ok_or(FloorError::ArithmeticOverflow)?;

        ctx.accounts.contract_state.total_usdc_in_lobby = ctx
            .accounts
            .contract_state
            .total_usdc_in_lobby
            .checked_add(usdc_amount)
            .ok_or(FloorError::ArithmeticOverflow)?;
    }

    if aat_amount > 0 {
        transfer_checked(
            CpiContext::new(
                ctx.accounts.aat_token_program.to_account_info(),
                TransferChecked {
                    from: ctx.accounts.investor_aat_account.to_account_info(),
                    mint: ctx.accounts.aat_mint.to_account_info(),
                    to: ctx.accounts.aat_vault.to_account_info(),
                    authority: ctx.accounts.investor.to_account_info(),
                },
            ),
            aat_amount,
            ctx.accounts.aat_mint.decimals,
        )?;

        lobby_entry.aat_staked = lobby_entry
            .aat_staked
            .checked_add(aat_amount)
            .ok_or(FloorError::ArithmeticOverflow)?;

        ctx.accounts.contract_state.total_aat_staked = ctx
            .accounts
            .contract_state
            .total_aat_staked
            .checked_add(aat_amount)
            .ok_or(FloorError::ArithmeticOverflow)?;
    }

    Ok(())
}
