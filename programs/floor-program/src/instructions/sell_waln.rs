use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    log::sol_log_data,
    program::{invoke, invoke_signed},
    system_instruction,
};
use anchor_lang::Discriminator;
use anchor_spl::token_2022::spl_token_2022::instruction::transfer_checked as build_transfer_checked_ix;
use anchor_spl::token_interface::{
    transfer_checked, Mint, TokenAccount, TokenInterface, TransferChecked,
};

use crate::errors::FloorError;
use crate::instructions::start_round::execute_round_start;
use crate::seeds::{
    CONTRACT_STATE_SEED, INVESTOR_POOL_SEED, ROUND_LOCKED_WALN_SEED, TREASURY_SEED,
    USDC_VAULT_SEED, WALN_VAULT_SEED,
};
use crate::state::{
    InvestorAlloc, InvestorAllocated, InvestorPool, ProgramState, RoundClosed, RoundLockedWaln,
    MIN_SELL_WALN,
};
use crate::utils::{get_hook_program_id, validate_hook_accounts};

#[derive(Accounts)]
pub struct SellWaln<'info> {
    #[account(mut)]
    pub caller: Signer<'info>,

    pub seller: Signer<'info>,

    #[account(
        mut,
        seeds = [CONTRACT_STATE_SEED],
        bump,
    )]
    pub contract_state: AccountLoader<'info, ProgramState>,

    #[account(
        mut,
        seeds = [INVESTOR_POOL_SEED],
        bump,
    )]
    pub investor_pool: Account<'info, InvestorPool>,

    pub waln_mint: Box<InterfaceAccount<'info, Mint>>,
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
        bump,
        token::mint = waln_mint,
        token::authority = contract_state,
        token::token_program = waln_token_program,
    )]
    pub waln_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        seeds = [USDC_VAULT_SEED],
        bump,
        token::mint = usdc_mint,
        token::authority = contract_state,
        token::token_program = usdc_token_program,
    )]
    pub usdc_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    /// CHECK: Treasury PDA — system-owned, funds round account creation
    #[account(
        mut,
        seeds = [TREASURY_SEED],
        bump,
    )]
    pub treasury: UncheckedAccount<'info>,

    #[account(address = anchor_spl::token_2022::ID)]
    pub waln_token_program: Interface<'info, TokenInterface>,
    pub usdc_token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

