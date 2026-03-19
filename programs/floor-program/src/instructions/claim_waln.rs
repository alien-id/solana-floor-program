use anchor_lang::prelude::*;
use anchor_spl::token_interface::{
    transfer_checked, Mint, TokenAccount, TokenInterface, TransferChecked,
};

use crate::errors::FloorError;
use crate::seeds::{CONTRACT_STATE_SEED, ROUND_LOCKED_WALN_SEED, WALN_VAULT_SEED};
use crate::state::{ProgramState, RoundLockedWaln};

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
        seeds = [ROUND_LOCKED_WALN_SEED, &round_index.to_le_bytes()],
        bump,
    )]
    pub round_locked_waln: AccountLoader<'info, RoundLockedWaln>,

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

    let investor_key = ctx.accounts.investor.key();

    let waln_amount;
    {
        let mut round_locked_waln = ctx.accounts.round_locked_waln.load_mut()?;
        let count = round_locked_waln.count as usize;

        let idx = round_locked_waln.investors[..count]
            .binary_search_by_key(&investor_key.to_bytes(), |a| a.investor.to_bytes())
            .map_err(|_| error!(FloorError::InvalidInvestor))?;

        let alloc = &mut round_locked_waln.investors[idx];

        require!(alloc.claimed == 0, FloorError::AlreadyClaimed);

        let clock = Clock::get()?;
        require!(
            clock.unix_timestamp >= alloc.unlock,
            FloorError::NotYetUnlocked
        );

        waln_amount = alloc.waln_amount;
        alloc.claimed = 1;
    }

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

    Ok(())
}
