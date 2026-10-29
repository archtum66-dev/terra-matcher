#![forbid(unsafe_code)]
//! Mintable NEP-141 test token for the Terra escrow testnet run. It has no value.
//! The token account itself is the only minter (`mint` needs `predecessor == current_account_id`).

use near_contract_standards::fungible_token::events::FtMint;
use near_contract_standards::fungible_token::metadata::{
    FungibleTokenMetadata, FungibleTokenMetadataProvider, FT_METADATA_SPEC,
};
use near_contract_standards::fungible_token::{
    FungibleToken, FungibleTokenCore, FungibleTokenResolver,
};
use near_contract_standards::storage_management::{
    StorageBalance, StorageBalanceBounds, StorageManagement,
};
use near_sdk::borsh::BorshSerialize;
use near_sdk::collections::LazyOption;
use near_sdk::json_types::U128;
use near_sdk::{
    env, near, require, AccountId, BorshStorageKey, NearToken, PanicOnDefault, PromiseOrValue,
};

#[cfg(test)]
const STORAGE_DEPOSIT_MIN: NearToken = NearToken::from_yoctonear(1250000000000000000000);

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct Contract {
    token: FungibleToken,
    metadata: LazyOption<FungibleTokenMetadata>,
}

#[derive(BorshSerialize, BorshStorageKey)]
#[borsh(crate = "near_sdk::borsh")]
enum StorageKey {
    FungibleToken,
    Metadata,
}

#[near]
impl Contract {
    #[init]
    pub fn new(symbol: String) -> Self {
        let metadata = FungibleTokenMetadata {
            spec: FT_METADATA_SPEC.to_string(),
            name: "Terra test token (no value)".to_string(),
            symbol,
            icon: None,
            reference: None,
            reference_hash: None,
            decimals: 6,
        };
        Self {
            token: FungibleToken::new(StorageKey::FungibleToken),
            metadata: LazyOption::new(StorageKey::Metadata, Some(&metadata)),
        }
    }

    /// Mints `amount` to `account_id`. Only the token account itself may call this.
    /// Registers the receiver on first use (the token account pays the storage).
    pub fn mint(&mut self, account_id: AccountId, amount: U128) {
        require!(
            env::predecessor_account_id() == env::current_account_id(),
            "only_owner"
        );
        if self.token.accounts.get(&account_id).is_none() {
            self.token.internal_register_account(&account_id);
        }
        self.token.internal_deposit(&account_id, amount.0);
        FtMint {
            owner_id: &account_id,
            amount,
            memo: Some("mint"),
        }
        .emit();
    }
}

// Written out because `impl_fungible_token_core!` and `impl_fungible_token_storage!` are deprecated
// or broken in near-contract-standards 5.30 (`storage_withdraw` takes `U128` in the macro and
// `NearToken` in the trait).
#[near]
impl FungibleTokenCore for Contract {
    #[payable]
    fn ft_transfer(&mut self, receiver_id: AccountId, amount: U128, memo: Option<String>) {
        self.token.ft_transfer(receiver_id, amount, memo)
    }

    #[payable]
    fn ft_transfer_call(
        &mut self,
        receiver_id: AccountId,
        amount: U128,
        memo: Option<String>,
        msg: String,
    ) -> PromiseOrValue<U128> {
        self.token.ft_transfer_call(receiver_id, amount, memo, msg)
    }

    fn ft_total_supply(&self) -> U128 {
        self.token.ft_total_supply()
    }

    fn ft_balance_of(&self, account_id: AccountId) -> U128 {
        self.token.ft_balance_of(account_id)
    }
}

#[near]
impl FungibleTokenResolver for Contract {
    #[private]
    fn ft_resolve_transfer(
        &mut self,
        sender_id: AccountId,
        receiver_id: AccountId,
        amount: U128,
    ) -> U128 {
        let (used_amount, _burned) =
            self.token
                .internal_ft_resolve_transfer(&sender_id, receiver_id, amount);
        used_amount.into()
    }
}

#[near]
impl StorageManagement for Contract {
    #[payable]
    fn storage_deposit(
        &mut self,
        account_id: Option<AccountId>,
        registration_only: Option<bool>,
    ) -> StorageBalance {
        self.token.storage_deposit(account_id, registration_only)
    }

    #[payable]
    fn storage_withdraw(&mut self, amount: Option<NearToken>) -> StorageBalance {
        self.token.storage_withdraw(amount)
    }

