use crate::utils::{
    get_aat_nft_mint_address, get_contract_state_address, get_investor_pool_address,
    get_nft_authority_address, get_treasury_address, get_usdc_vault_address,
    get_waln_vault_address,
};
use anchor_client::{
    solana_sdk::pubkey::Pubkey,
    Program,
};
use anchor_lang::solana_program::system_program;
use anyhow::{anyhow, Result};
use floor_program::state::{InvestorPool, ProgramState};
use std::sync::Arc;
use anchor_client::solana_sdk::signature::Keypair;

fn load_zero_copy<T: bytemuck::Pod>(
    rpc: &solana_rpc_client::rpc_client::RpcClient,
    pubkey: &Pubkey,
) -> Result<T> {
    let account = rpc.get_account(pubkey)?;
    let min_len = 8 + std::mem::size_of::<T>();
    if account.data.len() < min_len {
        return Err(anyhow!(
            "Account data too small: {} < {}",
            account.data.len(),
            min_len
        ));
    }
    let data = &account.data[8..8 + std::mem::size_of::<T>()];
    let value = bytemuck::try_from_bytes::<T>(data)
        .map_err(|e| anyhow!("Failed to deserialize account: {:?}", e))?;
    Ok(*value)
}

pub fn handle_info(program: &Program<Arc<Keypair>>) -> Result<()> {
    let program_id = program.id();
    let (contract_state_pubkey, _) = get_contract_state_address(&program_id);
    let (usdc_vault_pubkey, _) = get_usdc_vault_address(&program_id);
    let (waln_vault_pubkey, _) = get_waln_vault_address(&program_id);
    let (investor_pool_pubkey, _) = get_investor_pool_address(&program_id);
    let (treasury_pubkey, _) = get_treasury_address(&program_id);

    let rpc = program.rpc();
    let state = load_zero_copy::<ProgramState>(&rpc, &contract_state_pubkey)?;
    let pool = load_zero_copy::<InvestorPool>(&rpc, &investor_pool_pubkey)?;

    let usdc_scale = 10_f64.powi(state.usdc_decimals as i32);
    let waln_scale = 10_f64.powi(state.waln_decimals as i32);

    let usdc_vault_balance = rpc
        .get_token_account_balance(&usdc_vault_pubkey)
        .map(|b| b.amount.parse::<u64>().unwrap_or(0))
        .unwrap_or(0);
    let waln_vault_balance = rpc
        .get_token_account_balance(&waln_vault_pubkey)
        .map(|b| b.amount.parse::<u64>().unwrap_or(0))
        .unwrap_or(0);
    let treasury_lamports = rpc
        .get_account(&treasury_pubkey)
        .map(|a| a.lamports)
        .unwrap_or(0);

    println!("\n=== Floor Program State ===\n");
    println!("Program ID:     {}", program_id);
    println!("Contract State: {} (must be whitelisted in transfer hook)", contract_state_pubkey);
    println!("Admin:          {}", state.admin);
    if state.pending_admin != Pubkey::default() {
        println!("Pending Admin:  {} (transfer in progress)", state.pending_admin);
    }
    println!("USDC Mint:      {}", state.usdc_mint);
    println!("WALN Mint:      {}", state.waln_mint);

    println!("\n--- Vaults ---");
    println!(
        "USDC Vault:     {} | {} ({:.6} USDC)",
        usdc_vault_pubkey,
        usdc_vault_balance,
        usdc_vault_balance as f64 / usdc_scale
    );
    println!(
        "WALN Vault:     {} | {} ({:.6} WALN)",
        waln_vault_pubkey,
        waln_vault_balance,
        waln_vault_balance as f64 / waln_scale
    );
    println!(
        "Treasury:       {} | {:.9} SOL",
        treasury_pubkey,
        treasury_lamports as f64 / 1e9
    );

    println!("\n--- Config ---");
    println!(
        "Floor Price:    {} ({:.6} USDC)",
        state.floor_price_usdc,
        state.floor_price_usdc as f64 / usdc_scale
    );
    println!(
        "Round Size:     {} ({:.6} WALN)",
        state.round_size_waln,
        state.round_size_waln as f64 / waln_scale
    );
    println!("Lock Period:    {} seconds", state.lock_period_seconds);
    println!("USDC Withdraw Lock: {} seconds", state.usdc_withdraw_lock_seconds);
    println!("Paused:         {}", state.paused == 1);

    println!("\n--- Round Status ---");
    println!("Round Started:  {}", state.round_started == 1);
    println!("Round Count:    {}", state.round_count);
    println!(
        "Current Round WALN:  {} ({:.6} WALN)",
        state.current_round_waln,
        state.current_round_waln as f64 / waln_scale
    );
    println!(
        "Current Round Floor: {} ({:.6} USDC)",
        state.current_round_floor_price,
        state.current_round_floor_price as f64 / usdc_scale
    );
    println!(
        "Current Round Size:  {} ({:.6} WALN)",
        state.current_round_size_waln,
        state.current_round_size_waln as f64 / waln_scale
    );
    println!(
        "USDC Locked for Round: {} ({:.6} USDC)",
        state.total_usdc_locked_for_round,
        state.total_usdc_locked_for_round as f64 / usdc_scale
    );

    println!("\n--- Lobby ---");
    println!(
        "Total USDC in Lobby: {} ({:.6} USDC)",
        state.total_usdc_in_lobby,
        state.total_usdc_in_lobby as f64 / usdc_scale
    );
    println!("Total AAT Volume:    {}", state.total_aat_volume);
    println!(
        "WALN Dust Carryover: {} ({:.6} WALN)",
        state.waln_dust_carryover,
        state.waln_dust_carryover as f64 / waln_scale
    );

    println!("\n--- Investor Pool ({} investors) ---", pool.count);
    let count = pool.count as usize;
    for record in pool.investors[..count].iter() {
        if record.investor == Pubkey::default() {
            continue;
        }
        println!(
            "  {} | deposited: {:.6} USDC | locked: {:.6} USDC | committed: {:.6} USDC | waln: {:.6} WALN | aat: {} | usdc_unlock_ts: {}",
            record.investor,
            record.usdc_deposited as f64 / usdc_scale,
            record.usdc_locked_current_round as f64 / usdc_scale,
            record.usdc_committed as f64 / usdc_scale,
            record.waln_purchased_total as f64 / waln_scale,
            record.aat_volume,
            record.usdc_unlock_ts,
        );
    }

    Ok(())
}

