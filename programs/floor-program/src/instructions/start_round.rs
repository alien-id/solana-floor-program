use anchor_lang::prelude::*;
use anchor_lang::solana_program::{program::invoke_signed, system_instruction};

use crate::errors::FloorError;
use crate::seeds::{
    CONTRACT_STATE_SEED, INVESTOR_POOL_SEED, ROUND_LOCKED_WALN_SEED, TREASURY_SEED,
};
use crate::state::{InvestorAlloc, InvestorPool, ProgramState, RoundLockedWaln};

#[derive(Accounts)]
#[instruction(round_index: u64)]
pub struct StartRound<'info> {
    #[account(mut)]
    pub caller: Signer<'info>,

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

    /// CHECK: Created here via system_instruction::create_account with PDA seeds
    #[account(
        mut,
        seeds = [ROUND_LOCKED_WALN_SEED, &round_index.to_le_bytes()],
        bump,
    )]
    pub round_locked_waln: UncheckedAccount<'info>,

    /// CHECK: Treasury PDA — system-owned, funds account creation
    #[account(
        mut,
        seeds = [TREASURY_SEED],
        bump,
    )]
    pub treasury: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<StartRound>, round_index: u64) -> Result<()> {
    let round_size_waln;
    let floor_price_usdc;
    let waln_decimals;
    {
        let state = ctx.accounts.contract_state.load()?;
        require!(state.paused == 0, FloorError::ContractPaused);
        require!(state.round_started == 0, FloorError::InvalidParameter);
        require!(state.round_count == round_index, FloorError::InvalidParameter);
        round_size_waln = state.round_size_waln;
        floor_price_usdc = state.floor_price_usdc;
        waln_decimals = state.waln_decimals;
    }

    let waln_scale = 10_u128.pow(waln_decimals as u32);
    let round_cap_usdc_u128 = (round_size_waln as u128)
        .checked_mul(floor_price_usdc as u128)
        .ok_or(FloorError::ArithmeticOverflow)?
        .checked_div(waln_scale)
        .ok_or(FloorError::ArithmeticOverflow)?;
    require!(round_cap_usdc_u128 > 0, FloorError::InvalidParameter);

    let min_deposit = round_cap_usdc_u128 / 2;

    let pool = &mut ctx.accounts.investor_pool;
    let n = pool.investors.len();
    let mut eligible = vec![false; n];
    for (i, r) in pool.investors.iter().enumerate() {
        eligible[i] = r.usdc_deposited > 0
            && r.aat_volume > 0
            && (r.usdc_deposited as u128) >= min_deposit;
    }

    let mut total_aat_volume: u64;
    loop {
        total_aat_volume = 0;
        for (i, r) in pool.investors.iter().enumerate() {
            if eligible[i] {
                total_aat_volume = total_aat_volume
                    .checked_add(r.aat_volume)
                    .ok_or(FloorError::ArithmeticOverflow)?;
            }
        }
        require!(total_aat_volume > 0, FloorError::NoEligibleInvestors);

        let mut changed = false;
        for (i, r) in pool.investors.iter().enumerate() {
            if !eligible[i] {
                continue;
            }
            let required = round_cap_usdc_u128
                .checked_mul(r.aat_volume as u128)
                .ok_or(FloorError::ArithmeticOverflow)?
                .checked_div(total_aat_volume as u128)
                .ok_or(FloorError::ArithmeticOverflow)?;
            if (r.usdc_deposited as u128) < required {
                eligible[i] = false;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    let mut entries: Vec<InvestorAlloc> = Vec::new();
    let mut total_usdc_locked: u64 = 0;
    for (i, r) in pool.investors.iter_mut().enumerate() {
        if !eligible[i] {
            continue;
        }
        let usdc_locked_u128 = round_cap_usdc_u128
            .checked_mul(r.aat_volume as u128)
            .ok_or(FloorError::ArithmeticOverflow)?
            .checked_div(total_aat_volume as u128)
            .ok_or(FloorError::ArithmeticOverflow)?;
        let usdc_locked = usdc_locked_u128.min(r.usdc_deposited as u128) as u64;
        if usdc_locked == 0 {
            continue;
        }

        let base_waln = u64::try_from(
            (usdc_locked as u128)
                .checked_mul(waln_scale)
                .ok_or(FloorError::ArithmeticOverflow)?
                .checked_div(floor_price_usdc as u128)
                .ok_or(FloorError::ArithmeticOverflow)?,
        )
        .map_err(|_| FloorError::ArithmeticOverflow)?;

        r.usdc_deposited = r
            .usdc_deposited
            .checked_sub(usdc_locked)
            .ok_or(FloorError::ArithmeticOverflow)?;
        r.usdc_locked_current_round = usdc_locked;

        entries.push(InvestorAlloc {
            investor: r.investor,
            waln_amount: base_waln,
        });
        total_usdc_locked = total_usdc_locked
            .checked_add(usdc_locked)
            .ok_or(FloorError::ArithmeticOverflow)?;
    }
    require!(!entries.is_empty(), FloorError::NoEligibleInvestors);

    entries.sort_unstable_by(|a, b| a.investor.to_bytes().cmp(&b.investor.to_bytes()));

    let participant_count = entries.len() as u32;
    let space = RoundLockedWaln::space(entries.len());
    let rent = Rent::get()?.minimum_balance(space);

    let treasury_bump = ctx.bumps.treasury;
    let rlw_bump = ctx.bumps.round_locked_waln;
    let rlw_info = ctx.accounts.round_locked_waln.to_account_info();
    let treasury_info = ctx.accounts.treasury.to_account_info();
    let sys_info = ctx.accounts.system_program.to_account_info();

    require!(rlw_info.data_is_empty(), FloorError::InvalidParameter);

    invoke_signed(
        &system_instruction::create_account(
            treasury_info.key,
            rlw_info.key,
            rent,
            space as u64,
            &crate::ID,
        ),
        &[treasury_info.clone(), rlw_info.clone(), sys_info.clone()],
        &[
            &[TREASURY_SEED, &[treasury_bump]],
            &[
                ROUND_LOCKED_WALN_SEED,
                &round_index.to_le_bytes(),
                &[rlw_bump],
            ],
        ],
    )?;

    let rlw = RoundLockedWaln {
        round_index,
        bump: rlw_bump,
        unlock: 0,
        remaining_to_claim: participant_count,
        finalized: false,
        investors: entries,
    };
    {
        let mut data = rlw_info.try_borrow_mut_data()?;
        let mut writer: &mut [u8] = &mut data;
        rlw.try_serialize(&mut writer)?;
    }

    let mut state = ctx.accounts.contract_state.load_mut()?;
    state.current_round_floor_price = floor_price_usdc;
    state.current_round_size_waln = round_size_waln;
    state.total_usdc_locked_for_round = total_usdc_locked;
    state.round_started = 1;

    Ok(())
}
