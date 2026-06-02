use anchor_lang::prelude::*;

#[cfg(not(feature = "no-entrypoint"))]
use solana_security_txt::security_txt;

#[cfg(not(feature = "no-entrypoint"))]
security_txt! {
    name: "ALN Floor Contract",
    project_url: "https://alien.org/",
    contacts: "email:aliensol@eti.gg, twitter:@alienorg",
    policy: "https://alien.org/sol-security-policy",
    preferred_languages: "en",
    source_code: "https://github.com/alien-id/solana-floor-program"
}

pub mod errors;
pub mod instructions;
pub mod utils;
pub mod seeds;
pub mod state;

use instructions::*;

declare_id!("3tBjDJHPCMmKjxRAxTHiaUmZ9Li1D8YvWCS48pBMjRSe");

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

    pub fn set_usdc_withdraw_lock(ctx: Context<AdminOnly>, new_lock_seconds: i64) -> Result<()> {
        admin::set_usdc_withdraw_lock(ctx, new_lock_seconds)
    }

    pub fn set_investor_usdc_unlock(
        ctx: Context<SetInvestorUsdcUnlock>,
        investor: Pubkey,
        new_unlock_ts: i64,
    ) -> Result<()> {
        admin::set_investor_usdc_unlock(ctx, investor, new_unlock_ts)
    }

    pub fn set_paused(ctx: Context<AdminOnly>, paused: bool) -> Result<()> {
        admin::set_paused(ctx, paused)
    }

    pub fn cancel_round(ctx: Context<CancelRound>) -> Result<()> {
        admin::cancel_round(ctx)
    }

    pub fn remove_investor_from_pool(ctx: Context<RemoveInvestorFromPool>) -> Result<()> {
        admin::remove_investor_from_pool(ctx)
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

    pub fn update_aat_volume(ctx: Context<UpdateAatVolume>, new_volume: u64) -> Result<()> {
        update_aat_volume::handler(ctx, new_volume)
    }

    pub fn transfer_authority(ctx: Context<TransferAuthority>) -> Result<()> {
        transfer_authority::transfer_authority(ctx)
    }

    pub fn accept_authority(ctx: Context<AcceptAuthority>) -> Result<()> {
        transfer_authority::accept_authority(ctx)
    }
}