pub fn handle_initialize(
    program: &Program<Arc<Keypair>>,
    usdc_mint: Pubkey,
    waln_mint: Pubkey,
    floor_price_usdc: u64,
    round_size_waln: u64,
    lock_period_seconds: i64,
) -> Result<()> {
    let program_id = program.id();
    let (contract_state, _) = get_contract_state_address(&program_id);
    let (usdc_vault, _) = get_usdc_vault_address(&program_id);
    let (waln_vault, _) = get_waln_vault_address(&program_id);
    let (investor_pool, _) = get_investor_pool_address(&program_id);

    println!("Initializing floor program...");
    println!("  USDC Mint:        {}", usdc_mint);
    println!("  WALN Mint:        {}", waln_mint);
    println!("  Floor Price:      {} (raw)", floor_price_usdc);
    println!("  Round Size:       {} (raw)", round_size_waln);
    println!("  Lock Period:      {} seconds", lock_period_seconds);

    let tx = program
        .request()
        .accounts(floor_program::accounts::Initialize {
            admin: program.payer(),
            contract_state,
            usdc_mint,
            waln_mint,
            usdc_vault,
            waln_vault,
            investor_pool,
            system_program: system_program::ID,
            usdc_token_program: anchor_spl::token::ID,
            waln_token_program: anchor_spl::token_2022::ID,
        })
        .args(floor_program::instruction::Initialize {
            floor_price_usdc,
            round_size_waln,
            lock_period_seconds,
        })
        .send()?;

    println!("Transaction successful: {}", tx);
    Ok(())
}

