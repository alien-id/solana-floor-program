use anchor_lang::prelude::*;
use anchor_lang::solana_program::{program::invoke, system_instruction};
use anchor_spl::token_2022::Token2022;
use anchor_spl::token_interface::{
    transfer_checked, Mint, TokenAccount, TokenInterface, TransferChecked,
};

use crate::errors::FloorError;
use crate::utils::verify_aat_nft_and_get_allocation;
use crate::seeds::{CONTRACT_STATE_SEED, INVESTOR_POOL_SEED, USDC_VAULT_SEED};
use crate::state::{InvestorPool, InvestorRecord, ProgramState};

#[derive(Accounts)]
pub struct DepositUsdc<'info> {
    #[account(mut)]
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
    pub investor_pool: Account<'info, InvestorPool>,

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
        bump,
        token::mint = usdc_mint,
        token::authority = contract_state,
        token::token_program = usdc_token_program,
    )]
    pub usdc_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    /// CHECK: Verified in handler via verify_aat_nft_and_get_allocation
    pub aat_nft: UncheckedAccount<'info>,
    #[account(
        associated_token::mint = aat_nft,
        associated_token::authority = investor,
        associated_token::token_program = aat_token_program,
    )]
    pub investor_aat_account: Box<InterfaceAccount<'info, TokenAccount>>,

    pub aat_token_program: Program<'info, Token2022>,
    pub usdc_token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<DepositUsdc>, usdc_amount: u64) -> Result<()> {
    let usdc_decimals;
    let usdc_mint_key;
    {
        let state = ctx.accounts.contract_state.load()?;
        require!(state.frozen == 0, FloorError::ContractFrozen);
        require!(usdc_amount > 0, FloorError::ZeroAmount);
        usdc_mint_key = state.usdc_mint;
        usdc_decimals = ctx.accounts.usdc_mint.decimals;
    }
    require!(
        ctx.accounts.usdc_mint.key() == usdc_mint_key,
        FloorError::InvalidMint
    );

    let aat_vol = verify_aat_nft_and_get_allocation(
        &ctx.accounts.aat_nft.to_account_info(),
        &ctx.accounts.investor.key(),
    )?;
    require!(aat_vol > 0, FloorError::ZeroAatAllocation);

    require!(
        ctx.accounts.investor_aat_account.amount == 1,
        FloorError::NoAatNft
    );

    let usdc_withdraw_lock_seconds;
    {
        let state = ctx.accounts.contract_state.load()?;
        usdc_withdraw_lock_seconds = state.usdc_withdraw_lock_seconds;
    }

    let investor_key = ctx.accounts.investor.key();

    // Grow pool when this is a new investor
    let exists = ctx
        .accounts
        .investor_pool
        .investors
        .iter()
        .any(|r| r.investor == investor_key);

    if !exists {
        let new_len = ctx.accounts.investor_pool.investors.len() + 1;
        let new_size = InvestorPool::space(new_len);
        let pool_info = ctx.accounts.investor_pool.to_account_info();
        let rent = Rent::get()?.minimum_balance(new_size);
        let cur_lamports = pool_info.lamports();
        if cur_lamports < rent {
            let diff = rent - cur_lamports;
            invoke(
                &system_instruction::transfer(
                    &ctx.accounts.investor.key(),
                    pool_info.key,
                    diff,
                ),
                &[
                    ctx.accounts.investor.to_account_info(),
                    pool_info.clone(),
                    ctx.accounts.system_program.to_account_info(),
                ],
            )?;
        }
        pool_info.resize(new_size)?;

        ctx.accounts.investor_pool.investors.push(InvestorRecord {
            investor: investor_key,
            usdc_deposited: 0,
            usdc_locked_current_round: 0,
            usdc_committed: 0,
            waln_purchased_total: 0,
            aat_volume: 0,
            usdc_unlock_ts: 0,
        });
    }

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
        usdc_decimals,
    )?;

    let record = ctx
        .accounts
        .investor_pool
        .investors
        .iter_mut()
        .find(|r| r.investor == investor_key)
        .ok_or(FloorError::InvalidInvestor)?;

    record.aat_volume = aat_vol;
    record.usdc_deposited = record
        .usdc_deposited
        .checked_add(usdc_amount)
        .ok_or(FloorError::ArithmeticOverflow)?;

    if usdc_withdraw_lock_seconds > 0 {
        let now = Clock::get()?.unix_timestamp;
        let new_unlock_ts = now
            .checked_add(usdc_withdraw_lock_seconds)
            .ok_or(FloorError::ArithmeticOverflow)?;
        record.usdc_unlock_ts = record.usdc_unlock_ts.max(new_unlock_ts);
    }

    let mut state = ctx.accounts.contract_state.load_mut()?;
    state.total_usdc_in_lobby = state
        .total_usdc_in_lobby
        .checked_add(usdc_amount)
        .ok_or(FloorError::ArithmeticOverflow)?;

    Ok(())
}
