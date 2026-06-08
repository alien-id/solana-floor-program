use crate::errors::FloorError;
use crate::seeds::{CONTRACT_STATE_SEED, ROUND_LOCKED_WALN_SEED, TREASURY_SEED, WALN_VAULT_SEED};
use crate::state::{ProgramState, RoundLockedWaln, WalnClaimed};
use crate::utils::{get_hook_program_id, validate_hook_accounts};
use anchor_lang::prelude::*;
use anchor_lang::solana_program::program::invoke_signed;
use anchor_spl::associated_token::{
    create_idempotent, get_associated_token_address_with_program_id, AssociatedToken, Create,
};
use anchor_spl::token_2022::spl_token_2022::instruction::transfer_checked as build_transfer_checked_ix;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

/// Number of `remaining_accounts` consumed per investor: [wallet, ata].
const PER_INVESTOR: usize = 2;
/// Number of shared transfer-hook accounts prepended once when the mint has a hook.
/// These are derived from the mint + source authority (`contract_state`), so they are
/// identical for every recipient and only need to be supplied once per transaction.
const HOOK_ACCOUNTS: usize = 8;

#[derive(Accounts)]
#[instruction(round_index: u64)]
pub struct FinalizeClaimForAll<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(
        seeds = [CONTRACT_STATE_SEED],
        bump,
        constraint = contract_state.load()?.admin == admin.key() @ FloorError::Unauthorized,
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

/// Admin-driven settlement of a round's locked wALN. For each investor passed in
/// `remaining_accounts` the program creates their associated token account if it is
/// missing (admin pays rent), pushes their wALN allocation out of the vault, and zeroes
/// their entry. Once every allocation has been settled the round account is closed and
/// its rent is refunded to the treasury. May be called in batches.
///
/// `remaining_accounts` layout:
///   [0..HOOK_ACCOUNTS]  shared transfer-hook accounts (omitted when the mint has no hook)
///   then, repeated per investor:
///     [+0] investor wallet (the ATA owner)
///     [+1] investor wALN associated token account (writable)
pub fn handler<'info>(
    ctx: Context<'_, '_, 'info, 'info, FinalizeClaimForAll<'info>>,
    round_index: u64,
) -> Result<()> {
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

    {
        let clock = Clock::get()?;
        require!(
            clock.unix_timestamp >= ctx.accounts.round_locked_waln.unlock,
            FloorError::NotYetUnlocked
        );
    }

    let mint_key = ctx.accounts.waln_mint.key();
    let token_program_key = ctx.accounts.waln_token_program.key();
    let contract_state_key = ctx.accounts.contract_state.key();
    let decimals = ctx.accounts.waln_mint.decimals;

    let hook_program_id = get_hook_program_id(&ctx.accounts.waln_mint.to_account_info()).ok();
    let hook_len = if hook_program_id.is_some() { HOOK_ACCOUNTS } else { 0 };

    require!(
        ctx.remaining_accounts.len() > hook_len
            && (ctx.remaining_accounts.len() - hook_len) % PER_INVESTOR == 0,
        FloorError::InvalidRemainingAccounts
    );

    // Shared transfer-hook accounts (same for every recipient) — validate once.
    let hook_accs = &ctx.remaining_accounts[..hook_len];
    if let Some(hook_program_id) = hook_program_id {
        validate_hook_accounts(hook_accs, &mint_key, &contract_state_key, &hook_program_id)?;
    }

    let batch = (ctx.remaining_accounts.len() - hook_len) / PER_INVESTOR;

    let seeds: &[&[u8]] = &[CONTRACT_STATE_SEED, &[state_bump]];
    let signer = &[seeds];

    for i in 0..batch {
        let base = hook_len + i * PER_INVESTOR;
        let investor_wallet = &ctx.remaining_accounts[base];
        let investor_ata = &ctx.remaining_accounts[base + 1];

        let investor_key = investor_wallet.key();

        let waln_amount = {
            let rlw = &mut ctx.accounts.round_locked_waln;
            let idx = rlw
                .investors
                .binary_search_by_key(&investor_key.to_bytes(), |a| a.investor.to_bytes())
                .map_err(|_| error!(FloorError::InvalidInvestor))?;
            let alloc = &mut rlw.investors[idx];
            require!(alloc.waln_amount > 0, FloorError::AlreadyClaimed);
            let amount = alloc.waln_amount;
            alloc.waln_amount = 0;
            rlw.remaining_to_claim = rlw
                .remaining_to_claim
                .checked_sub(1)
                .ok_or(FloorError::ArithmeticOverflow)?;
            amount
        };

        let expected_ata =
            get_associated_token_address_with_program_id(&investor_key, &mint_key, &token_program_key);
        require!(
            investor_ata.key() == expected_ata,
            FloorError::InvalidRemainingAccounts
        );

        create_idempotent(CpiContext::new(
            ctx.accounts.associated_token_program.to_account_info(),
            Create {
                payer: ctx.accounts.admin.to_account_info(),
                associated_token: investor_ata.clone(),
                authority: investor_wallet.clone(),
                mint: ctx.accounts.waln_mint.to_account_info(),
                system_program: ctx.accounts.system_program.to_account_info(),
                token_program: ctx.accounts.waln_token_program.to_account_info(),
            },
        ))?;

        let mut ix = build_transfer_checked_ix(
            &token_program_key,
            &ctx.accounts.waln_vault.key(),
            &mint_key,
            &investor_ata.key(),
            &contract_state_key,
            &[],
            waln_amount,
            decimals,
        )?;
        for acc in hook_accs.iter() {
            ix.accounts.push(AccountMeta {
                pubkey: acc.key(),
                is_signer: acc.is_signer,
                is_writable: acc.is_writable,
            });
        }

        let mut account_infos = vec![
            ctx.accounts.waln_vault.to_account_info(),
            ctx.accounts.waln_mint.to_account_info(),
            investor_ata.clone(),
            ctx.accounts.contract_state.to_account_info(),
        ];
        for acc in hook_accs.iter() {
            account_infos.push(acc.clone());
        }

        invoke_signed(&ix, &account_infos, signer)?;

        emit!(WalnClaimed {
            round_index,
            investor: investor_key,
            waln_amount,
        });
    }

    if ctx.accounts.round_locked_waln.remaining_to_claim == 0 {
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
