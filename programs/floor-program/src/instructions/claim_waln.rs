use anchor_lang::prelude::*;
use anchor_lang::solana_program::program::invoke_signed;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};
use anchor_spl::token_2022::spl_token_2022::instruction::transfer_checked as build_transfer_checked_ix;
use crate::errors::FloorError;
use crate::seeds::{CONTRACT_STATE_SEED, ROUND_LOCKED_WALN_SEED, WALN_VAULT_SEED};
use crate::state::{ProgramState, RoundLockedWaln};
use crate::utils::{get_hook_program_id, validate_hook_accounts};

#[derive(Accounts)]
#[instruction(round_index: u64)]
pub struct ClaimWaln<'info> {
    #[account(mut)]
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
        init_if_needed,
        payer = investor,
        associated_token::mint = waln_mint,
        associated_token::authority = investor,
        associated_token::token_program = waln_token_program,
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

    #[account(address = anchor_spl::token_2022::ID)]
    pub waln_token_program: Interface<'info, TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn handler<'info>(ctx: Context<'_, '_, 'info, 'info, ClaimWaln<'info>>, _round_index: u64, hook_bumps: [u8; 4]) -> Result<()> {
    let state_bump;
    let waln_mint_key;
    {
        let state = ctx.accounts.contract_state.load()?;
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

    if let Ok(hook_program_id) = get_hook_program_id(&ctx.accounts.waln_mint.to_account_info()) {
        validate_hook_accounts(
            ctx.remaining_accounts,
            &ctx.accounts.waln_mint.key(),
            &ctx.accounts.contract_state.key(),
            &hook_program_id,
            hook_bumps,
        )?;
    }

    let mut ix = build_transfer_checked_ix(
        &ctx.accounts.waln_token_program.key(),
        &ctx.accounts.waln_vault.key(),
        &ctx.accounts.waln_mint.key(),
        &ctx.accounts.investor_waln_account.key(),
        &ctx.accounts.contract_state.key(),
        &[],
        waln_amount,
        ctx.accounts.waln_mint.decimals,
    )?;

    for acc in ctx.remaining_accounts.iter() {
        ix.accounts.push(AccountMeta {
            pubkey: acc.key(),
            is_signer: acc.is_signer,
            is_writable: acc.is_writable,
        });
    }

    let mut account_infos = vec![
        ctx.accounts.waln_vault.to_account_info(),
        ctx.accounts.waln_mint.to_account_info(),
        ctx.accounts.investor_waln_account.to_account_info(),
        ctx.accounts.contract_state.to_account_info(),
    ];
    for acc in ctx.remaining_accounts.iter() {
        account_infos.push(acc.clone());
    }

    invoke_signed(&ix, &account_infos, signer)?;
    Ok(())
}
