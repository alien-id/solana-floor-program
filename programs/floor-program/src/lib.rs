use anchor_lang::prelude::*;

pub mod errors;
pub mod instructions;
pub mod utils;
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
        initialize::handler(
            ctx,
            floor_price_usdc,
            round_size_waln,
            lock_period_seconds,
        )
    }

    pub fn set_floor_price(ctx: Context<AdminOnly>, new_price_usdc: u64) -> Result<()> {
        admin::set_floor_price(ctx, new_price_usdc)
    }

    pub fn set_round_size(ctx: Context<AdminOnly>, new_round_size_waln: u64) -> Result<()> {
        admin::set_round_size(ctx, new_round_size_waln)
    }

    pub fn set_lock_period(ctx: Context<AdminOnly>, new_lock_period: i64) -> Result<()> {
        admin::set_lock_period(ctx, new_lock_period)
    }

    pub fn set_paused(ctx: Context<AdminOnly>, paused: bool) -> Result<()> {
        admin::set_paused(ctx, paused)
    }

    pub fn cancel_round(ctx: Context<CancelRound>) -> Result<()> {
        admin::cancel_round(ctx)
    }

    pub fn fund_treasury(ctx: Context<FundTreasury>, amount: u64) -> Result<()> {
        admin::fund_treasury(ctx, amount)
    }

    pub fn deposit_usdc(ctx: Context<DepositUsdc>, usdc_amount: u64) -> Result<()> {
        deposit_usdc::handler(ctx, usdc_amount)
    }

    pub fn withdraw_usdc(ctx: Context<WithdrawUsdc>, amount: u64) -> Result<()> {
        withdraw_usdc::handler(ctx, amount)
    }

    pub fn sell_waln<'info>(
        ctx: Context<'_, '_, 'info, 'info, SellWaln<'info>>,
        waln_amount: u64,
        hook_bumps: [u8; 4],
    ) -> Result<()> {
        sell_waln::handler(ctx, waln_amount, hook_bumps)
    }

    pub fn claim_waln<'info>(
        ctx: Context<'_, '_, 'info, 'info, ClaimWaln<'info>>,
        round_index: u64,
        hook_bumps: [u8; 4],
    ) -> Result<()> {
        claim_waln::handler(ctx, round_index, hook_bumps)
    }

    pub fn mint_aat_nft(ctx: Context<MintAatNft>, aat_volume: u64) -> Result<()> {
        mint_aat_nft::handler(ctx, aat_volume)
    }
}
