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
    #[msg("No eligible investors with valid AAT NFTs and USDC deposited")]
    NoEligibleInvestors,
    #[msg("Investor does not hold a valid AAT NFT from the configured collection")]
    NoAatNft,
    #[msg("Core Asset fails verification (wrong collection, missing attribute, or invalid format)")]
    InvalidAatNft,
    #[msg("Invalid remaining accounts layout")]
    InvalidRemainingAccounts,
    #[msg("Unauthorized — signer is not admin")]
    Unauthorized,
    #[msg("Mint account has insufficient space for required extensions")]
    InvalidMintAccountSpace,
    #[msg("Mint does not match the configured mint for this contract")]
    InvalidMint,
    #[msg("Cannot withdraw while funds are locked in an active round")]
    FundsLocked,
    #[msg("Floor price and round size must be greater than zero")]
    InvalidParameter,
    #[msg("Total wALN allocation across all NFTs would exceed 1_000_000 (100%)")]
    WalnAllocationLimitExceeded,
    #[msg("Investors have insufficient deposits to cover the full round size")]
    InsufficientDepositsForRound,
    #[msg("Invalid hook accounts count")]
    InvalidHookAccountsCount,
    #[msg("Invalid hook accounts")]
    InvalidHookAccounts,
    #[msg("Signer is not the pending admin")]
    NotPendingAdmin,
    #[msg("No pending authority transfer in progress")]
    NoPendingAdmin,
    #[msg("USDC is still locked — withdrawal is not allowed before the unlock timestamp")]
    UsdcLocked,
    #[msg("Cannot update AAT volume while investor funds are locked in an active round")]
    RoundInProgress,
}
