use anchor_lang::prelude::*;
use anchor_lang::solana_program::program::invoke;
use anchor_lang::system_program::{create_account, CreateAccount};
use anchor_spl::token_2022;
use anchor_spl::token_2022::Token2022;
use spl_token_2022::{extension::ExtensionType, state::Mint};
use crate::errors::FloorError;
use crate::seeds::CONTRACT_STATE_SEED;
use crate::state::{AatNftAuthority, ProgramState};

#[derive(Accounts)]
pub struct CreateAatMint<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(mut)]
    pub mint: Signer<'info>,

    #[account(
        seeds = [CONTRACT_STATE_SEED],
        bump = contract_state.bump,
        constraint = contract_state.admin == admin.key() @ FloorError::Unauthorized,
    )]
    pub contract_state: Box<Account<'info, ProgramState>>,

    /// CHECK: investor wallet that will receive the NFT
    pub investor: UncheckedAccount<'info>,

    #[account(
        init_if_needed,
        seeds = [b"nft_authority"],
        bump,
        space = 8,
        payer = admin
    )]
    pub nft_authority: Account<'info, AatNftAuthority>,

    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token2022>,
}

pub fn handler(ctx: Context<CreateAatMint>) -> Result<()> {
    let space = match ExtensionType::try_calculate_account_len::<Mint>(&[ExtensionType::MetadataPointer]) {
        Ok(space) => space,
        Err(_) => return err!(FloorError::InvalidMintAccountSpace),
    };

    let meta_data_space = 250usize;
    let lamports_required = Rent::get()?.minimum_balance(space + meta_data_space);

    msg!(
        "Create mint account: {} bytes, {} lamports (pre-funded for metadata)",
        space,
        lamports_required
    );

    create_account(
        CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            CreateAccount {
                from: ctx.accounts.admin.to_account_info(),
                to: ctx.accounts.mint.to_account_info(),
            }
        ),
        lamports_required,
        space as u64,
        &ctx.accounts.token_program.key()
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

    let mint_cpi_ix = CpiContext::new(
        ctx.accounts.token_program.to_account_info(),
        token_2022::InitializeMint2 {
            mint: ctx.accounts.mint.to_account_info(),
        }
    );

    token_2022::initialize_mint2(mint_cpi_ix, 0, &ctx.accounts.nft_authority.key(), None)?;

    Ok(())
}