    #[payable]
    fn storage_unregister(&mut self, force: Option<bool>) -> bool {
        self.token.internal_storage_unregister(force).is_some()
    }

    fn storage_balance_bounds(&self) -> StorageBalanceBounds {
        self.token.storage_balance_bounds()
    }

    fn storage_balance_of(&self, account_id: AccountId) -> Option<StorageBalance> {
        self.token.storage_balance_of(account_id)
    }
}

#[near]
impl FungibleTokenMetadataProvider for Contract {
    fn ft_metadata(&self) -> FungibleTokenMetadata {
        match self.metadata.get() {
            Some(m) => m,
            None => env::panic_str("metadata_missing"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use near_contract_standards::storage_management::StorageManagement;
    use near_sdk::test_utils::{accounts, VMContextBuilder};
    use near_sdk::testing_env;
    use near_sdk::NearToken;

    fn get_context(predecessor: AccountId) -> VMContextBuilder {
        let mut builder = VMContextBuilder::new();
        builder
            .current_account_id(accounts(0))
            .signer_account_id(predecessor.clone())
            .predecessor_account_id(predecessor);
        builder
    }

    #[test]
    fn new_sets_symbol_and_decimals() {
        let context = get_context(accounts(0));
        testing_env!(context.build());
        let contract = Contract::new("TBASE".to_string());
        let meta = contract.ft_metadata();
        assert_eq!(meta.symbol, "TBASE");
        assert_eq!(meta.decimals, 6);
        assert_eq!(meta.name, "Terra test token (no value)");
    }

    #[test]
    fn mint_by_contract_account_credits_and_increases_supply() {
        let token_id = accounts(0);
        let alice = accounts(1);
        let mut context = get_context(token_id.clone());
        testing_env!(context.build());
        let mut contract = Contract::new("TBASE".to_string());
        testing_env!(context.predecessor_account_id(token_id.clone()).build());
        contract.mint(alice.clone(), U128(500));
        assert_eq!(contract.ft_balance_of(alice).0, 500);
        assert_eq!(contract.ft_total_supply().0, 500);
    }

    #[test]
    fn mint_twice_and_after_registration_does_not_panic() {
        let token_id = accounts(0);
        let alice = accounts(1);
        let mut context = get_context(token_id.clone());
        context.attached_deposit(STORAGE_DEPOSIT_MIN);
        testing_env!(context.build());
        let mut contract = Contract::new("TQUOTE".to_string());
        // alice registers herself first, then the token account mints twice
        testing_env!(context.predecessor_account_id(alice.clone()).build());
        contract.storage_deposit(None, Some(true));
        testing_env!(context.predecessor_account_id(token_id).build());
        contract.mint(alice.clone(), U128(10));
        contract.mint(alice.clone(), U128(5));
        assert_eq!(contract.ft_balance_of(alice).0, 15);
        assert_eq!(contract.ft_total_supply().0, 15);
    }

    #[test]
    #[should_panic(expected = "only_owner")]
    fn mint_by_other_account_panics() {
        let token_id = accounts(0);
        let alice = accounts(1);
        let bob = accounts(2);
        let mut context = get_context(token_id.clone());
        testing_env!(context.build());
        let mut contract = Contract::new("TBASE".to_string());
        testing_env!(context.predecessor_account_id(bob).build());
        contract.mint(alice, U128(1));
    }

    #[test]
    fn transfer_after_mint_and_storage_deposit() {
        let token_id = accounts(0);
        let alice = accounts(1);
        let bob = accounts(2);
        let mut context = get_context(token_id.clone());
        context.attached_deposit(STORAGE_DEPOSIT_MIN);
        testing_env!(context.build());
        let mut contract = Contract::new("TBASE".to_string());
        testing_env!(context.predecessor_account_id(token_id.clone()).build());
        contract.mint(alice.clone(), U128(1000));
        testing_env!(context
            .predecessor_account_id(bob.clone())
            .attached_deposit(STORAGE_DEPOSIT_MIN)
            .build());
        contract.storage_deposit(None, None);
        testing_env!(context
            .predecessor_account_id(alice.clone())
            .attached_deposit(NearToken::from_yoctonear(1))
            .build());
        contract.ft_transfer(bob.clone(), U128(100), None);
        assert_eq!(contract.ft_balance_of(alice).0, 900);
        assert_eq!(contract.ft_balance_of(bob).0, 100);
    }
}
