use anchor_lang::prelude::*;

use crate::errors::FloorError;
use crate::seeds::CONTRACT_STATE_SEED;
use crate::state::ProgramState;

#[derive(Accounts)]
pub struct TransferAuthority<'info> {
    pub admin: Signer<'info>,

    /// CHECK: The new authority; validated by the caller.
    pub new_admin: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [CONTRACT_STATE_SEED],
        bump,
        constraint = contract_state.load()?.admin == admin.key() @ FloorError::Unauthorized,
    )]
    pub contract_state: AccountLoader<'info, ProgramState>,
}

pub fn transfer_authority(ctx: Context<TransferAuthority>) -> Result<()> {
    let mut state = ctx.accounts.contract_state.load_mut()?;
    state.pending_admin = ctx.accounts.new_admin.key();
    Ok(())
}

#[derive(Accounts)]
pub struct AcceptAuthority<'info> {
    pub pending_admin: Signer<'info>,

    #[account(
        mut,
        seeds = [CONTRACT_STATE_SEED],
        bump,
        constraint = contract_state.load()?.pending_admin != Pubkey::default() @ FloorError::NoPendingAdmin,
        constraint = contract_state.load()?.pending_admin == pending_admin.key() @ FloorError::NotPendingAdmin,
    )]
    pub contract_state: AccountLoader<'info, ProgramState>,
}

pub fn accept_authority(ctx: Context<AcceptAuthority>) -> Result<()> {
    let mut state = ctx.accounts.contract_state.load_mut()?;
    state.admin = state.pending_admin;
    state.pending_admin = Pubkey::default();
    Ok(())
}