pub fn handler<'info>(
    ctx: Context<'_, '_, 'info, 'info, SellWaln<'info>>,
    max_waln_amount: u64,
) -> Result<()> {
    let waln_decimals = ctx.accounts.waln_mint.decimals;
    let usdc_decimals = ctx.accounts.usdc_mint.decimals;

    let round_index;
    let floor_price_usdc;
    let state_bump;
    let current_round_size_waln;
    let round_size_waln_val;
    let floor_price_usdc_val;
    let need_round_start;
    let snapshot_price;
    let snapshot_size;
    let waln_amount;

    {
        let state = ctx.accounts.contract_state.load()?;
        require!(state.frozen == 0, FloorError::ContractFrozen);
        require!(state.sell_paused == 0, FloorError::SellPaused);
        require!(max_waln_amount > 0, FloorError::ZeroAmount);
        require!(
            ctx.accounts.waln_mint.key() == state.waln_mint,
            FloorError::InvalidMint
        );
        require!(
            ctx.accounts.usdc_mint.key() == state.usdc_mint,
            FloorError::InvalidMint
        );

        round_index = state.round_count;
        state_bump = state.bump;
        round_size_waln_val = state.round_size_waln;
        floor_price_usdc_val = state.floor_price_usdc;
        snapshot_price = state.floor_price_usdc;
        snapshot_size = state.round_size_waln;
        need_round_start = state.round_started == 0;

        let (eff_floor_price, eff_round_size, eff_round_waln) = if need_round_start {
            (snapshot_price, snapshot_size, 0u64)
        } else {
            (
                state.current_round_floor_price,
                state.current_round_size_waln,
                state.current_round_waln,
            )
        };

        let remaining_in_round = eff_round_size
            .checked_sub(eff_round_waln)
            .ok_or(FloorError::ArithmeticOverflow)?;
        waln_amount = max_waln_amount.min(remaining_in_round);
        require!(waln_amount > 0, FloorError::ZeroAmount);

        let post_sale_remaining = remaining_in_round
            .checked_sub(waln_amount)
            .ok_or(FloorError::ArithmeticOverflow)?;
        if post_sale_remaining > 0 {
            let local_waln_scale = 10_u128.pow(waln_decimals as u32);
            let post_sale_usdc = (post_sale_remaining as u128)
                .checked_mul(eff_floor_price as u128)
                .ok_or(FloorError::ArithmeticOverflow)?
                .checked_div(local_waln_scale)
                .ok_or(FloorError::ArithmeticOverflow)?;
            require!(post_sale_usdc > 0, FloorError::SellLeavesUnpayableDust);
        }

        if eff_round_waln == 0 {
            let is_completing = waln_amount == remaining_in_round;
            require!(
                waln_amount >= MIN_SELL_WALN || is_completing,
                FloorError::SellAmountTooSmall
            );
        }

        floor_price_usdc = eff_floor_price;
        current_round_size_waln = eff_round_size;
    }

    let usdc_locked_new = if need_round_start {
        let (_, usdc_locked) = execute_round_start(
            &mut ctx.accounts.investor_pool.investors,
            snapshot_size,
            snapshot_price,
            waln_decimals,
        )?;
        usdc_locked
    } else {
        0u64
    };

    let waln_scale = 10_u128.pow(waln_decimals as u32);

    let usdc_out_u128 = (waln_amount as u128)
        .checked_mul(floor_price_usdc as u128)
        .ok_or(FloorError::ArithmeticOverflow)?
        .checked_div(waln_scale)
        .ok_or(FloorError::ArithmeticOverflow)?;
    let usdc_out = u64::try_from(usdc_out_u128).map_err(|_| FloorError::ArithmeticOverflow)?;

    require!(usdc_out > 0, FloorError::ZeroAmount);

    let seller_info = ctx.accounts.seller.to_account_info();
    let treasury_info = ctx.accounts.treasury.to_account_info();
    let treasury_bump = ctx.bumps.treasury;
    let system_program_info = ctx.accounts.system_program.to_account_info();
    let contract_state_info = ctx.accounts.contract_state.to_account_info();

    let hook_offset: usize;
    if let Ok(hook_program_id) = get_hook_program_id(&ctx.accounts.waln_mint.to_account_info()) {
        validate_hook_accounts(
            &ctx.remaining_accounts[..8],
            &ctx.accounts.waln_mint.key(),
            &ctx.accounts.seller.key(),
            &hook_program_id,
        )?;
        hook_offset = 8;
    } else {
        hook_offset = 0;
    }

    let hook_accounts = &ctx.remaining_accounts[..hook_offset];

    let mut ix = build_transfer_checked_ix(
        &ctx.accounts.waln_token_program.key(),
        &ctx.accounts.seller_waln_account.key(),
        &ctx.accounts.waln_mint.key(),
        &ctx.accounts.waln_vault.key(),
        &ctx.accounts.seller.key(),
        &[],
        waln_amount,
        waln_decimals,
    )?;

    for acc in hook_accounts.iter() {
        ix.accounts.push(AccountMeta {
            pubkey: acc.key(),
            is_signer: acc.is_signer,
            is_writable: acc.is_writable,
        });
    }

    let mut account_infos = vec![
        ctx.accounts.seller_waln_account.to_account_info(),
        ctx.accounts.waln_mint.to_account_info(),
        ctx.accounts.waln_vault.to_account_info(),
        seller_info.clone(),
    ];
    for acc in hook_accounts.iter() {
        account_infos.push(acc.clone());
    }

    invoke(&ix, &account_infos)?;

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

    let mut state = ctx.accounts.contract_state.load_mut()?;
    if need_round_start {
        state.current_round_floor_price = snapshot_price;
        state.current_round_size_waln = snapshot_size;
        state.current_round_lock_period = state.lock_period_seconds;
        state.current_round_usdc_spent = 0;
        state.total_usdc_locked_for_round = usdc_locked_new;
        state.round_started = 1;
    }
    state.current_round_usdc_spent = state
        .current_round_usdc_spent
        .checked_add(usdc_out)
        .ok_or(FloorError::ArithmeticOverflow)?;
    state.current_round_waln = state
        .current_round_waln
        .checked_add(waln_amount)
        .ok_or(FloorError::ArithmeticOverflow)?;

    if state.current_round_waln >= current_round_size_waln {
        let remaining = &ctx.remaining_accounts[hook_offset..];
        require!(!remaining.is_empty(), FloorError::InvalidRemainingAccounts);

        let clock = Clock::get()?;
        let lock_period = state.current_round_lock_period;
        let dust_pool = state.waln_dust_carryover;
        let waln_in_round = state.current_round_waln;

        let unlock_timestamp = clock
            .unix_timestamp
            .checked_add(lock_period)
            .ok_or(FloorError::ArithmeticOverflow)?;

        let round_locked_waln_info = &remaining[0];

        let (round_locked_waln_pda, round_locked_waln_bump) = Pubkey::find_program_address(
            &[ROUND_LOCKED_WALN_SEED, &round_index.to_le_bytes()],
            &crate::ID,
        );
        require!(
            round_locked_waln_pda == round_locked_waln_info.key(),
            FloorError::InvalidRemainingAccounts
        );

        let mut total_usdc_spent: u64 = 0;
        let mut total_waln_purchased: u64 = 0;
        let mut total_aat_volume_at_trigger: u64 = 0;

        let mut participant_data: Vec<(Pubkey, u64)> =
            Vec::with_capacity(ctx.accounts.investor_pool.investors.len());
        let mut participant_count_for_dust: u64 = 0;

        {
            let pool = &mut ctx.accounts.investor_pool;

            for record in pool.investors.iter_mut() {
                if record.usdc_locked_current_round == 0 {
                    continue;
                }

                participant_count_for_dust = participant_count_for_dust
                    .checked_add(1)
                    .ok_or(FloorError::ArithmeticOverflow)?;

                let investor = record.investor;
                let alloc = record.aat_volume;
                total_aat_volume_at_trigger = total_aat_volume_at_trigger
                    .checked_add(alloc)
                    .ok_or(FloorError::ArithmeticOverflow)?;

                let usdc_locked = record.usdc_locked_current_round;
                let base_waln = u64::try_from(
                    (usdc_locked as u128)
                        .checked_mul(waln_scale)
                        .ok_or(FloorError::ArithmeticOverflow)?
                        .checked_div(floor_price_usdc as u128)
                        .ok_or(FloorError::ArithmeticOverflow)?,
                )
                .map_err(|_| FloorError::ArithmeticOverflow)?;

                record.usdc_committed = record
                    .usdc_committed
                    .checked_add(usdc_locked)
                    .ok_or(FloorError::ArithmeticOverflow)?;
                record.usdc_locked_current_round = 0;
                record.waln_purchased_total = record
                    .waln_purchased_total
                    .checked_add(base_waln)
                    .ok_or(FloorError::ArithmeticOverflow)?;

                total_usdc_spent = total_usdc_spent
                    .checked_add(usdc_locked)
                    .ok_or(FloorError::ArithmeticOverflow)?;
                total_waln_purchased = total_waln_purchased
                    .checked_add(base_waln)
                    .ok_or(FloorError::ArithmeticOverflow)?;

                participant_data.push((investor, base_waln));
            }
        }

        if dust_pool > 0 && participant_count_for_dust > 0 {
            // Pseudo-random dust recipient (unix_timestamp % N): accepted as designed.
            // dust_pool is the per-round pro-rata rounding remainder
            // (round_size − sum(allocations)) - bounded to a fraction of a wALN
            // because a round can't start without eligible investors and locks sum
            // to the full round cap. A validator can nudge the winner via timestamp,
            // but the prize is negligible.
            let dust_recipient_idx =
                (clock.unix_timestamp as u64).wrapping_rem(participant_count_for_dust) as usize;

            participant_data[dust_recipient_idx].1 = participant_data[dust_recipient_idx]
                .1
                .checked_add(dust_pool)
                .ok_or(FloorError::ArithmeticOverflow)?;
            total_waln_purchased = total_waln_purchased
                .checked_add(dust_pool)
                .ok_or(FloorError::ArithmeticOverflow)?;

            let winner_key = participant_data[dust_recipient_idx].0;
            let pool = &mut ctx.accounts.investor_pool;
            if let Some(record) = pool.investors.iter_mut().find(|r| r.investor == winner_key) {
                record.waln_purchased_total = record
                    .waln_purchased_total
                    .checked_add(dust_pool)
                    .ok_or(FloorError::ArithmeticOverflow)?;
            }
        }

        let participant_count = participant_data.len() as u32;
        participant_data.retain(|(_, waln_amount)| *waln_amount > 0);
        participant_data.sort_unstable_by(|a, b| a.0.to_bytes().cmp(&b.0.to_bytes()));

        if !participant_data.is_empty() {
            let claimable_participant_count = participant_data.len() as u32;
            let round_locked_waln_space = RoundLockedWaln::space(participant_data.len());
            let round_locked_waln_rent = Rent::get()?.minimum_balance(round_locked_waln_space);

            let existing_locked_waln_lamports = round_locked_waln_info.lamports();
            if existing_locked_waln_lamports == 0 {
                invoke_signed(
                    &system_instruction::create_account(
                        treasury_info.key,
                        round_locked_waln_info.key,
                        round_locked_waln_rent,
                        round_locked_waln_space as u64,
                        &crate::ID,
                    ),
                    &[
                        treasury_info.clone(),
                        round_locked_waln_info.clone(),
                        system_program_info.clone(),
                    ],
                    &[
                        &[TREASURY_SEED, &[treasury_bump]],
                        &[
                            ROUND_LOCKED_WALN_SEED,
                            &round_index.to_le_bytes(),
                            &[round_locked_waln_bump],
                        ],
                    ],
                )?;
            } else {
                if existing_locked_waln_lamports < round_locked_waln_rent {
                    invoke_signed(
                        &system_instruction::transfer(
                            treasury_info.key,
                            round_locked_waln_info.key,
                            round_locked_waln_rent - existing_locked_waln_lamports,
                        ),
                        &[
                            treasury_info.clone(),
                            round_locked_waln_info.clone(),
                            system_program_info.clone(),
                        ],
                        &[&[TREASURY_SEED, &[treasury_bump]]],
                    )?;
                }
                invoke_signed(
                    &system_instruction::allocate(
                        round_locked_waln_info.key,
                        round_locked_waln_space as u64,
                    ),
                    &[round_locked_waln_info.clone(), system_program_info.clone()],
                    &[&[
                        ROUND_LOCKED_WALN_SEED,
                        &round_index.to_le_bytes(),
                        &[round_locked_waln_bump],
                    ]],
                )?;
                invoke_signed(
                    &system_instruction::assign(round_locked_waln_info.key, &crate::ID),
                    &[round_locked_waln_info.clone(), system_program_info.clone()],
                    &[&[
                        ROUND_LOCKED_WALN_SEED,
                        &round_index.to_le_bytes(),
                        &[round_locked_waln_bump],
                    ]],
                )?;
            }

            {
                let entries: Vec<InvestorAlloc> = participant_data
                    .iter()
                    .map(|(investor, waln_amount)| InvestorAlloc {
                        investor: *investor,
                        waln_amount: *waln_amount,
                    })
                    .collect();
                let rlw = RoundLockedWaln {
                    round_index,
                    bump: round_locked_waln_bump,
                    unlock: unlock_timestamp,
                    remaining_to_claim: claimable_participant_count,
                    investors: entries,
                };
                let mut data = round_locked_waln_info.try_borrow_mut_data()?;
                let mut writer: &mut [u8] = &mut data;
                rlw.try_serialize(&mut writer)?;
            }
        }

        const ALLOC_LOG_LEN: usize = 8 + 8 + 32 + 8 + 8 + 8;
        let alloc_disc = InvestorAllocated::DISCRIMINATOR;
        let round_index_le = round_index.to_le_bytes();
        let unlock_le = unlock_timestamp.to_le_bytes();

        for (investor, waln_amount) in participant_data.iter() {
            let usdc_spent_for = u64::try_from(
                (*waln_amount as u128)
                    .checked_mul(floor_price_usdc as u128)
                    .ok_or(FloorError::ArithmeticOverflow)?
                    .checked_div(waln_scale)
                    .ok_or(FloorError::ArithmeticOverflow)?,
            )
            .map_err(|_| FloorError::ArithmeticOverflow)?;

            let mut buf = [0u8; ALLOC_LOG_LEN];
            buf[0..8].copy_from_slice(alloc_disc);
            buf[8..16].copy_from_slice(&round_index_le);
            buf[16..48].copy_from_slice(investor.as_ref());
            buf[48..56].copy_from_slice(&waln_amount.to_le_bytes());
            buf[56..64].copy_from_slice(&usdc_spent_for.to_le_bytes());
            buf[64..72].copy_from_slice(&unlock_le);

            sol_log_data(&[&buf]);
        }

        emit!(RoundClosed {
            round_index,
            waln_purchased: total_waln_purchased,
            usdc_spent: total_usdc_spent,
            total_aat_volume_at_trigger,
            participant_count,
            unlock: unlock_timestamp,
        });

        state.round_count = state
            .round_count
            .checked_add(1)
            .ok_or(FloorError::ArithmeticOverflow)?;
        state.current_round_waln = 0;
        state.current_round_usdc_spent = 0;
        state.total_usdc_in_lobby = state
            .total_usdc_in_lobby
            .checked_sub(total_usdc_spent)
            .ok_or(FloorError::ArithmeticOverflow)?;
        state.round_started = 0;
        state.waln_dust_carryover = u64::try_from(
            (waln_in_round as u128)
                .checked_add(dust_pool as u128)
                .ok_or(FloorError::ArithmeticOverflow)?
                .checked_sub(total_waln_purchased as u128)
                .ok_or(FloorError::ArithmeticOverflow)?,
        )
        .map_err(|_| FloorError::ArithmeticOverflow)?;

        {
            match execute_round_start(
                &mut ctx.accounts.investor_pool.investors,
                round_size_waln_val,
                floor_price_usdc_val,
                waln_decimals,
            ) {
                Ok((_aat_vol, usdc_locked)) => {
                    state.current_round_floor_price = floor_price_usdc_val;
                    state.current_round_size_waln = round_size_waln_val;
                    state.current_round_lock_period = state.lock_period_seconds;
                    state.current_round_usdc_spent = 0;
                    state.total_usdc_locked_for_round = usdc_locked;
                    state.round_started = 1;
                }
                Err(ref e) if e == &FloorError::NoEligibleInvestors.into() => {
                    msg!("Auto-start skipped: no eligible investors");
                    state.total_usdc_locked_for_round = 0;
                }
                Err(ref e) if e == &FloorError::InvalidParameter.into() => {
                    msg!("Auto-start skipped: round cap would be zero (round_size * floor_price < 10^decimals)");
                    state.total_usdc_locked_for_round = 0;
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }
    }

    Ok(())
}
