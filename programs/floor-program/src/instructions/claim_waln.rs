use anchor_lang::prelude::*;
use anchor_spl::token_interface::{
    transfer_checked, Mint, TokenAccount, TokenInterface, TransferChecked,
};

use crate::errors::FloorError;
use crate::seeds::{CONTRACT_STATE_SEED, LOCKED_WALN_SEED, WALN_VAULT_SEED};
use crate::state::{ProgramState, LockedWaln};

#[derive(Accounts)]
#[instruction(round_index: u64)]
pub struct ClaimWaln<'info> {
    pub investor: Signer<'info>,

    #[account(
        seeds = [CONTRACT_STATE_SEED],
        bump,
    )]
    pub contract_state: AccountLoader<'info, ProgramState>,

    #[account(
        mut,
        seeds = [LOCKED_WALN_SEED, investor.key().as_ref(), &round_index.to_le_bytes()],
        bump = locked_waln.bump,
        constraint = locked_waln.investor == investor.key() @ FloorError::InvalidInvestor,
    )]
    pub locked_waln: Account<'info, LockedWaln>,

    pub waln_mint: InterfaceAccount<'info, Mint>,

    #[account(
        mut,
        token::mint = waln_mint,
        token::authority = investor,
        token::token_program = waln_token_program,
    )]
    pub investor_waln_account: InterfaceAccount<'info, TokenAccount>,

    #[account(
        mut,
        seeds = [WALN_VAULT_SEED],
        bump,
        token::mint = waln_mint,
        token::authority = contract_state,
        token::token_program = waln_token_program,
    )]
    pub waln_vault: InterfaceAccount<'info, TokenAccount>,

    pub waln_token_program: Interface<'info, TokenInterface>,
}

pub fn handler(ctx: Context<ClaimWaln>, _round_index: u64) -> Result<()> {
    let state_bump;
    let waln_mint_key;
    {
        let state = ctx.accounts.contract_state.load()?;
        require!(state.paused == 0, FloorError::ContractPaused);
        state_bump = state.bump;
        waln_mint_key = state.waln_mint;
    }
    require!(ctx.accounts.waln_mint.key() == waln_mint_key, FloorError::InvalidMint);

    let locked_waln = &mut ctx.accounts.locked_waln;
    require!(!locked_waln.claimed, FloorError::AlreadyClaimed);

    let clock = Clock::get()?;
    require!(
        clock.unix_timestamp >= locked_waln.unlock,
        FloorError::NotYetUnlocked
    );

    let waln_amount = locked_waln.waln_amount;
    let seeds: &[&[u8]] = &[CONTRACT_STATE_SEED, &[state_bump]];
    let signer = &[seeds];

    transfer_checked(
        CpiContext::new_with_signer(
            ctx.accounts.waln_token_program.to_account_info(),
            TransferChecked {
                from: ctx.accounts.waln_vault.to_account_info(),
                mint: ctx.accounts.waln_mint.to_account_info(),
                to: ctx.accounts.investor_waln_account.to_account_info(),
                authority: ctx.accounts.contract_state.to_account_info(),
            },
            signer,
        ),
        waln_amount,
        ctx.accounts.waln_mint.decimals,
    )?;

    locked_waln.claimed = true;

    Ok(())
}
