#![forbid(unsafe_code)]

use escrow_types::{canonical_bytes, fill_amounts, Order, Side, SignedOrder};
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::json_types::U128;
use near_sdk::store::{LookupMap, LookupSet};
use near_sdk::{
    assert_one_yocto, env, ext_contract, near, AccountId, BorshStorageKey, Gas, NearToken,
    PromiseResult,
};

#[ext_contract(ext_ft)]
trait FungibleToken {
    fn ft_transfer(&mut self, receiver_id: AccountId, amount: U128, memo: Option<String>);
}

#[derive(BorshStorageKey, BorshSerialize, BorshDeserialize, Clone, Copy, Hash, PartialEq, Eq)]
enum EscrowStorageKey {
    Balances,
    Keys,
    Filled,
    Cancelled,
}

type BalanceKey = (AccountId, AccountId);
type AccountHashKey = (AccountId, [u8; 32]);

#[near(contract_state)]
#[derive(BorshDeserialize, BorshSerialize)]
pub struct Escrow {
    balances: LookupMap<BalanceKey, u128>,
    keys: LookupMap<AccountId, [u8; 32]>,
    filled: LookupMap<AccountHashKey, u64>,
    cancelled: LookupSet<AccountHashKey>,
}

impl Default for Escrow {
    fn default() -> Self {
        Self {
            balances: LookupMap::new(EscrowStorageKey::Balances),
            keys: LookupMap::new(EscrowStorageKey::Keys),
            filled: LookupMap::new(EscrowStorageKey::Filled),
            cancelled: LookupSet::new(EscrowStorageKey::Cancelled),
        }
    }
}

#[derive(near_sdk::serde::Serialize, near_sdk::serde::Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(crate = "near_sdk::serde")]
pub struct OrderStateView {
    pub filled_lots: u64,
    pub cancelled: bool,
}

