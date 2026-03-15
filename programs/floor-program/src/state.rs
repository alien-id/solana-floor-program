use anchor_lang::prelude::*;

#[account]
#[derive(InitSpace)]
pub struct ProgramState {
    pub admin: Pubkey,
    pub usdc_mint: Pubkey,
    pub waln_mint: Pubkey,
    pub aat_mint: Pubkey,
    pub usdc_vault: Pubkey,
    pub waln_vault: Pubkey,
    pub aat_vault: Pubkey,
    pub floor_price_usdc: u64,
    pub round_size_waln: u64,
    pub current_round_waln: u64,
    pub total_usdc_in_lobby: u64,
    pub total_aat_staked: u64,
    pub round_count: u64,
    pub lock_period_seconds: i64,
    pub paused: bool,
    pub round_started: bool,
    pub bump: u8,
    pub usdc_vault_bump: u8,
    pub waln_vault_bump: u8,
    pub aat_vault_bump: u8,
}

#[account]
#[derive(InitSpace)]
pub struct LobbyEntry {
    pub investor: Pubkey,
    pub usdc_deposited: u64,
    pub usdc_locked_current_round: u64,
    pub usdc_committed: u64,
    pub aat_staked: u64,
    pub waln_purchased_total: u64,
    pub bump: u8,
}

#[account]
#[derive(InitSpace)]
pub struct LockedWaln {
    pub investor: Pubkey,
    pub round_index: u64,
    pub waln_amount: u64,
    pub unlock: i64,
    pub claimed: bool,
    pub bump: u8,
}

#[account]
#[derive(InitSpace)]
pub struct RoundRecord {
    pub round_index: u64,
    pub triggered_at: i64,
    pub waln_purchased: u64,
    pub usdc_spent: u64,
    pub total_aat_staked_at_trigger: u64,
    pub participant_count: u32,
    pub bump: u8,
}