pub fn handle_set_floor_price(program: &Program<Arc<Keypair>>, new_price_usdc: u64) -> Result<()> {
    let program_id = program.id();
    let (contract_state, _) = get_contract_state_address(&program_id);

    println!("Setting floor price to: {} (raw)", new_price_usdc);

    let tx = program
        .request()
        .accounts(floor_program::accounts::AdminOnly {
            admin: program.payer(),
            contract_state,
        })
        .args(floor_program::instruction::SetFloorPrice { new_price_usdc })
        .send()?;

    println!("Transaction successful: {}", tx);
    Ok(())
}

pub fn handle_set_round_size(
    program: &Program<Arc<Keypair>>,
    new_round_size_waln: u64,
) -> Result<()> {
    let program_id = program.id();
    let (contract_state, _) = get_contract_state_address(&program_id);

    println!("Setting round size to: {} (raw)", new_round_size_waln);

    let tx = program
        .request()
        .accounts(floor_program::accounts::AdminOnly {
            admin: program.payer(),
            contract_state,
        })
        .args(floor_program::instruction::SetRoundSize {
            new_round_size_waln,
        })
        .send()?;

    println!("Transaction successful: {}", tx);
    Ok(())
}

pub fn handle_set_lock_period(program: &Program<Arc<Keypair>>, new_lock_period: i64) -> Result<()> {
    let program_id = program.id();
    let (contract_state, _) = get_contract_state_address(&program_id);

    println!("Setting lock period to: {} seconds", new_lock_period);

    let tx = program
        .request()
        .accounts(floor_program::accounts::AdminOnly {
            admin: program.payer(),
            contract_state,
        })
        .args(floor_program::instruction::SetLockPeriod { new_lock_period })
        .send()?;

    println!("Transaction successful: {}", tx);
    Ok(())
}

pub fn handle_set_paused(program: &Program<Arc<Keypair>>, paused: bool) -> Result<()> {
    let program_id = program.id();
    let (contract_state, _) = get_contract_state_address(&program_id);

    println!("Setting paused to: {}", paused);

    let tx = program
        .request()
        .accounts(floor_program::accounts::AdminOnly {
            admin: program.payer(),
            contract_state,
        })
        .args(floor_program::instruction::SetPaused { paused })
        .send()?;

    println!("Transaction successful: {}", tx);
    Ok(())
}

pub fn handle_cancel_round(program: &Program<Arc<Keypair>>) -> Result<()> {
    let program_id = program.id();
    let (contract_state, _) = get_contract_state_address(&program_id);
    let (investor_pool, _) = get_investor_pool_address(&program_id);

    println!("Cancelling current round...");

    let tx = program
        .request()
        .accounts(floor_program::accounts::CancelRound {
            admin: program.payer(),
            contract_state,
            investor_pool,
        })
        .args(floor_program::instruction::CancelRound {})
        .send()?;

    println!("Transaction successful: {}", tx);
    Ok(())
}

pub fn handle_fund_treasury(program: &Program<Arc<Keypair>>, amount_lamports: u64) -> Result<()> {
    let program_id = program.id();
    let (contract_state, _) = get_contract_state_address(&program_id);
    let (treasury, _) = get_treasury_address(&program_id);

    println!(
        "Funding treasury with {} lamports ({:.9} SOL)...",
        amount_lamports,
        amount_lamports as f64 / 1e9
    );

    let tx = program
        .request()
        .accounts(floor_program::accounts::FundTreasury {
            admin: program.payer(),
            contract_state,
            treasury,
            system_program: system_program::ID,
        })
        .args(floor_program::instruction::FundTreasury {
            amount: amount_lamports,
        })
        .send()?;

    println!("Transaction successful: {}", tx);
    Ok(())
}

