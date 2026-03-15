use anchor_lang::prelude::*;

#[error_code]
pub enum FloorError {
    #[msg("Contract is paused")]
    ContractPaused,
    #[msg("Sell amount exceeds remaining round capacity; split into smaller sells")]
    SellAmountExceedsRound,
    #[msg("Amount must be greater than zero")]
    ZeroAmount,
    #[msg("Insufficient funds available")]
    InsufficientFunds,
    #[msg("wALN is not yet unlocked")]
    NotYetUnlocked,
    #[msg("wALN has already been claimed")]
    AlreadyClaimed,
    #[msg("Arithmetic overflow")]
    ArithmeticOverflow,
    #[msg("Invalid investor — account does not belong to signer")]
    InvalidInvestor,
    #[msg("No AAT staked in lobby")]
    NoAatStaked,
    #[msg("Invalid remaining accounts layout")]
    InvalidRemainingAccounts,
    #[msg("Unauthorized — signer is not admin")]
    Unauthorized,
    // #[msg("AAT mint must have the NonTransferable Token-2022 extension")]
    // AatMintNotNonTransferable,
}
