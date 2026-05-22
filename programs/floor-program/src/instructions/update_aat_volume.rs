use anchor_lang::prelude::*;
use anchor_lang::solana_program::program::invoke_signed;
use anchor_spl::token_2022::Token2022;
use spl_token_2022::extension::{BaseStateWithExtensions, StateWithExtensions};
use spl_token_2022::state::Mint;
use spl_token_metadata_interface::state::TokenMetadata;

use crate::errors::FloorError;
use crate::instructions::mint_aat_nft::MAX_TOTAL_AAT_VOLUME;
use crate::seeds::{AAT_NFT_SEED, CONTRACT_STATE_SEED, INVESTOR_POOL_SEED};
use crate::state::{AatNftAuthority, InvestorPool, ProgramState};

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
    pub investor_pool: Account<'info, InvestorPool>,

    /// CHECK: investor wallet — used only for mint PDA derivation
    pub investor: UncheckedAccount<'info>,

    /// CHECK: Token-2022 mint PDA for the investor's AAT NFT
    #[account(
        mut,
        seeds = [AAT_NFT_SEED, investor.key().as_ref()],
        bump,
    )]
    pub mint: UncheckedAccount<'info>,

    #[account(
        seeds = [b"nft_authority"],
        bump,
    )]
    pub nft_authority: Account<'info, AatNftAuthority>,

    pub token_program: Program<'info, Token2022>,
}

pub fn handler(ctx: Context<UpdateAatVolume>, new_aat_volume: u64) -> Result<()> {

    let old_aat_volume = {
        let data = ctx.accounts.mint.data.borrow();
        let state = StateWithExtensions::<Mint>::unpack(&data)
            .map_err(|_| error!(FloorError::InvalidAatNft))?;
        let metadata = state
            .get_variable_len_extension::<TokenMetadata>()
            .map_err(|_| error!(FloorError::InvalidAatNft))?;
        let mut found: Option<u64> = None;
        for (key, value) in &metadata.additional_metadata {
            if key == "aat_volume" {
                found = Some(
                    value
                        .parse::<u64>()
                        .map_err(|_| error!(FloorError::InvalidAatNft))?,
                );
                break;
            }
        }
        found.ok_or(error!(FloorError::InvalidAatNft))?
    };

    let investor_key = ctx.accounts.investor.key();

    if let Some(record) = ctx
        .accounts
        .investor_pool
        .investors
        .iter()
        .find(|r| r.investor == investor_key)
    {
        require!(
            record.usdc_locked_current_round == 0,
            FloorError::RoundInProgress
        );
    }

    {
        let mut state = ctx.accounts.contract_state.load_mut()?;
        let new_total = state
            .total_aat_volume
            .checked_sub(old_aat_volume)
            .ok_or(FloorError::ArithmeticOverflow)?
            .checked_add(new_aat_volume)
            .ok_or(FloorError::ArithmeticOverflow)?;
        require!(
            new_total <= MAX_TOTAL_AAT_VOLUME,
            FloorError::WalnAllocationLimitExceeded
        );
        state.total_aat_volume = new_total;
    }

    if let Some(record) = ctx
        .accounts
        .investor_pool
        .investors
        .iter_mut()
        .find(|r| r.investor == investor_key)
    {
        record.aat_volume = new_aat_volume;
    }

    let nft_authority_bump = ctx.bumps.nft_authority;
    let nft_signer: &[&[&[u8]]] = &[&[b"nft_authority", &[nft_authority_bump]]];

    invoke_signed(
        &spl_token_metadata_interface::instruction::update_field(
            &spl_token_2022::id(),
            ctx.accounts.mint.key,
            ctx.accounts.nft_authority.to_account_info().key,
            spl_token_metadata_interface::state::Field::Key("aat_volume".to_string()),
            new_aat_volume.to_string(),
        ),
        &[
            ctx.accounts.mint.to_account_info().clone(),
            ctx.accounts.nft_authority.to_account_info().clone(),
        ],
        nft_signer,
    )?;

    Ok(())
}
