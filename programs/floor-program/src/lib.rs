use anchor_lang::prelude::*;

pub mod errors;
pub mod instructions;
pub mod nft_utils;
pub mod seeds;
pub mod state;

use instructions::*;

declare_id!("CEwVmSxQGdVWZwQozZPnwKPtCK837efbs3X9fTMfxz2v");

#[program]
pub mod floor_program {
    use super::*;

    pub fn initialize(
        ctx: Context<Initialize>,
        floor_price_usdc: u64,
        round_size_waln: u64,
        lock_period_seconds: i64,
    ) -> Result<()> {
        instructions::initialize::handler(ctx, floor_price_usdc, round_size_waln, lock_period_seconds)
    }

    pub fn set_floor_price(ctx: Context<AdminOnly>, new_price_usdc: u64) -> Result<()> {
        instructions::admin::set_floor_price(ctx, new_price_usdc)
    }

    pub fn set_round_size(ctx: Context<AdminOnly>, new_round_size_waln: u64) -> Result<()> {
        instructions::admin::set_round_size(ctx, new_round_size_waln)
    }

    pub fn set_lock_period(ctx: Context<AdminOnly>, new_lock_period: i64) -> Result<()> {
        instructions::admin::set_lock_period(ctx, new_lock_period)
    }

    pub fn set_paused(ctx: Context<AdminOnly>, paused: bool) -> Result<()> {
        instructions::admin::set_paused(ctx, paused)
    }

    pub fn deposit_usdc(ctx: Context<DepositUsdc>, usdc_amount: u64) -> Result<()> {
        instructions::deposit_usdc::handler(ctx, usdc_amount)
    }

    pub fn withdraw_usdc(ctx: Context<WithdrawUsdc>, amount: u64) -> Result<()> {
        instructions::withdraw_usdc::handler(ctx, amount)
    }

    pub fn sell_waln<'info>(ctx: Context<'_, '_, 'info, 'info, SellWaln<'info>>, waln_amount: u64) -> Result<()> {
        instructions::sell_waln::handler(ctx, waln_amount)
    }

    pub fn claim_waln(ctx: Context<ClaimWaln>, round_index: u64) -> Result<()> {
        instructions::claim_waln::handler(ctx, round_index)
    }

    pub fn mint_aat_nft(ctx: Context<MintAatNft>, aat_volume: u64) -> Result<()> {
        instructions::mint_aat_nft::handler(ctx, aat_volume)
    }
}
