use anchor_lang::prelude::*;

pub const MAX_INVESTORS: usize = 100;

#[zero_copy]
pub struct InvestorAlloc {
    pub investor: Pubkey,
    pub waln_amount: u64,
    pub unlock: i64,
    pub claimed: u8,
    pub _pad: [u8; 7],
}

#[account(zero_copy)]
pub struct RoundLockedWaln {
    pub round_index: u64,
    pub count: u32,
    pub bump: u8,
    pub _pad: [u8; 3],
    pub investors: [InvestorAlloc; MAX_INVESTORS],
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
    pub paused: u8,
    pub round_started: u8,
    pub bump: u8,
    pub usdc_vault_bump: u8,
    pub waln_vault_bump: u8,
    pub waln_decimals: u8,
    pub usdc_decimals: u8,
    pub _padding: [u8; 1],
}


#[account]
pub struct AatNftAuthority {}

#[zero_copy]
pub struct InvestorRecord {
    pub investor: Pubkey,
    pub usdc_deposited: u64,
    pub usdc_locked_current_round: u64,
    pub usdc_committed: u64,
    pub waln_purchased_total: u64,
    pub aat_volume: u64,
    pub usdc_unlock_ts: i64,
}

#[account(zero_copy)]
pub struct InvestorPool {
    pub count: u32,
    pub bump: u8,
    pub _padding: [u8; 3],
    pub investors: [InvestorRecord; MAX_INVESTORS],
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
