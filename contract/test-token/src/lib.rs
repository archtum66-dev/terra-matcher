#![forbid(unsafe_code)]

use near_contract_standards::fungible_token::events::FtMint;
use near_contract_standards::fungible_token::metadata::{
    FungibleTokenMetadata, FungibleTokenMetadataProvider, FT_METADATA_SPEC,
};
use near_contract_standards::fungible_token::FungibleToken;
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::LazyOption;
use near_sdk::json_types::U128;
use near_sdk::{
    env, near, require, AccountId, BorshStorageKey, NearToken, PanicOnDefault,
};

const STORAGE_DEPOSIT_MIN: NearToken = NearToken::from_yoctonear(1250000000000000000000);

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct Contract {
    token: FungibleToken,
    metadata: LazyOption<FungibleTokenMetadata>,
}

#[derive(BorshSerialize, BorshDeserialize, BorshStorageKey)]
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

    pub fn mint(&mut self, account_id: AccountId, amount: U128) {
        require!(
            env::predecessor_account_id() == env::current_account_id(),
            "only_owner"
        );
        self.token.internal_register_account(&account_id);
        let amt = amount.0;
        self.token.internal_deposit(&account_id, amt);
        FtMint {
            owner_id: &account_id,
            amount,
            memo: Some("mint"),
        }
        .emit();
    }
}

near_contract_standards::impl_fungible_token_core!(Contract, token);
near_contract_standards::impl_fungible_token_storage!(Contract, token);

#[near]
impl FungibleTokenMetadataProvider for Contract {
    fn ft_metadata(&self) -> FungibleTokenMetadata {
        self.metadata.get().cloned().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use near_contract_standards::fungible_token::FungibleTokenCore;
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
        testing_env!(
            context
                .predecessor_account_id(bob.clone())
                .attached_deposit(STORAGE_DEPOSIT_MIN)
                .build()
        );
        contract.storage_deposit(None, None);
        testing_env!(
            context
                .predecessor_account_id(alice.clone())
                .attached_deposit(NearToken::from_yoctonear(1))
                .build()
        );
        contract.ft_transfer(bob.clone(), U128(100), None);
        assert_eq!(contract.ft_balance_of(alice).0, 900);
        assert_eq!(contract.ft_balance_of(bob).0, 100);
    }
}