#[near]
impl Escrow {
    #[init]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn ft_on_transfer(
        &mut self,
        sender_id: AccountId,
        amount: U128,
        _msg: String,
    ) -> String {
        let token = env::predecessor_account_id();
        let credit = amount.0;
        self.credit_balance(&sender_id, &token, credit);
        log_deposit(&sender_id, &token, credit);
        "0".to_string()
    }

    pub fn register_key(&mut self, public_key_hex: String) {
        let account = env::predecessor_account_id();
        let pk = decode_hex_32(&public_key_hex, true);
        self.keys.insert(account.clone(), pk);
        log_key_registered(&account);
    }

    #[payable]
    pub fn withdraw(
        &mut self,
        token_id: AccountId,
        amount: U128,
        receiver_id: Option<AccountId>,
    ) {
        assert_one_yocto();
        let account = env::predecessor_account_id();
        let receiver = receiver_id.unwrap_or_else(|| account.clone());
        let amt = amount.0;
        self.debit_balance(&account, &token_id, amt);
        log_withdraw(&account, &token_id, amt, &receiver);
        let _promise = ext_ft::ext(token_id.clone())
            .with_attached_deposit(NearToken::from_yoctonear(1))
            .with_static_gas(Gas::from_tgas(10))
            .ft_transfer(receiver.clone(), amount, None)
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(Gas::from_tgas(5))
                    .on_withdraw(account, token_id, amount, receiver),
            );
    }

    #[private]
    pub fn on_withdraw(
        &mut self,
        account_id: AccountId,
        token_id: AccountId,
        amount: U128,
        _receiver_id: AccountId,
    ) {
        if env::promise_results_count() != 1 {
            self.refund_failed_withdraw(account_id, token_id, amount);
            return;
        }
        match env::promise_result(0) {
            PromiseResult::Successful(_) => {}
            PromiseResult::Failed => {
                self.refund_failed_withdraw(account_id, token_id, amount);
            }
        }
    }

    pub fn refund_failed_withdraw(
        &mut self,
        account_id: AccountId,
        token_id: AccountId,
        amount: U128,
    ) {
        self.credit_balance(&account_id, &token_id, amount.0);
    }

    pub fn settle(
        &mut self,
        buy: SignedOrder,
        sell: SignedOrder,
        fill_lots: u64,
        price_ticks: u64,
    ) {
        let contract_id = env::current_account_id().to_string();
        validate_order_version_and_contract(&buy.order, &contract_id);
        validate_order_version_and_contract(&sell.order, &contract_id);
        validate_not_expired(&buy.order);
        validate_not_expired(&sell.order);

        if buy.order.side != Side::Buy {
            env::panic_str("same_side");
        }
        if sell.order.side != Side::Sell {
            env::panic_str("same_side");
        }
        if buy.order.account_id == sell.order.account_id {
            env::panic_str("same_account");
        }
        validate_same_market(&buy.order, &sell.order);

        let buy_account = parse_account(&buy.order.account_id);
        let sell_account = parse_account(&sell.order.account_id);

        let buy_hash = hash_order(&buy.order);
        let sell_hash = hash_order(&sell.order);

        verify_registered_and_signature(&self.keys, &buy, &buy_account, &buy_hash);
        verify_registered_and_signature(&self.keys, &sell, &sell_account, &sell_hash);

        if self.is_cancelled(&buy_account, &buy_hash) {
            env::panic_str("order_cancelled");
        }
        if self.is_cancelled(&sell_account, &sell_hash) {
            env::panic_str("order_cancelled");
        }

        if sell.order.price_ticks > price_ticks || price_ticks > buy.order.price_ticks {
            env::panic_str("price_out_of_limits");
        }

        if fill_lots == 0 {
            env::panic_str("zero_fill");
        }

        let buy_filled = self.filled_lots(&buy_account, &buy_hash);
        let sell_filled = self.filled_lots(&sell_account, &sell_hash);
        match buy_filled.checked_add(fill_lots) {
            None => env::panic_str("overfill"),
            Some(v) if v > buy.order.qty_lots => env::panic_str("overfill"),
            Some(_) => {}
        }
        match sell_filled.checked_add(fill_lots) {
            None => env::panic_str("overfill"),
            Some(v) if v > sell.order.qty_lots => env::panic_str("overfill"),
            Some(_) => {}
        }

        let Some((base_amount, quote_amount)) = fill_amounts(&buy.order, fill_lots, price_ticks)
        else {
            env::panic_str("amount_overflow");
        };

        let base_token = parse_account(&buy.order.base_token);
        let quote_token = parse_account(&buy.order.quote_token);

        self.ensure_balance(&buy_account, &quote_token, quote_amount);
        self.ensure_balance(&sell_account, &base_token, base_amount);

        self.debit_balance(&buy_account, &quote_token, quote_amount);
        self.credit_balance(&buy_account, &base_token, base_amount);
        self.credit_balance(&sell_account, &quote_token, quote_amount);
        self.debit_balance(&sell_account, &base_token, base_amount);

        self.filled
            .insert((buy_account.clone(), buy_hash), buy_filled + fill_lots);
        self.filled
            .insert((sell_account.clone(), sell_hash), sell_filled + fill_lots);

        log_fill(
            &buy_hash,
            &sell_hash,
            &buy_account,
            &sell_account,
            fill_lots,
            price_ticks,
            base_amount,
            quote_amount,
        );
    }

    pub fn cancel_order(&mut self, order_hash: String) {
        let account = env::predecessor_account_id();
        let hash = parse_order_hash_hex(&order_hash);
        let key = (account.clone(), hash);
        self.cancelled.insert(key);
        log_cancel(&account, &hash);
    }

    pub fn balance_of(&self, account_id: AccountId, token_id: AccountId) -> U128 {
        U128(self.balance_get(&account_id, &token_id))
    }

    pub fn order_state(&self, account_id: AccountId, order_hash: String) -> OrderStateView {
        let hash = parse_order_hash_hex(&order_hash);
        OrderStateView {
            filled_lots: self.filled_lots(&account_id, &hash),
            cancelled: self.is_cancelled(&account_id, &hash),
        }
    }

    pub fn get_key(&self, account_id: AccountId) -> Option<String> {
        self.keys.get(&account_id).map(|pk| bytes32_to_hex(pk))
    }
}

