use anchor_lang::prelude::*;
use anchor_lang::solana_program::{program::invoke_signed, system_instruction};
use anchor_spl::token_interface::{
    transfer_checked, Mint, TokenAccount, TokenInterface, TransferChecked,
};

use crate::errors::FloorError;
use crate::instructions::start_round::execute_round_start;
use crate::nft_utils::verify_aat_nft_and_get_allocation;
use crate::seeds::{
    CONTRACT_STATE_SEED, LOBBY_ENTRY_SEED, LOCKED_WALN_SEED, ROUND_RECORD_SEED, USDC_VAULT_SEED,
    WALN_VAULT_SEED,
};
use crate::state::{LobbyEntry, LockedWaln, ProgramState, RoundRecord};

#[derive(Accounts)]
pub struct SellWaln<'info> {
    #[account(mut)]
    pub seller: Signer<'info>,

    #[account(
        mut,
        seeds = [CONTRACT_STATE_SEED],
        bump = contract_state.bump,
    )]
    pub contract_state: Account<'info, ProgramState>,

    #[account(constraint = waln_mint.key() == contract_state.waln_mint @ FloorError::InvalidMint)]
    pub waln_mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(constraint = usdc_mint.key() == contract_state.usdc_mint @ FloorError::InvalidMint)]
    pub usdc_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        mut,
        token::mint = waln_mint,
        token::authority = seller,
        token::token_program = waln_token_program,
    )]
    pub seller_waln_account: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        token::mint = usdc_mint,
        token::authority = seller,
        token::token_program = usdc_token_program,
    )]
    pub seller_usdc_account: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        seeds = [WALN_VAULT_SEED],
        bump = contract_state.waln_vault_bump,
        token::mint = waln_mint,
        token::authority = contract_state,
        token::token_program = waln_token_program,
    )]
    pub waln_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        seeds = [USDC_VAULT_SEED],
        bump = contract_state.usdc_vault_bump,
        token::mint = usdc_mint,
        token::authority = contract_state,
        token::token_program = usdc_token_program,
    )]
    pub usdc_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    pub waln_token_program: Interface<'info, TokenInterface>,
    pub usdc_token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

