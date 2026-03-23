# solana-floor-program

## Overview

This project implements the **ALN Floor Contract** — a Solana on-chain program that allows investors to buy wALN (wrapped ALN) at a fixed price below the open market, providing USDC liquidity in return.

The floor contract runs alongside the [Meteora DEX](https://meteora.ag/) open market. Any user can sell wALN into the floor contract at the configured fixed price, while only investors holding an **AAT (Alien App Token)** NFT can deposit USDC as buy-side liquidity.

### How It Works

The contract uses a **Lobby + Round** model:

1. **Lobby** — Investors deposit USDC and their AAT NFT is checked to determine their proportional allocation weight. Deposited funds can only be returned to the original depositor — no admin can redirect them.
2. **Round** — A round triggers automatically when accumulated wALN sold into the contract reaches the configured `round_size_waln` threshold. The contract then allocates USDC proportionally across all eligible investors (weighted by AAT volume), purchases the accumulated wALN, and records each investor's allocation with a lock period.
3. **Claim** — After the lock period expires, investors claim their purchased wALN.

The floor price and round size are adjustable admin parameters. All fund flows are enforced by the program — there is no admin withdrawal capability.

### AAT — Alien App Token

The **Alien App Token (AAT)** is a non-transferable SPL-22 NFT that acts as an access credential for the floor contract. It is issued to equity holders of Extraterrestrial Intelligence Corporation (Alien) in proportion to their shareholding. The `aat_volume` attribute on each NFT determines the investor's maximum proportional share of each round.

### wALN and Transfer Hook

wALN uses Solana's [Token Extensions](https://solana.com/developers/guides/token-extensions/getting-started) with a Transfer Hook implemented in [solana-sas-extension](https://github.com/alien-id/solana-sas-extension). Before any wALN transfer executes, the hook verifies:

1. **Attestation existence** — a valid attestation account exists for the sender's wallet, issued by the [Solana Attestation Service (SAS)](https://sas.solana.com/).
2. **Issuer verification** — the attestation was signed by the authorised external credential signer, ensuring only credentials from the authorised party are accepted.
3. **Cross-chain account proof** — the attestation encodes proof that the wallet owner controls an account on the Alien Network, binding the Solana identity to the external identity.

If any check fails, the transfer is rejected. The floor contract must supply the correct extra accounts when invoking `sell_waln` and `claim_waln` so that the hook CPI succeeds during wALN distribution.

> **Note:** Certain wallets (e.g. the floor contract's wALN vault) are whitelisted in the hook program and bypass attestation checks. Only the hook authority can manage the whitelist.

### On-Chain State

| Account | Seeds | Description |
|---|---|---|
| `ProgramState` | `["contract_state"]` | Global state: admin, mints, vault pubkeys, floor price, round size, lock period, round counters |
| `InvestorPool` | `["investor_pool"]` | Fixed-size array of up to 100 investor records tracking deposits, locked USDC, and wALN purchased |
| `RoundRecord` | `["round_record", round_index]` | Per-round metadata: trigger time, wALN purchased, USDC spent, participant count |
| `RoundLockedWaln` | `["round_locked_waln", round_index]` | Per-round allocation array: each investor's wALN amount, unlock timestamp, and claimed flag |
| `USDC Vault` | `["usdc_vault"]` | Program-owned token account holding all deposited USDC |
| `wALN Vault` | `["waln_vault"]` | Program-owned token account holding wALN sold by users |
| `Treasury` | `["treasury"]` | SOL treasury PDA used to pay rent for round accounts |

### Program Instructions

| Instruction | Authority | Description |
|---|---|---|
| `initialize` | Admin | Initialises the program state, creates vaults, and sets initial floor price, round size, and lock period. |
| `deposit_usdc` | Investor (AAT holder) | Deposits USDC into the lobby. Verifies the investor holds a valid AAT NFT from the configured collection. |
| `withdraw_usdc` | Investor | Withdraws available USDC from the lobby. Blocked while funds are locked in an active round. |
| `sell_waln` | Any user | Sells wALN into the contract at the current floor price. Automatically triggers a round when the round threshold is reached; invokes the transfer hook when transferring wALN. |
| `claim_waln` | Investor | Claims purchased wALN for a completed round after the lock period expires; invokes the transfer hook when sending wALN to the investor. |
| `mint_aat_nft` | Admin | Mints an AAT NFT (Metaplex Core asset) to an investor wallet with the configured `aat_volume` attribute. |
| `set_floor_price` | Admin | Updates the floor price (in raw USDC units). |
| `set_round_size` | Admin | Updates the round size threshold (in raw wALN units). |
| `set_lock_period` | Admin | Updates the lock period applied to newly purchased wALN. |
| `set_paused` | Admin | Pauses or unpauses user-facing operations (`deposit_usdc`, `sell_waln`). |
| `cancel_round` | Admin | Cancels the currently active round and refunds locked USDC back to investors. |
| `fund_treasury` | Admin | Deposits SOL into the treasury PDA to cover rent for round accounts. |
| `transfer_authority` | Admin | Initiates a two-step admin authority transfer to a new address. |
| `accept_authority` | Pending admin | Completes the authority transfer. The new admin signs to accept. |

## Deploy

1. **Install dependencies:**
   ```bash
   yarn install
   ```

2. **Copy and configure environment:**
   ```bash
   cp .env.example .env
   ```
   Fill in `USDC_MINT`, `WALN_MINT`, and other required values.

3. **Build the program:**
   ```bash
   anchor build
   ```

4. **Deploy the program:**
   ```bash
   anchor deploy
   ```

5. **Initialize the contract:**
   ```bash
   yarn integration:initialize
   ```

## Integration Scripts

TypeScript scripts for interacting with the deployed contract are in [`integration/`](./integration/). Each script reads configuration from `.env`.

| Script | Command | Description |
|---|---|---|
| `1_create_usdc_mint.ts` | `yarn integration:create-usdc-mint` | Creates a local USDC mint for testing |
| `3_initialize.ts` | `yarn integration:initialize` | Initialises the floor contract |
| `4_show_state.ts` | `yarn integration:show-state` | Prints current contract state and investor pool |
| `5_mint_aat_nft.ts` | `yarn integration:mint-aat-nft` | Mints an AAT NFT to an investor wallet |
| `6_deposit_usdc.ts` | `yarn integration:deposit-usdc` | Deposits USDC into the lobby |
| `7_withdraw_usdc.ts` | `yarn integration:withdraw-usdc` | Withdraws available USDC from the lobby |
| `8_sell_waln.ts` | `yarn integration:sell-waln` | Sells wALN into the contract |
| `9_claim_waln.ts` | `yarn integration:claim-waln` | Claims purchased wALN for a round |
| `10_set_floor_price.ts` | `yarn integration:set-floor-price` | Updates the floor price |
| `11_set_round_size.ts` | `yarn integration:set-round-size` | Updates the round size |
| `12_set_lock_period.ts` | `yarn integration:set-lock-period` | Updates the lock period |
| `13_set_paused.ts` | `yarn integration:set-paused` | Pauses or unpauses the contract |
| `14_fund_treasury.ts` | `yarn integration:fund-treasury` | Funds the treasury with SOL |
| `15_cancel_round.ts` | `yarn integration:cancel-round` | Cancels the current active round |

## Running Tests

1. **Install dependencies:**
   ```bash
   yarn install
   ```

2. **Run tests:**
   ```bash
   anchor test
   ```

## Admin CLI

A Rust command-line tool for admin operations is available in [`admin-cli/`](./admin-cli/). See the [admin-cli README](./admin-cli/README.md) for full usage.

```bash
cargo build --release --locked --bin admin-cli
./target/release/admin-cli --cluster devnet info
```

## SDK

A TypeScript SDK for interacting with the floor contract is available in [`sdk/floor-sdk.ts`](./sdk/floor-sdk.ts). It exposes typed helpers for all program instructions and account fetching.

## IDLs

External program IDLs are vendored in the [`idl/`](./idl/) directory:

| File | Program | Description |
|---|---|---|
| `alien_id_transfer_hook.json` | `AXmwHw9zuXBv5vNc28BoPfm8MS9gR3zbR5EN9nWiLMm8` | Transfer hook enforcing Alien ID verification on wALN transfers |
| `credential_signer.json` | `GKn6Gu6ZVD4M5s1csUZS2gdUCoWJyy5PcFRtbvNXKV2` | External credential signer from [alien-id/solana-attestation-signer](https://github.com/alien-id/solana-attestation-signer) |
| `session_registry.json` | `5pHXF7jCcRDS4672BwpVJyeuYToiGpEnuJBRxLmKemA` | Session registry from [alien-id/solana-attestation-signer](https://github.com/alien-id/solana-attestation-signer) |
| `solana_attestation_service.json` | `22zoJMtdu4tQc2PzL74ZUT7FrwgB1Udec8DdW4yw4BdG` | [Solana Attestation Service](https://github.com/solana-attestation-service/solana-attestation-service) |

Compiled binaries (`.so` files) used in local tests are in [`binaries/`](./binaries/).