impl Escrow {
    fn balance_get(&self, account: &AccountId, token: &AccountId) -> u128 {
        self.balances
            .get(&(account.clone(), token.clone()))
            .copied()
            .unwrap_or(0)
    }

    fn credit_balance(&mut self, account: &AccountId, token: &AccountId, amount: u128) {
        let key = (account.clone(), token.clone());
        let next = self.balance_get(account, token).saturating_add(amount);
        self.balances.insert(key, next);
    }

    fn debit_balance(&mut self, account: &AccountId, token: &AccountId, amount: u128) {
        let current = self.balance_get(account, token);
        if current < amount {
            env::panic_str("insufficient_balance");
        }
        let key = (account.clone(), token.clone());
        self.balances.insert(key, current - amount);
    }

    fn ensure_balance(&self, account: &AccountId, token: &AccountId, need: u128) {
        if self.balance_get(account, token) < need {
            env::panic_str("insufficient_balance");
        }
    }

    fn filled_lots(&self, account: &AccountId, hash: &[u8; 32]) -> u64 {
        self.filled
            .get(&(account.clone(), *hash))
            .copied()
            .unwrap_or(0)
    }

    fn is_cancelled(&self, account: &AccountId, hash: &[u8; 32]) -> bool {
        self.cancelled.contains(&(account.clone(), *hash))
    }
}

fn validate_order_version_and_contract(order: &Order, contract_id: &str) {
    if order.v != 1 {
        env::panic_str("wrong_version");
    }
    if order.contract_id != contract_id {
        env::panic_str("wrong_contract");
    }
}

fn validate_not_expired(order: &Order) {
    if env::block_timestamp() >= order.expiry_ns {
        env::panic_str("order_expired");
    }
}

fn validate_same_market(buy: &Order, sell: &Order) {
    if buy.base_token != sell.base_token
        || buy.quote_token != sell.quote_token
        || buy.lot_size_base != sell.lot_size_base
        || buy.quote_per_tick_lot != sell.quote_per_tick_lot
    {
        env::panic_str("market_mismatch");
    }
}

fn parse_account(raw: &str) -> AccountId {
    raw.parse().unwrap_or_else(|_| env::panic_str("wrong_contract"))
}

fn hash_order(order: &Order) -> [u8; 32] {
    let Ok(bytes) = canonical_bytes(order) else {
        env::panic_str("amount_overflow");
    };
    env::sha256(&bytes)
}

fn verify_registered_and_signature(
    keys: &LookupMap<AccountId, [u8; 32]>,
    signed: &SignedOrder,
    account: &AccountId,
    hash: &[u8; 32],
) {
    let Some(pk) = keys.get(account) else {
        env::panic_str("key_not_registered");
    };
    let sig = decode_hex_64(&signed.signature);
    if !env::ed25519_verify(&sig, hash, pk) {
        env::panic_str("bad_signature");
    }
}

