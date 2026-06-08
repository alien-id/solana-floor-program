use anchor_lang::prelude::*;

pub const MIN_SELL_WALN: u64 = 1_000_000_000;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug)]
pub struct InvestorAlloc {
    pub investor: Pubkey,
    pub waln_amount: u64,
}

impl InvestorAlloc {
    pub const SIZE: usize = 32 + 8;
}

#[account]
pub struct RoundLockedWaln {
    pub round_index: u64,
    pub bump: u8,
    pub unlock: i64,
    pub remaining_to_claim: u32,
    pub investors: Vec<InvestorAlloc>,
}

impl RoundLockedWaln {
    pub const FIXED_SIZE: usize = 8 + 1 + 8 + 4 + 4;
    pub fn space(n: usize) -> usize {
        8 + Self::FIXED_SIZE + n * InvestorAlloc::SIZE
    }
}

#[account(zero_copy)]
pub struct ProgramState {
    pub admin: Pubkey,
    pub usdc_mint: Pubkey,
    pub waln_mint: Pubkey,
    pub usdc_vault: Pubkey,
    pub waln_vault: Pubkey,
    pub floor_price_usdc: u64,
    pub round_size_waln: u64,
    pub current_round_waln: u64,
    pub total_usdc_in_lobby: u64,
    pub round_count: u64,
    pub lock_period_seconds: i64,
    pub usdc_withdraw_lock_seconds: i64,
    pub current_round_floor_price: u64,
    pub current_round_size_waln: u64,
    pub current_round_lock_period: i64,
    pub current_round_usdc_spent: u64,
    pub total_aat_volume: u64,
    pub waln_dust_carryover: u64,
    pub total_usdc_locked_for_round: u64,
    pub pending_admin: Pubkey,
    pub sell_paused: u8,
    pub round_started: u8,
    pub bump: u8,
    pub usdc_vault_bump: u8,
    pub waln_vault_bump: u8,
    pub waln_decimals: u8,
    pub usdc_decimals: u8,
    pub frozen: u8,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug)]
pub struct InvestorRecord {
    pub investor: Pubkey,
    pub usdc_deposited: u64,
    pub usdc_locked_current_round: u64,
    pub usdc_committed: u64,
    pub waln_purchased_total: u64,
    pub aat_volume: u64,
    pub usdc_unlock_ts: i64,
}

impl InvestorRecord {
    pub const SIZE: usize = 32 + 8 + 8 + 8 + 8 + 8 + 8;
}

#[account]
pub struct InvestorPool {
    pub bump: u8,
    pub investors: Vec<InvestorRecord>,
}

impl InvestorPool {
    pub const FIXED_SIZE: usize = 1 + 4;
    pub fn space(n: usize) -> usize {
        8 + Self::FIXED_SIZE + n * InvestorRecord::SIZE
    }
}

#[account]
#[derive(InitSpace)]
pub struct RoundRecord {
    pub round_index: u64,
    pub triggered_at: i64,
    pub waln_purchased: u64,
    pub usdc_spent: u64,
    pub total_aat_volume_at_trigger: u64,
    pub participant_count: u32,
    pub bump: u8,
}

#[event]
pub struct InvestorAllocated {
    pub round_index: u64,
    pub investor: Pubkey,
    pub waln_amount: u64,
    pub usdc_spent: u64,
    pub unlock: i64,
}

#[event]
pub struct RoundClosed {
    pub round_index: u64,
    pub waln_purchased: u64,
    pub usdc_spent: u64,
    pub participant_count: u32,
    pub unlock: i64,
}

/// Emitted once per investor whose locked wALN allocation is settled, by either the
/// investor's own `claim_waln` or the admin's `finalize_claim_for_all`.
#[event]
pub struct WalnClaimed {
    pub round_index: u64,
    pub investor: Pubkey,
    pub waln_amount: u64,
}
