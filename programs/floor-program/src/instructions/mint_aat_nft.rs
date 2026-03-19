use anchor_lang::prelude::*;
use anchor_lang::solana_program::program::invoke;
use anchor_lang::solana_program::program::invoke_signed;
use anchor_lang::solana_program::system_instruction;
use anchor_spl::associated_token::{self, AssociatedToken};
use anchor_spl::token_2022;
use anchor_spl::token_2022::Token2022;
use spl_token_2022::{extension::ExtensionType, state::Mint};
use spl_token_2022::instruction::AuthorityType;
use crate::errors::FloorError;
use crate::seeds::{AAT_NFT_SEED, CONTRACT_STATE_SEED};
use crate::state::{AatNftAuthority, ProgramState};

#[derive(Accounts)]
pub struct MintAatNft<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(
        mut,
        seeds = [AAT_NFT_SEED, investor.key().as_ref()],
        bump,
    )]
    /// CHECK: Initialized as Token-2022 mint with MetadataPointer in this instruction
    pub mint: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [CONTRACT_STATE_SEED],
        bump = contract_state.bump,
        constraint = contract_state.admin == admin.key() @ FloorError::Unauthorized,
    )]
    pub contract_state: Box<Account<'info, ProgramState>>,

    /// CHECK: investor wallet that will receive the NFT
    pub investor: UncheckedAccount<'info>,

    /// CHECK: investor's ATA for the NFT mint — created inside this instruction
    #[account(mut)]
    pub investor_aat_account: UncheckedAccount<'info>,

    #[account(
        init_if_needed,
        seeds = [b"nft_authority"],
        bump,
        space = 8,
        payer = admin,
    )]
    pub nft_authority: Account<'info, AatNftAuthority>,

    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token2022>,
    pub associated_token_program: Program<'info, AssociatedToken>,
}

pub const MAX_TOTAL_AAT_VOLUME: u64 = 1_000_000;

pub fn handler(ctx: Context<MintAatNft>, aat_volume: u64) -> Result<()> {
    let new_total = ctx.accounts.contract_state.total_aat_volume
        .checked_add(aat_volume)
        .ok_or(FloorError::ArithmeticOverflow)?;
    require!(
        new_total <= MAX_TOTAL_AAT_VOLUME,
        FloorError::WalnAllocationLimitExceeded
    );

    let space = match ExtensionType::try_calculate_account_len::<Mint>(&[
        ExtensionType::MetadataPointer,
        ExtensionType::NonTransferable,
    ]) {
        Ok(space) => space,
        Err(_) => return err!(FloorError::InvalidMintAccountSpace),
    };

    let meta_data_space = 512usize;
    let lamports_required = Rent::get()?.minimum_balance(space + meta_data_space);

    let investor_key = ctx.accounts.investor.key();
    let mint_bump = ctx.bumps.mint;
    let mint_signer_seeds: &[&[u8]] = &[AAT_NFT_SEED, investor_key.as_ref(), &[mint_bump]];
    let mint_signer: &[&[&[u8]]] = &[mint_signer_seeds];

    invoke_signed(
        &system_instruction::create_account(
            &ctx.accounts.admin.key(),
            &ctx.accounts.mint.key(),
            lamports_required,
            space as u64,
            &ctx.accounts.token_program.key(),
        ),
        &[
            ctx.accounts.admin.to_account_info(),
            ctx.accounts.mint.to_account_info(),
            ctx.accounts.system_program.to_account_info(),
        ],
        mint_signer,
    )?;

    let init_meta_data_pointer_ix = match spl_token_2022::extension::metadata_pointer::instruction::initialize(
        &Token2022::id(),
        &ctx.accounts.mint.key(),
        Some(ctx.accounts.nft_authority.key()),
        Some(ctx.accounts.mint.key())
    ) {
        Ok(ix) => ix,
        Err(_) => return err!(FloorError::InvalidMintAccountSpace),
    };

    invoke(
        &init_meta_data_pointer_ix,
        &[ctx.accounts.mint.to_account_info(), ctx.accounts.nft_authority.to_account_info()]
    )?;

    invoke(
        &spl_token_2022::instruction::initialize_non_transferable_mint(
            &Token2022::id(),
            &ctx.accounts.mint.key(),
        )
        .map_err(|_| error!(FloorError::InvalidMintAccountSpace))?,
        &[ctx.accounts.mint.to_account_info()],
    )?;

    token_2022::initialize_mint2(
        CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            token_2022::InitializeMint2 {
                mint: ctx.accounts.mint.to_account_info(),
            }
        ),
        0,
        &ctx.accounts.nft_authority.key(),
        None
    )?;

    let nft_authority_bump = ctx.bumps.nft_authority;
    let nft_signer: &[&[&[u8]]] = &[&[b"nft_authority", &[nft_authority_bump]]];

    msg!("Init metadata {}", ctx.accounts.mint.key());

    invoke_signed(
        &spl_token_metadata_interface::instruction::initialize(
            &spl_token_2022::id(),
            ctx.accounts.mint.key,
            ctx.accounts.nft_authority.to_account_info().key,
            ctx.accounts.mint.key,
            ctx.accounts.nft_authority.to_account_info().key,
            "AAT NFT".to_string(),
            "AAT".to_string(),
            "".to_string()
        ),
        &[
            ctx.accounts.mint.to_account_info().clone(),
            ctx.accounts.nft_authority.to_account_info().clone(),
        ],
        nft_signer
    )?;

    invoke_signed(
        &spl_token_metadata_interface::instruction::update_field(
            &spl_token_2022::id(),
            ctx.accounts.mint.key,
            ctx.accounts.nft_authority.to_account_info().key,
            spl_token_metadata_interface::state::Field::Key("aat_volume".to_string()),
            aat_volume.to_string()
        ),
        &[
            ctx.accounts.mint.to_account_info().clone(),
            ctx.accounts.nft_authority.to_account_info().clone(),
        ],
        nft_signer
    )?;

    associated_token::create(
        CpiContext::new(
            ctx.accounts.associated_token_program.to_account_info(),
            associated_token::Create {
                payer: ctx.accounts.admin.to_account_info(),
                associated_token: ctx.accounts.investor_aat_account.to_account_info(),
                authority: ctx.accounts.investor.to_account_info(),
                mint: ctx.accounts.mint.to_account_info(),
                system_program: ctx.accounts.system_program.to_account_info(),
                token_program: ctx.accounts.token_program.to_account_info(),
            }
        )
    )?;

    token_2022::mint_to(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            token_2022::MintTo {
                mint: ctx.accounts.mint.to_account_info(),
                to: ctx.accounts.investor_aat_account.to_account_info(),
                authority: ctx.accounts.nft_authority.to_account_info(),
            },
            nft_signer
        ),
        1
    )?;

    token_2022::set_authority(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            token_2022::SetAuthority {
                current_authority: ctx.accounts.nft_authority.to_account_info(),
                account_or_mint: ctx.accounts.mint.to_account_info(),
            },
            nft_signer
        ),
        AuthorityType::MintTokens,
        None
    )?;

    ctx.accounts.contract_state.total_aat_volume = new_total;

    Ok(())
}