fn decode_hex_32(hex_str: &str, enforce_key_length: bool) -> [u8; 32] {
    if enforce_key_length && hex_str.len() != 64 {
        env::panic_str("bad_key_length");
    }
    let bytes = hex::decode(hex_str).unwrap_or_else(|_| env::panic_str("bad_hex"));
    if bytes.len() != 32 {
        if enforce_key_length {
            env::panic_str("bad_key_length");
        }
        env::panic_str("bad_hex");
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    out
}

fn decode_hex_64(hex_str: &str) -> [u8; 64] {
    if hex_str.len() != 128 {
        env::panic_str("bad_signature");
    }
    let bytes = hex::decode(hex_str).unwrap_or_else(|_| env::panic_str("bad_hex"));
    if bytes.len() != 64 {
        env::panic_str("bad_signature");
    }
    let mut out = [0u8; 64];
    out.copy_from_slice(&bytes);
    out
}

fn parse_order_hash_hex(hex_str: &str) -> [u8; 32] {
    decode_hex_32(hex_str, false)
}

fn bytes32_to_hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn log_deposit(account: &AccountId, token: &AccountId, amount: u128) {
    env::log_str(&format!(
        "EVENT_JSON:{{\"standard\":\"terra_escrow\",\"version\":\"0.1.0\",\"event\":\"deposit\",\"data\":{{\"account\":\"{account}\",\"token\":\"{token}\",\"amount\":\"{amount}\"}}}}"
    ));
}

fn log_withdraw(account: &AccountId, token: &AccountId, amount: u128, receiver: &AccountId) {
    env::log_str(&format!(
        "EVENT_JSON:{{\"standard\":\"terra_escrow\",\"version\":\"0.1.0\",\"event\":\"withdraw\",\"data\":{{\"account\":\"{account}\",\"token\":\"{token}\",\"amount\":\"{amount}\",\"receiver\":\"{receiver}\"}}}}"
    ));
}

fn log_key_registered(account: &AccountId) {
    env::log_str(&format!(
        "EVENT_JSON:{{\"standard\":\"terra_escrow\",\"version\":\"0.1.0\",\"event\":\"key_registered\",\"data\":{{\"account\":\"{account}\"}}}}"
    ));
}

fn log_cancel(account: &AccountId, hash: &[u8; 32]) {
    env::log_str(&format!(
        "EVENT_JSON:{{\"standard\":\"terra_escrow\",\"version\":\"0.1.0\",\"event\":\"cancel\",\"data\":{{\"account\":\"{account}\",\"order_hash\":\"{}\"}}}}",
        bytes32_to_hex(hash)
    ));
}

fn log_fill(
    buy_hash: &[u8; 32],
    sell_hash: &[u8; 32],
    buyer: &AccountId,
    seller: &AccountId,
    fill_lots: u64,
    price_ticks: u64,
    base_amount: u128,
    quote_amount: u128,
) {
    env::log_str(&format!(
        "EVENT_JSON:{{\"standard\":\"terra_escrow\",\"version\":\"0.1.0\",\"event\":\"fill\",\"data\":{{\"buy_hash\":\"{}\",\"sell_hash\":\"{}\",\"buyer\":\"{buyer}\",\"seller\":\"{seller}\",\"fill_lots\":{fill_lots},\"price_ticks\":{price_ticks},\"base_amount\":\"{base_amount}\",\"quote_amount\":\"{quote_amount}\"}}}}",
        bytes32_to_hex(buy_hash),
        bytes32_to_hex(sell_hash),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use near_sdk::test_utils::{accounts, VMContextBuilder};
    use near_sdk::{testing_env, NearToken};

    const LOT: u128 = 1_000_000;
    const QUOTE_PER_TICK: u128 = 10;

    fn test_signing_key() -> SigningKey {
        SigningKey::from_bytes(&[7u8; 32])
    }

    fn pubkey_hex() -> String {
        bytes32_to_hex(&test_signing_key().verifying_key().to_bytes())
    }

    fn order_hash_for(order: &Order) -> [u8; 32] {
        let bytes = canonical_bytes(order).expect("canonical");
        env::sha256(&bytes)
    }

    fn sign_order(order: &Order, key: &SigningKey) -> SignedOrder {
        let hash = order_hash_for(order);
        let sig = key.sign(&hash);
        SignedOrder {
            order: order.clone(),
            signature: hex::encode(sig.to_bytes()),
        }
    }

    fn base_order(account: &str, side: Side, price: u64, qty: u64, nonce: u64) -> Order {
        Order {
            v: 1,
            contract_id: accounts(0).to_string(),
            account_id: account.to_string(),
            base_token: "base.testnet".to_string(),
            quote_token: "quote.testnet".to_string(),
            lot_size_base: LOT,
            quote_per_tick_lot: QUOTE_PER_TICK,
            side,
            price_ticks: price,
            qty_lots: qty,
            expiry_ns: 9_000_000_000_000_000,
            nonce,
        }
    }

    fn init_contract() -> Escrow {
        let mut ctx = VMContextBuilder::new();
        ctx.current_account_id(accounts(0));
        ctx.attached_deposit(NearToken::from_near(0));
        testing_env!(ctx.build());
        Escrow::new()
    }

    fn set_predecessor(account: AccountId) {
        let mut ctx = VMContextBuilder::new();
        ctx.current_account_id(accounts(0));
        ctx.predecessor_account_id(account);
        testing_env!(ctx.build());
    }

    fn deposit(contract: &mut Escrow, sender: AccountId, token: AccountId, amount: u128) {
        set_predecessor(token.clone());
        contract.ft_on_transfer(sender, U128(amount), String::new());
    }

    fn register(contract: &mut Escrow, account: AccountId) {
        set_predecessor(account);
        contract.register_key(pubkey_hex());
    }

    fn fund_pair(contract: &mut Escrow, buyer: AccountId, seller: AccountId, quote: u128, base: u128) {
        deposit(contract, buyer.clone(), "quote.testnet".parse().expect("id"), quote);
        deposit(contract, seller.clone(), "base.testnet".parse().expect("id"), base);
        register(contract, buyer);
        register(contract, seller);
    }

    #[test]
    fn deposit_credits_balance() {
        let mut c = init_contract();
        let alice = accounts(1);
        deposit(&mut c, alice.clone(), "quote.testnet".parse().expect("id"), 500);
        assert_eq!(c.balance_of(alice, "quote.testnet".parse().expect("id")).0, 500);
    }

    #[test]
    #[should_panic(expected = "bad_key_length")]
    fn register_key_wrong_length() {
        let mut c = init_contract();
        set_predecessor(accounts(1));
        c.register_key("abcd".to_string());
    }

    #[test]
    fn settle_happy_path() {
        let mut c = init_contract();
        let buyer = accounts(1);
        let seller = accounts(2);
        let fill_lots = 5u64;
        let price = 100u64;
        let quote_need = fill_lots as u128 * price as u128 * QUOTE_PER_TICK;
        let base_need = fill_lots as u128 * LOT;
        fund_pair(&mut c, buyer.clone(), seller.clone(), quote_need, base_need);

        let buy = sign_order(&base_order(&buyer.to_string(), Side::Buy, 100, 5, 1), &test_signing_key());
        let sell = sign_order(
            &base_order(&seller.to_string(), Side::Sell, 90, 5, 2),
            &test_signing_key(),
        );

        c.settle(buy, sell, fill_lots, price);

        let logs = near_sdk::env::logs();
        assert!(logs.iter().any(|line| line.contains("\"event\":\"fill\"")));

        assert_eq!(c.balance_of(buyer.clone(), "quote.testnet".parse().expect("id")).0, 0);
        assert_eq!(c.balance_of(buyer.clone(), "base.testnet".parse().expect("id")).0, base_need);
        assert_eq!(c.balance_of(seller.clone(), "quote.testnet".parse().expect("id")).0, quote_need);
        assert_eq!(c.balance_of(seller.clone(), "base.testnet".parse().expect("id")).0, 0);

        let buy_hash = bytes32_to_hex(&order_hash_for(&base_order(
            &buyer.to_string(),
            Side::Buy,
            100,
            5,
            1,
        )));
        let state = c.order_state(buyer, buy_hash);
        assert_eq!(state.filled_lots, fill_lots);
    }

    #[test]
    fn partial_fill_in_two_steps() {
        let mut c = init_contract();
        let buyer = accounts(1);
        let seller = accounts(2);
        fund_pair(&mut c, buyer.clone(), seller.clone(), 10_000 * QUOTE_PER_TICK, 10 * LOT);

        let buy = sign_order(&base_order(&buyer.to_string(), Side::Buy, 100, 10, 1), &test_signing_key());
        let sell = sign_order(
            &base_order(&seller.to_string(), Side::Sell, 50, 10, 2),
            &test_signing_key(),
        );

        c.settle(buy.clone(), sell.clone(), 3, 80);
        c.settle(buy, sell, 7, 75);

        let buy_hash = bytes32_to_hex(&order_hash_for(&base_order(
            &buyer.to_string(),
            Side::Buy,
            100,
            10,
            1,
        )));
        assert_eq!(
            c.order_state(buyer, buy_hash).filled_lots,
            10
        );
    }

    #[test]
    #[should_panic(expected = "bad_signature")]
    fn settle_bad_signature() {
        let mut c = init_contract();
        let buyer = accounts(1);
        let seller = accounts(2);
        fund_pair(&mut c, buyer.clone(), seller.clone(), 5000, 5 * LOT);
        let mut buy = sign_order(&base_order(&buyer.to_string(), Side::Buy, 100, 5, 1), &test_signing_key());
        buy.signature = "00".repeat(64);
        let sell = sign_order(
            &base_order(&seller.to_string(), Side::Sell, 90, 5, 2),
            &test_signing_key(),
        );
        c.settle(buy, sell, 5, 100);
    }

    #[test]
    #[should_panic(expected = "key_not_registered")]
    fn settle_key_not_registered() {
        let mut c = init_contract();
        let buyer = accounts(1);
        let seller = accounts(2);
        deposit(&mut c, buyer.clone(), "quote.testnet".parse().expect("id"), 5000);
        deposit(&mut c, seller.clone(), "base.testnet".parse().expect("id"), 5 * LOT);
        register(&mut c, seller);
        let buy = sign_order(&base_order(&buyer.to_string(), Side::Buy, 100, 5, 1), &test_signing_key());
        let sell = sign_order(
            &base_order(&seller.to_string(), Side::Sell, 90, 5, 2),
            &test_signing_key(),
        );
        c.settle(buy, sell, 5, 100);
    }

    #[test]
    #[should_panic(expected = "order_expired")]
    fn settle_expired() {
        let mut c = init_contract();
        let buyer = accounts(1);
        let seller = accounts(2);
        fund_pair(&mut c, buyer.clone(), seller.clone(), 5000, 5 * LOT);
        let mut buy_order = base_order(&buyer.to_string(), Side::Buy, 100, 5, 1);
        buy_order.expiry_ns = 1;
        let mut ctx = VMContextBuilder::new();
        ctx.current_account_id(accounts(0));
        ctx.block_timestamp(2);
        testing_env!(ctx.build());
        let buy = sign_order(&buy_order, &test_signing_key());
        let sell = sign_order(
            &base_order(&seller.to_string(), Side::Sell, 90, 5, 2),
            &test_signing_key(),
        );
        c.settle(buy, sell, 5, 100);
    }

    #[test]
    #[should_panic(expected = "wrong_contract")]
    fn settle_wrong_contract_id() {
        let mut c = init_contract();
        let buyer = accounts(1);
        let seller = accounts(2);
        fund_pair(&mut c, buyer.clone(), seller.clone(), 5000, 5 * LOT);
        let mut buy_order = base_order(&buyer.to_string(), Side::Buy, 100, 5, 1);
        buy_order.contract_id = "other.testnet".to_string();
        let buy = sign_order(&buy_order, &test_signing_key());
        let sell = sign_order(
            &base_order(&seller.to_string(), Side::Sell, 90, 5, 2),
            &test_signing_key(),
        );
        c.settle(buy, sell, 5, 100);
    }

    #[test]
    #[should_panic(expected = "market_mismatch")]
    fn settle_market_mismatch() {
        let mut c = init_contract();
        let buyer = accounts(1);
        let seller = accounts(2);
        fund_pair(&mut c, buyer.clone(), seller.clone(), 5000, 5 * LOT);
        let buy = sign_order(&base_order(&buyer.to_string(), Side::Buy, 100, 5, 1), &test_signing_key());
        let mut sell_order = base_order(&seller.to_string(), Side::Sell, 90, 5, 2);
        sell_order.quote_token = "other.quote".to_string();
        let sell = sign_order(&sell_order, &test_signing_key());
        c.settle(buy, sell, 5, 100);
    }

    #[test]
    #[should_panic(expected = "same_side")]
    fn settle_same_side() {
        let mut c = init_contract();
        let buyer = accounts(1);
        let seller = accounts(2);
        fund_pair(&mut c, buyer.clone(), seller.clone(), 5000, 5 * LOT);
        let buy = sign_order(&base_order(&buyer.to_string(), Side::Buy, 100, 5, 1), &test_signing_key());
        let sell = sign_order(
            &base_order(&seller.to_string(), Side::Buy, 90, 5, 2),
            &test_signing_key(),
        );
        c.settle(buy, sell, 5, 100);
    }

    #[test]
    #[should_panic(expected = "same_account")]
    fn settle_same_account() {
        let mut c = init_contract();
        let buyer = accounts(1);
        fund_pair(&mut c, buyer.clone(), buyer.clone(), 5000, 5 * LOT);
        let buy = sign_order(&base_order(&buyer.to_string(), Side::Buy, 100, 5, 1), &test_signing_key());
        let sell = sign_order(
            &base_order(&buyer.to_string(), Side::Sell, 90, 5, 2),
            &test_signing_key(),
        );
        c.settle(buy, sell, 5, 100);
    }

    #[test]
    #[should_panic(expected = "price_out_of_limits")]
    fn settle_price_above_buy_limit() {
        let mut c = init_contract();
        let buyer = accounts(1);
        let seller = accounts(2);
        fund_pair(&mut c, buyer.clone(), seller.clone(), 5000, 5 * LOT);
        let buy = sign_order(&base_order(&buyer.to_string(), Side::Buy, 100, 5, 1), &test_signing_key());
        let sell = sign_order(
            &base_order(&seller.to_string(), Side::Sell, 90, 5, 2),
            &test_signing_key(),
        );
        c.settle(buy, sell, 5, 101);
    }

    #[test]
    #[should_panic(expected = "price_out_of_limits")]
    fn settle_price_below_sell_limit() {
        let mut c = init_contract();
        let buyer = accounts(1);
        let seller = accounts(2);
        fund_pair(&mut c, buyer.clone(), seller.clone(), 5000, 5 * LOT);
        let buy = sign_order(&base_order(&buyer.to_string(), Side::Buy, 100, 5, 1), &test_signing_key());
        let sell = sign_order(
            &base_order(&seller.to_string(), Side::Sell, 90, 5, 2),
            &test_signing_key(),
        );
        c.settle(buy, sell, 5, 89);
    }

    #[test]
    #[should_panic(expected = "overfill")]
    fn settle_overfill() {
        let mut c = init_contract();
        let buyer = accounts(1);
        let seller = accounts(2);
        fund_pair(&mut c, buyer.clone(), seller.clone(), 5000, 5 * LOT);
        let buy = sign_order(&base_order(&buyer.to_string(), Side::Buy, 100, 5, 1), &test_signing_key());
        let sell = sign_order(
            &base_order(&seller.to_string(), Side::Sell, 90, 5, 2),
            &test_signing_key(),
        );
        c.settle(buy, sell, 6, 100);
    }

    #[test]
    #[should_panic(expected = "zero_fill")]
    fn settle_zero_fill() {
        let mut c = init_contract();
        let buyer = accounts(1);
        let seller = accounts(2);
        fund_pair(&mut c, buyer.clone(), seller.clone(), 5000, 5 * LOT);
        let buy = sign_order(&base_order(&buyer.to_string(), Side::Buy, 100, 5, 1), &test_signing_key());
        let sell = sign_order(
            &base_order(&seller.to_string(), Side::Sell, 90, 5, 2),
            &test_signing_key(),
        );
        c.settle(buy, sell, 0, 100);
    }

    #[test]
    #[should_panic(expected = "insufficient_balance")]
    fn settle_insufficient_balance() {
        let mut c = init_contract();
        let buyer = accounts(1);
        let seller = accounts(2);
        fund_pair(&mut c, buyer.clone(), seller.clone(), 1, 5 * LOT);
        let buy = sign_order(&base_order(&buyer.to_string(), Side::Buy, 100, 5, 1), &test_signing_key());
        let sell = sign_order(
            &base_order(&seller.to_string(), Side::Sell, 90, 5, 2),
            &test_signing_key(),
        );
        c.settle(buy, sell, 5, 100);
    }

    #[test]
    #[should_panic(expected = "amount_overflow")]
    fn settle_amount_overflow() {
        let mut c = init_contract();
        let buyer = accounts(1);
        let seller = accounts(2);
        fund_pair(&mut c, buyer.clone(), seller.clone(), u128::MAX, u128::MAX);
        let mut buy_order = base_order(&buyer.to_string(), Side::Buy, 100, 5, 1);
        buy_order.lot_size_base = u128::MAX;
        buy_order.quote_per_tick_lot = u128::MAX;
        let mut sell_order = base_order(&seller.to_string(), Side::Sell, 90, 5, 2);
        sell_order.lot_size_base = u128::MAX;
        sell_order.quote_per_tick_lot = u128::MAX;
        let buy = sign_order(&buy_order, &test_signing_key());
        let sell = sign_order(&sell_order, &test_signing_key());
        c.settle(buy, sell, 2, u64::MAX);
    }

    #[test]
    #[should_panic(expected = "order_cancelled")]
    fn settle_cancelled() {
        let mut c = init_contract();
        let buyer = accounts(1);
        let seller = accounts(2);
        fund_pair(&mut c, buyer.clone(), seller.clone(), 5000, 5 * LOT);
        let buy_order = base_order(&buyer.to_string(), Side::Buy, 100, 5, 1);
        let buy_hash = bytes32_to_hex(&order_hash_for(&buy_order));
        set_predecessor(buyer.clone());
        c.cancel_order(buy_hash);
        let buy = sign_order(&buy_order, &test_signing_key());
        let sell = sign_order(
            &base_order(&seller.to_string(), Side::Sell, 90, 5, 2),
            &test_signing_key(),
        );
        c.settle(buy, sell, 5, 100);
    }

    #[test]
    fn cancel_only_affects_caller() {
        let mut c = init_contract();
        let buyer = accounts(1);
        let seller = accounts(2);
        register(&mut c, buyer.clone());
        let buy_order = base_order(&buyer.to_string(), Side::Buy, 100, 5, 1);
        let buy_hash = bytes32_to_hex(&order_hash_for(&buy_order));
        set_predecessor(seller);
        c.cancel_order(buy_hash.clone());
        let state = c.order_state(buyer, buy_hash);
        assert!(!state.cancelled);
    }

    #[test]
    fn refund_failed_withdraw_restores_balance() {
        let mut c = init_contract();
        let alice = accounts(1);
        deposit(&mut c, alice.clone(), "quote.testnet".parse().expect("id"), 100);
        c.refund_failed_withdraw(
            alice.clone(),
            "quote.testnet".parse().expect("id"),
            U128(40),
        );
        assert_eq!(c.balance_of(alice.clone(), "quote.testnet".parse().expect("id")).0, 140);
    }

    #[test]
    fn withdraw_debits_then_refund_restores() {
        let mut c = init_contract();
        let alice = accounts(1);
        deposit(&mut c, alice.clone(), "quote.testnet".parse().expect("id"), 100);
        let token: AccountId = "quote.testnet".parse().expect("id");
        let mut ctx = VMContextBuilder::new();
        ctx.current_account_id(accounts(0));
        ctx.predecessor_account_id(alice.clone());
        ctx.attached_deposit(NearToken::from_yoctonear(1));
        testing_env!(ctx.build());
        c.withdraw(token.clone(), U128(40), None);
        assert_eq!(c.balance_of(alice.clone(), token.clone()).0, 60);
        c.refund_failed_withdraw(alice.clone(), token, U128(40));
        assert_eq!(c.balance_of(alice, "quote.testnet".parse().expect("id")).0, 100);
    }

    #[test]
    fn withdraw_debits_before_promise() {
        let mut c = init_contract();
        let alice = accounts(1);
        deposit(&mut c, alice.clone(), "quote.testnet".parse().expect("id"), 100);
        let mut ctx = VMContextBuilder::new();
        ctx.current_account_id(accounts(0));
        ctx.predecessor_account_id(alice.clone());
        ctx.attached_deposit(NearToken::from_yoctonear(1));
        testing_env!(ctx.build());
        c.withdraw("quote.testnet".parse().expect("id"), U128(40), None);
        assert_eq!(c.balance_of(alice, "quote.testnet".parse().expect("id")).0, 60);
    }
}
