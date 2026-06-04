use anchor_lang::prelude::*;
use anchor_lang::solana_program::program::invoke_signed;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token_2022::spl_token_2022::instruction::transfer_checked as build_transfer_checked_ix;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

use crate::errors::FloorError;
use crate::seeds::{CONTRACT_STATE_SEED, ROUND_LOCKED_WALN_SEED, TREASURY_SEED, WALN_VAULT_SEED};
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
        bump = round_locked_waln.bump,
    )]
    pub round_locked_waln: Account<'info, RoundLockedWaln>,

    /// CHECK: Treasury PDA — receives the rent refund when the round account closes
    #[account(
        mut,
        seeds = [TREASURY_SEED],
        bump,
    )]
    pub treasury: UncheckedAccount<'info>,

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

pub fn handler<'info>(ctx: Context<'_, '_, 'info, 'info, ClaimWaln<'info>>, _round_index: u64) -> Result<()> {
    let state_bump;
    let waln_mint_key;
    {
        let state = ctx.accounts.contract_state.load()?;
        require!(state.frozen == 0, FloorError::ContractFrozen);
        state_bump = state.bump;
        waln_mint_key = state.waln_mint;
    }
    require!(
        ctx.accounts.waln_mint.key() == waln_mint_key,
        FloorError::InvalidMint
    );

    let investor_key = ctx.accounts.investor.key();

    let waln_amount;
    let should_close;
    {
        let rlw = &mut ctx.accounts.round_locked_waln;
        let unlock_ts = rlw.unlock;
        let clock = Clock::get()?;
        require!(
            clock.unix_timestamp >= unlock_ts,
            FloorError::NotYetUnlocked
        );

        let idx = rlw
            .investors
            .binary_search_by_key(&investor_key.to_bytes(), |a| a.investor.to_bytes())
            .map_err(|_| error!(FloorError::InvalidInvestor))?;

        {
            let alloc = &mut rlw.investors[idx];
            require!(alloc.waln_amount > 0, FloorError::AlreadyClaimed);
            waln_amount = alloc.waln_amount;
            alloc.waln_amount = 0;
        }

        rlw.remaining_to_claim = rlw
            .remaining_to_claim
            .checked_sub(1)
            .ok_or(FloorError::ArithmeticOverflow)?;
        should_close = rlw.remaining_to_claim == 0;
    }

    let seeds: &[&[u8]] = &[CONTRACT_STATE_SEED, &[state_bump]];
    let signer = &[seeds];

    if let Ok(hook_program_id) = get_hook_program_id(&ctx.accounts.waln_mint.to_account_info()) {
        validate_hook_accounts(
            ctx.remaining_accounts,
            &ctx.accounts.waln_mint.key(),
            &ctx.accounts.contract_state.key(),
            &hook_program_id,
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

    if should_close {
        let rlw_info = ctx.accounts.round_locked_waln.to_account_info();
        let treasury_info = ctx.accounts.treasury.to_account_info();

        let dest_starting = treasury_info.lamports();
        **treasury_info.try_borrow_mut_lamports()? = dest_starting
            .checked_add(rlw_info.lamports())
            .ok_or(FloorError::ArithmeticOverflow)?;
        **rlw_info.try_borrow_mut_lamports()? = 0;
        rlw_info.assign(&System::id());
        rlw_info.resize(0)?;
    }

    Ok(())
}
