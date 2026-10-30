//! Checks that signed orders made by `terra-keys` are accepted by the contract's `settle`.
//! The fixture is the unchanged output of `terra-keys make-order` and `terra-keys settle-args`.

use escrow::Escrow;
use escrow_types::SignedOrder;
use near_sdk::json_types::U128;
use near_sdk::serde_json::{self, Value};
use near_sdk::test_utils::VMContextBuilder;
use near_sdk::{testing_env, AccountId};

const FIXTURE: &str = include_str!("fixtures/cli_settle_args.json");
// public halves of the throw-away test keys used to sign the fixture
const ALICE_PUBLIC_KEY: &str = "8a88e3dd7409f195fd52db2d3cba5d72ca6709bf1d94121bf3748801b40f6f5c";
const BOB_PUBLIC_KEY: &str = "8139770ea87d175f56a35466c34c7ecccb8d8a91b4ee37a25df60f5b8fc9b394";

fn id(s: &str) -> AccountId {
    s.parse().expect("account id")
}

fn call_as(predecessor: &str) {
    let mut ctx = VMContextBuilder::new();
    ctx.current_account_id(id("escrow.testnet"));
    ctx.predecessor_account_id(id(predecessor));
    testing_env!(ctx.build());
}

#[test]
fn settle_accepts_orders_made_by_terra_keys() {
    let args: Value = serde_json::from_str(FIXTURE).expect("fixture is JSON");
    let buy: SignedOrder = serde_json::from_value(args["buy"].clone()).expect("buy");
    let sell: SignedOrder = serde_json::from_value(args["sell"].clone()).expect("sell");
    let fill_lots = args["fill_lots"]
        .as_u64()
        .expect("fill_lots is a JSON number");
    let price_ticks = args["price_ticks"]
        .as_u64()
        .expect("price_ticks is a JSON number");
    assert_eq!((fill_lots, price_ticks), (3, 95));

    call_as("escrow.testnet");
    let mut c = Escrow::new();

    // deposits: alice holds quote, bob holds base
    call_as("quote.testnet");
    c.ft_on_transfer(id("alice.testnet"), U128(1_000_000_000), String::new());
    call_as("base.testnet");
    c.ft_on_transfer(id("bob.testnet"), U128(100_000_000), String::new());

    call_as("alice.testnet");
    c.register_key(ALICE_PUBLIC_KEY.to_string());
    call_as("bob.testnet");
    c.register_key(BOB_PUBLIC_KEY.to_string());

    // anyone may settle
    call_as("matcher.testnet");
    c.settle(buy, sell, fill_lots, price_ticks);

    let base_amount = 3 * 1_000_000u128;
    let quote_amount = 3 * 95 * 1_000u128;
    let bal = |c: &Escrow, who: &str, token: &str| c.balance_of(id(who), id(token)).0;
    assert_eq!(bal(&c, "alice.testnet", "base.testnet"), base_amount);
    assert_eq!(
        bal(&c, "alice.testnet", "quote.testnet"),
        1_000_000_000 - quote_amount
    );
    assert_eq!(bal(&c, "bob.testnet", "quote.testnet"), quote_amount);
    assert_eq!(
        bal(&c, "bob.testnet", "base.testnet"),
        100_000_000 - base_amount
    );
}
