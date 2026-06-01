use anchor_lang::prelude::*;
use anchor_lang::solana_program::program::invoke_signed;
use anchor_spl::token_2022::Token2022;

use crate::errors::FloorError;
use crate::instructions::mint_aat_nft::MAX_TOTAL_AAT_VOLUME;
use crate::seeds::{AAT_NFT_SEED, CONTRACT_STATE_SEED, INVESTOR_POOL_SEED};
use crate::state::{AatNftAuthority, InvestorPool, ProgramState};
use crate::utils::verify_aat_nft_and_get_allocation;

#[derive(Accounts)]
pub struct UpdateAatVolume<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(
        mut,
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
    pub investor_pool: AccountLoader<'info, InvestorPool>,

    /// CHECK: investor wallet whose AAT NFT allocation is being updated
    pub investor: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [AAT_NFT_SEED, investor.key().as_ref()],
        bump,
    )]
    /// CHECK: Token-2022 AAT NFT mint; verified in handler via verify_aat_nft_and_get_allocation
    pub mint: UncheckedAccount<'info>,

    #[account(
        seeds = [b"nft_authority"],
        bump,
    )]
    pub nft_authority: Account<'info, AatNftAuthority>,

    pub token_program: Program<'info, Token2022>,
    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<UpdateAatVolume>, new_volume: u64) -> Result<()> {
    let investor_key = ctx.accounts.investor.key();

    {
        let state = ctx.accounts.contract_state.load()?;
        require!(state.round_started == 0, FloorError::FundsLocked);
    }

    let old_volume = verify_aat_nft_and_get_allocation(
        &ctx.accounts.mint.to_account_info(),
        &investor_key,
    )?;

    {
        let mut state = ctx.accounts.contract_state.load_mut()?;
        let new_total = state
            .total_aat_volume
            .checked_sub(old_volume)
            .ok_or(FloorError::ArithmeticOverflow)?
            .checked_add(new_volume)
            .ok_or(FloorError::ArithmeticOverflow)?;
        require!(
            new_total <= MAX_TOTAL_AAT_VOLUME,
            FloorError::WalnAllocationLimitExceeded
        );
        state.total_aat_volume = new_total;
    }

    let nft_authority_bump = ctx.bumps.nft_authority;
    let nft_signer: &[&[&[u8]]] = &[&[b"nft_authority", &[nft_authority_bump]]];

    invoke_signed(
        &spl_token_metadata_interface::instruction::update_field(
            &spl_token_2022::id(),
            ctx.accounts.mint.key,
            ctx.accounts.nft_authority.to_account_info().key,
            spl_token_metadata_interface::state::Field::Key("aat_volume".to_string()),
            new_volume.to_string(),
        ),
        &[
            ctx.accounts.mint.to_account_info(),
            ctx.accounts.nft_authority.to_account_info(),
        ],
        nft_signer,
    )?;

    {
        let mut pool = ctx.accounts.investor_pool.load_mut()?;
        let count = pool.count as usize;
        if let Some(record) = pool.investors[..count]
            .iter_mut()
            .find(|r| r.investor == investor_key)
        {
            record.aat_volume = new_volume;
        }
    }

    Ok(())
}