pub fn handler<'info>(
    ctx: Context<'_, '_, 'info, 'info, SellWaln<'info>>,
    waln_amount: u64,
) -> Result<()> {
    {
        let state = &ctx.accounts.contract_state;
        require!(!state.paused, FloorError::ContractPaused);
        require!(waln_amount > 0, FloorError::ZeroAmount);
    }

    let waln_decimals = ctx.accounts.waln_mint.decimals;

    // Round Start (lazy) — executes when round_started = false.
    if !ctx.accounts.contract_state.round_started {
        let remaining = ctx.remaining_accounts;
        require!(
            remaining.len() >= 4 && (remaining.len() - 1) % 3 == 0,
            FloorError::InvalidRemainingAccounts
        );

        let round_index = ctx.accounts.contract_state.round_count;
        let (expected_rr_pda, _) = Pubkey::find_program_address(
            &[ROUND_RECORD_SEED, &round_index.to_le_bytes()],
            &crate::ID,
        );
        require!(
            expected_rr_pda == remaining[0].key(),
            FloorError::InvalidRemainingAccounts
        );

        let investor_triplets = &remaining[1..];
        let snapshot_price = ctx.accounts.contract_state.floor_price_usdc;
        let snapshot_size = ctx.accounts.contract_state.round_size_waln;
        execute_round_start(
            investor_triplets,
            snapshot_size,
            snapshot_price,
            waln_decimals,
        )?;
        ctx.accounts.contract_state.current_round_floor_price = snapshot_price;
        ctx.accounts.contract_state.current_round_size_waln = snapshot_size;
        ctx.accounts.contract_state.round_started = true;
    }

    {
        let state = &ctx.accounts.contract_state;
        let remaining_in_round = state
            .current_round_size_waln
            .checked_sub(state.current_round_waln)
            .ok_or(FloorError::ArithmeticOverflow)?;
        require!(
            waln_amount <= remaining_in_round,
            FloorError::SellAmountExceedsRound
        );
    }

    let floor_price_usdc = ctx.accounts.contract_state.current_round_floor_price;
    let usdc_decimals = ctx.accounts.usdc_mint.decimals;
    let state_bump = ctx.accounts.contract_state.bump;
    let waln_scale = 10_u128.pow(waln_decimals as u32);

    let usdc_out_u128 = (waln_amount as u128)
        .checked_mul(floor_price_usdc as u128)
        .ok_or(FloorError::ArithmeticOverflow)?
        .checked_div(waln_scale)
        .ok_or(FloorError::ArithmeticOverflow)?;
    let usdc_out = u64::try_from(usdc_out_u128).map_err(|_| FloorError::ArithmeticOverflow)?;

    let seller_info = ctx.accounts.seller.to_account_info();
    let system_program_info = ctx.accounts.system_program.to_account_info();
    let contract_state_info = ctx.accounts.contract_state.to_account_info();

    transfer_checked(
        CpiContext::new(
            ctx.accounts.waln_token_program.to_account_info(),
            TransferChecked {
                from: ctx.accounts.seller_waln_account.to_account_info(),
                mint: ctx.accounts.waln_mint.to_account_info(),
                to: ctx.accounts.waln_vault.to_account_info(),
                authority: seller_info.clone(),
            },
        ),
        waln_amount,
        waln_decimals,
    )?;

    let seeds: &[&[u8]] = &[CONTRACT_STATE_SEED, &[state_bump]];
    let signer = &[seeds];

    transfer_checked(
        CpiContext::new_with_signer(
            ctx.accounts.usdc_token_program.to_account_info(),
            TransferChecked {
                from: ctx.accounts.usdc_vault.to_account_info(),
                mint: ctx.accounts.usdc_mint.to_account_info(),
                to: ctx.accounts.seller_usdc_account.to_account_info(),
                authority: contract_state_info.clone(),
            },
            signer,
        ),
        usdc_out,
        usdc_decimals,
    )?;

    let state = &mut ctx.accounts.contract_state;
    state.current_round_waln = state
        .current_round_waln
        .checked_add(waln_amount)
        .ok_or(FloorError::ArithmeticOverflow)?;

    // Round End — executes when round is complete.
    if state.current_round_waln >= state.current_round_size_waln {
        let remaining = ctx.remaining_accounts;
        require!(
            remaining.len() >= 4 && (remaining.len() - 1) % 3 == 0,
            FloorError::InvalidRemainingAccounts
        );

        let clock = Clock::get()?;
        let round_index = state.round_count;
        let lock_period = state.lock_period_seconds;
        let floor_price = state.current_round_floor_price;
        let round_size_waln_val = state.round_size_waln;
        let floor_price_usdc_val = state.floor_price_usdc;

        let unlock_timestamp = clock
            .unix_timestamp
            .checked_add(lock_period)
            .ok_or(FloorError::ArithmeticOverflow)?;

        let round_record_info = &remaining[0];
        let investor_triplets = &remaining[1..];

        let (round_record_pda, round_record_bump) = Pubkey::find_program_address(
            &[ROUND_RECORD_SEED, &round_index.to_le_bytes()],
            &crate::ID,
        );
        require!(
            round_record_pda == round_record_info.key(),
            FloorError::InvalidRemainingAccounts
        );

        let mut total_usdc_spent: u64 = 0;
        let mut total_waln_purchased: u64 = 0;
        let mut participant_count: u32 = 0;
        let mut total_aat_volume_at_trigger: u64 = 0;

        for chunk in investor_triplets.chunks(3) {
            if chunk.len() < 3 {
                break;
            }
            let lobby_entry_info = &chunk[0];
            let locked_waln_info = &chunk[1];
            let core_asset_info = &chunk[2];

            let mut lobby_entry: Account<LobbyEntry> =
                match Account::try_from(lobby_entry_info) {
                    Ok(e) => e,
                    Err(_) => continue,
                };

            let expected_lobby_pda = Pubkey::create_program_address(
                &[LOBBY_ENTRY_SEED, lobby_entry.investor.as_ref(), &[lobby_entry.bump]],
                &crate::ID,
            )
            .map_err(|_| error!(FloorError::InvalidRemainingAccounts))?;
            require!(
                expected_lobby_pda == lobby_entry_info.key(),
                FloorError::InvalidRemainingAccounts
            );

            if lobby_entry.usdc_locked_current_round == 0 {
                continue;
            }

            let investor = lobby_entry.investor;

            // Verify the Core Asset still belongs to the investor and get allocation weight.
            let alloc = match verify_aat_nft_and_get_allocation(
                core_asset_info,
                &investor,
            ) {
                Ok(a) => a,
                Err(_) => 0,
            };
            total_aat_volume_at_trigger = total_aat_volume_at_trigger
                .checked_add(alloc)
                .ok_or(FloorError::ArithmeticOverflow)?;

            let usdc_locked = lobby_entry.usdc_locked_current_round;
            let waln_alloc = u64::try_from(
                (usdc_locked as u128)
                    .checked_mul(waln_scale)
                    .ok_or(FloorError::ArithmeticOverflow)?
                    .checked_div(floor_price as u128)
                    .ok_or(FloorError::ArithmeticOverflow)?,
            )
            .map_err(|_| FloorError::ArithmeticOverflow)?;

            lobby_entry.usdc_committed = lobby_entry
                .usdc_committed
                .checked_add(usdc_locked)
                .ok_or(FloorError::ArithmeticOverflow)?;
            lobby_entry.usdc_locked_current_round = 0;
            lobby_entry.waln_purchased_total = lobby_entry
                .waln_purchased_total
                .checked_add(waln_alloc)
                .ok_or(FloorError::ArithmeticOverflow)?;

            let (locked_waln_pda, locked_waln_bump) = Pubkey::find_program_address(
                &[
                    LOCKED_WALN_SEED,
                    investor.as_ref(),
                    &round_index.to_le_bytes(),
                ],
                &crate::ID,
            );
            require!(
                locked_waln_pda == locked_waln_info.key(),
                FloorError::InvalidRemainingAccounts
            );

            let space = 8 + LockedWaln::INIT_SPACE;
            let rent = Rent::get()?.minimum_balance(space);

            invoke_signed(
                &system_instruction::create_account(
                    seller_info.key,
                    locked_waln_info.key,
                    rent,
                    space as u64,
                    &crate::ID,
                ),
                &[seller_info.clone(), locked_waln_info.clone(), system_program_info.clone()],
                &[&[
                    LOCKED_WALN_SEED,
                    investor.as_ref(),
                    &round_index.to_le_bytes(),
                    &[locked_waln_bump],
                ]],
            )?;

            {
                let mut data = locked_waln_info.try_borrow_mut_data()?;
                data[..8].copy_from_slice(&LockedWaln::DISCRIMINATOR);
                let record = LockedWaln {
                    investor,
                    round_index,
                    waln_amount: waln_alloc,
                    unlock: unlock_timestamp,
                    claimed: false,
                    bump: locked_waln_bump,
                };
                use anchor_lang::AnchorSerialize;
                record.serialize(&mut &mut data[8..])?;
            }

            total_usdc_spent = total_usdc_spent
                .checked_add(usdc_locked)
                .ok_or(FloorError::ArithmeticOverflow)?;
            total_waln_purchased = total_waln_purchased
                .checked_add(waln_alloc)
                .ok_or(FloorError::ArithmeticOverflow)?;
            participant_count = participant_count
                .checked_add(1)
                .ok_or(FloorError::ArithmeticOverflow)?;

            lobby_entry.exit(&crate::ID)?;
        }

        let round_record_space = 8 + RoundRecord::INIT_SPACE;
        let round_record_rent = Rent::get()?.minimum_balance(round_record_space);

        invoke_signed(
            &system_instruction::create_account(
                seller_info.key,
                round_record_info.key,
                round_record_rent,
                round_record_space as u64,
                &crate::ID,
            ),
            &[seller_info.clone(), round_record_info.clone(), system_program_info.clone()],
            &[&[
                ROUND_RECORD_SEED,
                &round_index.to_le_bytes(),
                &[round_record_bump],
            ]],
        )?;

        {
            let mut rr_data = round_record_info.try_borrow_mut_data()?;
            rr_data[..8].copy_from_slice(&RoundRecord::DISCRIMINATOR);
            let record = RoundRecord {
                round_index,
                triggered_at: clock.unix_timestamp,
                waln_purchased: total_waln_purchased,
                usdc_spent: total_usdc_spent,
                total_aat_volume_at_trigger,
                participant_count,
                bump: round_record_bump,
            };
            use anchor_lang::AnchorSerialize;
            record.serialize(&mut &mut rr_data[8..])?;
        }

        let state = &mut ctx.accounts.contract_state;
        state.round_count = state
            .round_count
            .checked_add(1)
            .ok_or(FloorError::ArithmeticOverflow)?;
        state.current_round_waln = 0;
        state.total_usdc_in_lobby = state
            .total_usdc_in_lobby
            .checked_sub(total_usdc_spent)
            .ok_or(FloorError::ArithmeticOverflow)?;
        state.round_started = false;

        // Immediately start next round if eligible investors remain.
        // Snapshot the current (possibly updated) floor_price_usdc and round_size_waln.
        if execute_round_start(
            investor_triplets,
            round_size_waln_val,
            floor_price_usdc_val,
            waln_decimals,
        )
        .is_ok()
        {
            state.current_round_floor_price = floor_price_usdc_val;
            state.current_round_size_waln = round_size_waln_val;
            state.round_started = true;
        }
    }

    Ok(())
}
