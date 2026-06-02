use anchor_client::solana_sdk::pubkey::Pubkey;
use floor_program::seeds::{
    AAT_NFT_SEED, CONTRACT_STATE_SEED, INVESTOR_POOL_SEED, TREASURY_SEED, USDC_VAULT_SEED,
    WALN_VAULT_SEED,
};

pub(crate) fn get_contract_state_address(program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[CONTRACT_STATE_SEED], program_id)
}

pub(crate) fn get_usdc_vault_address(program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[USDC_VAULT_SEED], program_id)
}

pub(crate) fn get_waln_vault_address(program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[WALN_VAULT_SEED], program_id)
}

pub(crate) fn get_investor_pool_address(program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[INVESTOR_POOL_SEED], program_id)
}

pub(crate) fn get_treasury_address(program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[TREASURY_SEED], program_id)
}

pub(crate) fn get_aat_nft_mint_address(investor: &Pubkey, program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[AAT_NFT_SEED, investor.as_ref()], program_id)
}

pub(crate) fn get_nft_authority_address(program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"nft_authority"], program_id)
}

pub(crate) fn get_program_data_address(program_id: &Pubkey) -> (Pubkey, u8) {
    let bpf_loader_upgradeable = "BPFLoaderUpgradeab1e11111111111111111111111"
        .parse::<Pubkey>()
        .expect("valid BPF upgradeable loader id");
    Pubkey::find_program_address(&[program_id.as_ref()], &bpf_loader_upgradeable)
}
