use anchor_lang::prelude::*;

use crate::errors::FloorError;
use crate::seeds::{CONTRACT_STATE_SEED, ROUND_RECORD_SEED, TREASURY_SEED};
use crate::state::{ProgramState, RoundRecord};

#[derive(Accounts)]
#[instruction(round_index: u64)]
pub struct CloseRoundRecord<'info> {
    pub admin: Signer<'info>,

    #[account(
        seeds = [CONTRACT_STATE_SEED],
        bump,
        constraint = contract_state.load()?.admin == admin.key() @ FloorError::Unauthorized,
    )]
    pub contract_state: AccountLoader<'info, ProgramState>,

    #[account(
        mut,
        close = treasury,
        seeds = [ROUND_RECORD_SEED, &round_index.to_le_bytes()],
        bump,
    )]
    pub round_record: Account<'info, RoundRecord>,

    /// CHECK: Treasury PDA - system-owned, receives the reclaimed rent
    #[account(
        mut,
        seeds = [TREASURY_SEED],
        bump,
    )]
    pub treasury: UncheckedAccount<'info>,
}

pub fn close_round_record(_ctx: Context<CloseRoundRecord>, _round_index: u64) -> Result<()> {
    Ok(())
}
