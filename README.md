# solana-floor-program

## Overview

This project implements the **ALN Floor Contract** — a Solana on-chain program that allows investors to buy wALN (wrapped ALN) at a fixed price below the open market, providing USDC liquidity in return.

The floor contract runs alongside the [Meteora DEX](https://meteora.ag/) open market. Any user can sell wALN into the floor contract at the configured fixed price, while only investors holding an **AAT (Alien App Token)** NFT can deposit USDC as buy-side liquidity.

## Current Program ID

| Cluster | Program ID |
|---|---|
| localnet | `3tBjDJHPCMmKjxRAxTHiaUmZ9Li1D8YvWCS48pBMjRSe` |
| devnet | `3tBjDJHPCMmKjxRAxTHiaUmZ9Li1D8YvWCS48pBMjRSe` |

## Core Flow

1. **Initialize**
   - Creates the global program state, USDC vault, wALN vault, and investor pool.
   - Stores the admin, mint addresses, floor price, round size, lock period, token decimals, and PDA bumps.

2. **Mint AAT NFT**
   - Admin mints a deterministic Token-2022 AAT NFT for an investor using the `aat_nft` PDA seed and investor wallet.
   - The NFT is non-transferable and stores `aat_volume` in Token Metadata.
   - Total AAT volume is capped at `1_000_000`.

3. **Deposit USDC**
   - Investor deposits USDC into the lobby.
   - The program verifies the investor's deterministic AAT NFT and reads its `aat_volume`.
   - Deposits update the investor's lobby balance and the global `total_usdc_in_lobby`.
   - If `usdc_withdraw_lock_seconds` is enabled, the deposit sets the investor's USDC unlock timestamp.

4. **Sell wALN**
   - Any seller can sell wALN into the contract while the contract is not paused.
   - wALN must use Token-2022; if the mint has a transfer hook, the caller must provide the required hook accounts.
   - The seller receives USDC at the active round's snapshotted floor price.
   - A sell cannot exceed the remaining capacity of the current round.

5. **Round start and completion**
   - The first sell into an inactive round starts a round by locking eligible investors' USDC.
   - Eligibility requires a positive deposit, positive AAT volume, and enough deposited USDC to satisfy the round's minimum/weighted allocation checks.
   - Floor price and round size are snapshotted for the active round; admin updates apply to the next round.
   - When `current_round_waln` reaches the snapshotted round size, the program creates `RoundRecord` and `RoundLockedWaln` accounts, records investor allocations, handles wALN dust carryover, increments `round_count`, and attempts to auto-start the next round.

6. **Claim wALN**
   - Investors claim their round allocation after its unlock timestamp.
   - Claims are one-time per investor per round.
   - If wALN has a transfer hook, claim calls must provide the required hook accounts.

7. **Withdraw USDC**
   - Investors can withdraw unlocked, unallocated USDC from the lobby.
   - Passing `u64::MAX` withdraws the full available lobby balance.
   - Withdrawals are allowed while the contract is paused.

## On-Chain Accounts

| Account | Seeds | Description |
|---|---|---|
| `ProgramState` | `["contract_state"]` | Global config and accounting: admin, mints, vaults, prices, round counters, pause flag, lock settings, dust carryover, and PDA bumps. |
| `InvestorPool` | `["investor_pool"]` | Fixed-size zero-copy pool of up to 100 investor records. |
| `RoundRecord` | `["round_record", round_index]` | Completed round metadata: trigger time, wALN purchased, USDC spent, total AAT volume, and participant count. |
| `RoundLockedWaln` | `["round_locked_waln", round_index]` | Sorted per-round investor allocations with wALN amount, unlock timestamp, and claimed flag. |
| `USDC Vault` | `["usdc_vault"]` | Program-owned token account holding deposited USDC. |
| `wALN Vault` | `["waln_vault"]` | Program-owned Token-2022 account holding sold wALN until investors claim. |
| `AAT NFT Mint` | `["aat_nft", investor]` | Deterministic non-transferable Token-2022 NFT mint for an investor. |
| `AAT NFT Authority` | `["nft_authority"]` | PDA authority used to initialize AAT NFT metadata and mint supply. |
| `Treasury` | `["treasury"]` | System PDA funded with SOL by admin to pay rent for round accounts created during sell finalization. |

## Program Instructions

| Instruction | Authority | Description |
|---|---|---|
| `initialize(floor_price_usdc, round_size_waln, lock_period_seconds)` | Admin | Initializes state, vaults, and investor pool. |
| `mint_aat_nft(aat_volume)` | Admin | Mints a deterministic non-transferable Token-2022 AAT NFT to an investor. |
| `deposit_usdc(usdc_amount)` | Investor | Deposits USDC after verifying the investor's AAT NFT. |
| `withdraw_usdc(amount)` | Investor | Withdraws unlocked lobby USDC; `u64::MAX` means withdraw all available. |
| `sell_waln(waln_amount, hook_bumps)` | Any seller | Transfers wALN into the vault, pays seller USDC, and completes/starts rounds when needed. |
| `claim_waln(round_index, hook_bumps)` | Investor | Claims unlocked wALN allocation for a completed round. |
| `set_floor_price(new_price_usdc)` | Admin | Updates floor price for future rounds. |
| `set_round_size(new_round_size_waln)` | Admin | Updates round size for future rounds. |
| `set_lock_period(new_lock_period)` | Admin | Updates wALN claim lock period for future round allocations. |
| `set_usdc_withdraw_lock(new_lock_seconds)` | Admin | Updates the USDC withdrawal lock applied on future deposits. |
| `set_investor_usdc_unlock(investor, new_unlock_ts)` | Admin | Overrides one investor's USDC unlock timestamp. |
| `set_paused(paused)` | Admin | Pauses or unpauses `sell_waln`; withdrawals and claims remain available. |
| `cancel_round()` | Admin | Cancels an active empty round and returns locked USDC to investor lobby balances. |
| `fund_treasury(amount)` | Admin | Transfers SOL into the treasury PDA for round account rent. |
| `transfer_authority()` | Admin | Starts a two-step admin authority transfer to `pending_admin`. |
| `accept_authority()` | Pending admin | Accepts pending admin authority. |

## AAT NFT Details

AAT NFTs are Token-2022 mints with:

- **MetadataPointer** extension
- **NonTransferable** extension
- zero decimals
- supply of one
- `aat_volume` stored as Token Metadata additional metadata
- mint authority removed after minting

The program verifies AAT ownership by deriving the expected AAT mint PDA from `["aat_nft", investor]`, checking that it is owned by Token-2022, and reading its `aat_volume` metadata field.

## wALN and Transfer Hooks

wALN is expected to be a Token-2022 mint. The `sell_waln` and `claim_waln` instructions build Token-2022 `transfer_checked` instructions directly.

If the wALN mint has a Transfer Hook extension, callers must pass the hook accounts expected by the external hook program. The floor program validates the hook account PDAs before invoking the transfer.

Vendored IDLs and binaries for local tests are included for:

| Program | ID |
|---|---|
| Alien ID transfer hook | `AXmwHw9zuXBv5vNc28BoPfm8MS9gR3zbR5EN9nWiLMm8` |
| Solana Attestation Service | `22zoJMtdu4tQc2PzL74ZUT7FrwgB1Udec8DdW4yw4BdG` |
| Credential signer | `GKn6Gu6ZVD4M5s1csUZS2gdUCoWJyy5PcFRtbvNXKV2` |
| Session registry | `5pHXF7jCcRDS4672BwpVJyeuYToiGpEnuJBRxLmKemA` |

## Repository Layout

| Path | Purpose |
|---|---|
| `programs/floor-program/` | Anchor on-chain program. |
| `tests/floor-program.ts` | Main Anchor integration test suite. |
| `sdk/floor-sdk.ts` | TypeScript helper SDK for instructions and account fetches. |
| `integration/` | TypeScript scripts for local/devnet operations. |
| `admin-cli/` | Rust CLI for admin operations. |
| `idl/` | Vendored external program IDLs. |
| `binaries/` | Vendored external program binaries loaded by `anchor test`. |

## Setup

```bash
yarn install
```

Copy the example environment and fill in the values needed by the integration scripts:

```bash
cp .env.example .env
```

Important environment values:

| Variable | Description |
|---|---|
| `KEYPAIR_PATH` / `ANCHOR_WALLET` | Wallet used by scripts and Anchor. |
| `ANCHOR_PROVIDER_URL` | RPC URL, defaults to local validator in `.env.example`. |
| `USDC_MINT` | USDC mint used by initialization and scripts. |
| `WALN_MINT` | Token-2022 wALN mint. |
| `FLOOR_PRICE_USDC` | Floor price in raw USDC units. |
| `ROUND_SIZE_WALN` | Round size in raw wALN units. |
| `LOCK_PERIOD_SECONDS` | wALN claim lock period for allocations. |
| `USDC_WITHDRAW_LOCK_SECONDS` | USDC withdrawal lock applied on future deposits by script 16. |
| `INVESTOR` | Investor wallet for AAT minting and admin unlock override scripts. |
| `AAT_VOLUME` | AAT allocation volume for minted AAT NFT. |
| `USDC_AMOUNT` | USDC amount for deposit/withdraw scripts. |
| `WALN_AMOUNT` | wALN amount for sell script. |
| `ROUND_INDEX` | Round index for claim script. |
| `PAUSED` | Boolean for pause script. |
| `AMOUNT_LAMPORTS` | SOL amount for treasury funding. |

## Build and Test

Build the Anchor program:

```bash
anchor build
```

Run the test suite:

```bash
anchor test
```

The test validator loads external programs from `binaries/` as configured in `Anchor.toml`.

Run TypeScript formatting check:

```bash
yarn lint
```

## Deploy and Initialize

Deploy using the cluster and wallet configured in `Anchor.toml`:

```bash
anchor deploy
```

Initialize through the integration script:

```bash
yarn integration:initialize
```

## Integration Scripts

Scripts live in `integration/` and read values from `.env`.

| Script | Command | Description |
|---|---|---|
| `1_create_usdc_mint.ts` | `yarn integration:create-usdc-mint` | Creates a local test USDC mint. |
| `3_initialize.ts` | `yarn integration:initialize` | Initializes the floor contract. |
| `4_show_state.ts` | `yarn integration:show-state` | Prints state, vaults, and investor pool data. |
| `5_mint_aat_nft.ts` | `yarn integration:mint-aat-nft` | Mints an AAT NFT for `INVESTOR`. |
| `6_deposit_usdc.ts` | `yarn integration:deposit-usdc` | Deposits `USDC_AMOUNT` into the lobby. |
| `7_withdraw_usdc.ts` | `yarn integration:withdraw-usdc` | Withdraws `USDC_AMOUNT` from the investor's unlocked lobby balance. |
| `8_sell_waln.ts` | `yarn integration:sell-waln` | Sells `WALN_AMOUNT` into the contract. |
| `9_claim_waln.ts` | `yarn integration:claim-waln` | Claims allocation for `ROUND_INDEX`. |
| `10_set_floor_price.ts` | `yarn integration:set-floor-price` | Updates floor price. |
| `11_set_round_size.ts` | `yarn integration:set-round-size` | Updates future round size. |
| `12_set_lock_period.ts` | `yarn integration:set-lock-period` | Updates wALN claim lock period. |
| `13_set_paused.ts` | `yarn integration:set-paused` | Pauses or unpauses selling. |
| `14_fund_treasury.ts` | `yarn integration:fund-treasury` | Funds treasury PDA with SOL. |
| `15_cancel_round.ts` | `yarn integration:cancel-round` | Cancels the active empty round. |
| `16_set_usdc_withdraw_lock.ts` | `node -r ts-node/register integration/16_set_usdc_withdraw_lock.ts` | Updates USDC withdrawal lock duration. |
| `17_set_investor_usdc_unlock.ts` | `node -r ts-node/register integration/17_set_investor_usdc_unlock.ts` | Overrides one investor's USDC unlock timestamp. |

## Admin CLI

A Rust admin CLI is available in `admin-cli/`.

Build it from the repository root:

```bash
cargo build --release --locked --bin admin-cli
```

Example commands:

```bash
./target/release/admin-cli --cluster devnet info
./target/release/admin-cli --cluster devnet set-floor-price 100000
./target/release/admin-cli --cluster devnet set-round-size 200000000000
./target/release/admin-cli --cluster devnet set-lock-period 86400
./target/release/admin-cli --cluster devnet set-usdc-withdraw-lock 7776000
./target/release/admin-cli --cluster devnet mint-aat-nft <INVESTOR_PUBKEY> <AAT_VOLUME>
./target/release/admin-cli --cluster devnet fund-treasury 100000000
```

The CLI supports keypair path, base58 keypair file, cluster/RPC override, and program ID override flags. See `admin-cli/README.md` for detailed CLI usage.

## SDK

The TypeScript SDK in `sdk/floor-sdk.ts` provides helpers for deriving PDAs, building instructions, fetching state, and interacting with the floor contract from scripts or applications.

## Important Behavior Notes

- **Round price isolation**: `set_floor_price` does not affect an already active round.
- **Round size isolation**: `set_round_size` does not affect an already active round.
- **Sell cap**: sellers cannot sell more than the remaining capacity in the active round.
- **Pause behavior**: selling is paused by `set_paused(true)`; withdrawals and claims remain available.
- **USDC lock**: USDC withdrawal locks apply when investors deposit and can be overridden by admin.
- **Treasury funding**: treasury SOL is required for creating `RoundRecord` and `RoundLockedWaln` accounts during round finalization.
- **Dust carryover**: rounding dust from wALN allocation is carried forward and distributed in a later round.
