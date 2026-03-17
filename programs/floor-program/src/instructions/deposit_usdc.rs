use anchor_lang::prelude::*;
use anchor_spl::token_interface::{
    transfer_checked, Mint, TokenAccount, TokenInterface, TransferChecked,
};

use crate::errors::FloorError;
use crate::nft_utils::verify_aat_nft_and_get_allocation;
use crate::seeds::{CONTRACT_STATE_SEED, LOBBY_ENTRY_SEED, USDC_VAULT_SEED};
use crate::state::{LobbyEntry, ProgramState};

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

    #[account(constraint = usdc_mint.key() == contract_state.usdc_mint @ FloorError::InvalidMint)]
    pub usdc_mint: Box<InterfaceAccount<'info, Mint>>,

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

    /// CHECK: Verified in handler via verify_aat_nft_and_get_allocation
    pub aat_nft: UncheckedAccount<'info>,

    pub usdc_token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<DepositUsdc>, usdc_amount: u64) -> Result<()> {
    require!(!ctx.accounts.contract_state.paused, FloorError::ContractPaused);
    require!(usdc_amount > 0, FloorError::ZeroAmount);

    verify_aat_nft_and_get_allocation(
        &ctx.accounts.aat_nft.to_account_info(),
        &ctx.accounts.investor.key(),
    )?;

    let lobby_entry = &mut ctx.accounts.lobby_entry;

    if lobby_entry.investor == Pubkey::default() {
        lobby_entry.investor = ctx.accounts.investor.key();
        lobby_entry.bump = ctx.bumps.lobby_entry;
    }

    require!(
        lobby_entry.investor == ctx.accounts.investor.key(),
        FloorError::InvalidInvestor
    );

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

    Ok(())
}
