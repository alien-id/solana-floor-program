use anchor_lang::prelude::*;

use crate::errors::FloorError;
use crate::seeds::{CONTRACT_STATE_SEED, INVESTOR_POOL_SEED};
use crate::state::{InvestorPool, ProgramState};

#[derive(Accounts)]
pub struct RemoveInvestorFromPool<'info> {
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
        seeds = [INVESTOR_POOL_SEED],
        bump,
    )]
    pub investor_pool: Account<'info, InvestorPool>,
}

pub fn handler(ctx: Context<RemoveInvestorFromPool>, investor: Pubkey) -> Result<()> {
    {
        let pool = &mut ctx.accounts.investor_pool;

        let idx = pool
            .investors
            .iter()
            .position(|r| r.investor == investor)
            .ok_or(FloorError::InvalidInvestor)?;

        let record = &pool.investors[idx];
        require!(record.aat_volume == 0, FloorError::InvalidParameter);
        require!(record.usdc_deposited == 0, FloorError::InsufficientFunds);
        require!(
            record.usdc_locked_current_round == 0,
            FloorError::RoundInProgress
        );

        pool.investors.swap_remove(idx);
    }

    let new_len = ctx.accounts.investor_pool.investors.len();
    let new_size = InvestorPool::space(new_len);
    let pool_info = ctx.accounts.investor_pool.to_account_info();
    let rent = Rent::get()?.minimum_balance(new_size);
    let cur_lamports = pool_info.lamports();

    if cur_lamports > rent {
        let excess = cur_lamports - rent;
        **pool_info.try_borrow_mut_lamports()? -= excess;
        **ctx.accounts.admin.to_account_info().try_borrow_mut_lamports()? += excess;
    }

    pool_info.resize(new_size)?;

    Ok(())
}