pub fn handle_transfer_authority(
    program: &Program<Arc<Keypair>>,
    new_admin: Pubkey,
) -> Result<()> {
    let program_id = program.id();
    let (contract_state, _) = get_contract_state_address(&program_id);

    println!("Initiating admin authority transfer to: {}", new_admin);

    let tx = program
        .request()
        .accounts(floor_program::accounts::TransferAuthority {
            admin: program.payer(),
            new_admin,
            contract_state,
        })
        .args(floor_program::instruction::TransferAuthority {})
        .send()?;

    println!("Transaction successful: {}", tx);
    println!("New admin must call 'accept-authority' to complete the transfer.");
    Ok(())
}

pub fn handle_accept_authority(program: &Program<Arc<Keypair>>) -> Result<()> {
    let program_id = program.id();
    let (contract_state, _) = get_contract_state_address(&program_id);

    println!("Accepting admin authority transfer...");

    let tx = program
        .request()
        .accounts(floor_program::accounts::AcceptAuthority {
            pending_admin: program.payer(),
            contract_state,
        })
        .args(floor_program::instruction::AcceptAuthority {})
        .send()?;

    println!("Transaction successful: {}", tx);
    println!("Authority transfer complete. You are now the admin.");
    Ok(())
}

pub fn handle_set_usdc_withdraw_lock(
    program: &Program<Arc<Keypair>>,
    new_lock_seconds: i64,
) -> Result<()> {
    let program_id = program.id();
    let (contract_state, _) = get_contract_state_address(&program_id);

    println!("Setting USDC withdraw lock to: {} seconds", new_lock_seconds);

    let tx = program
        .request()
        .accounts(floor_program::accounts::AdminOnly {
            admin: program.payer(),
            contract_state,
        })
        .args(floor_program::instruction::SetUsdcWithdrawLock { new_lock_seconds })
        .send()?;

    println!("Transaction successful: {}", tx);
    Ok(())
}

pub fn handle_set_investor_usdc_unlock(
    program: &Program<Arc<Keypair>>,
    investor: Pubkey,
    new_unlock_ts: i64,
) -> Result<()> {
    let program_id = program.id();
    let (contract_state, _) = get_contract_state_address(&program_id);
    let (investor_pool, _) = get_investor_pool_address(&program_id);

    println!(
        "Setting USDC unlock timestamp for investor {} to: {} (Unix)",
        investor, new_unlock_ts
    );

    let tx = program
        .request()
        .accounts(floor_program::accounts::SetInvestorUsdcUnlock {
            admin: program.payer(),
            contract_state,
            investor_pool,
        })
        .args(floor_program::instruction::SetInvestorUsdcUnlock {
            investor,
            new_unlock_ts,
        })
        .send()?;

    println!("Transaction successful: {}", tx);
    Ok(())
}

pub fn handle_mint_aat_nft(
    program: &Program<Arc<Keypair>>,
    investor: Pubkey,
    aat_volume: u64,
) -> Result<()> {
    let program_id = program.id();
    let (contract_state, _) = get_contract_state_address(&program_id);
    let (mint, _) = get_aat_nft_mint_address(&investor, &program_id);
    let (nft_authority, _) = get_nft_authority_address(&program_id);

    let investor_aat_account =
        anchor_spl::associated_token::get_associated_token_address_with_program_id(
            &investor,
            &mint,
            &anchor_spl::token_2022::ID,
        );

    println!("Minting AAT NFT for investor: {}", investor);
    println!("  AAT Volume:  {}", aat_volume);
    println!("  NFT Mint:    {}", mint);
    println!("  Investor ATA: {}", investor_aat_account);

    let tx = program
        .request()
        .accounts(floor_program::accounts::MintAatNft {
            admin: program.payer(),
            mint,
            contract_state,
            investor,
            investor_aat_account,
            nft_authority,
            system_program: system_program::ID,
            token_program: anchor_spl::token_2022::ID,
            associated_token_program: anchor_spl::associated_token::ID,
        })
        .args(floor_program::instruction::MintAatNft { aat_volume })
        .send()?;

    println!("Transaction successful: {}", tx);
    Ok(())
}
