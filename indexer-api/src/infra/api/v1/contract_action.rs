use crate::{
    domain::{self, AsBytesExt, HexEncoded, storage::Storage},
    infra::api::{
        ContextExt, ResultExt,
        v1::{
            block::BlockOffset,
            transaction::{Transaction, TransactionOffset},
            unshielded::ContractBalance,
        },
    },
};
use async_graphql::{ComplexObject, Context, Interface, OneofObject, SimpleObject};
use derive_more::Debug;
use indexer_common::{domain::ByteVec, error::NotFoundError};
use std::marker::PhantomData;

/// A contract action.
#[derive(Debug, Clone, Interface)]
#[allow(clippy::duplicated_attributes)]
#[graphql(
    field(name = "address", ty = "HexEncoded"),
    field(name = "state", ty = "HexEncoded"),
    field(name = "chain_state", ty = "HexEncoded"),
    field(name = "transaction", ty = "Transaction<S>")
)]
pub enum ContractAction<S: Storage> {
    /// A contract deployment.
    Deploy(ContractDeploy<S>),

    /// A contract call.
    Call(ContractCall<S>),

    /// A contract update.
    Update(ContractUpdate<S>),
}

impl<S> From<domain::ContractAction> for ContractAction<S>
where
    S: Storage,
{
    fn from(action: domain::ContractAction) -> Self {
        let domain::ContractAction {
            id,
            address,
            state,
            attributes,
            zswap_state,
            transaction_id,
            ..
        } = action;

        match attributes {
            domain::ContractAttributes::Deploy => ContractAction::Deploy(ContractDeploy {
                address: address.hex_encode(),
                state: state.hex_encode(),
                chain_state: zswap_state.hex_encode(),
                transaction_id,
                contract_action_id: id,
                _s: PhantomData,
            }),

            domain::ContractAttributes::Call { entry_point } => {
                ContractAction::Call(ContractCall {
                    address: address.hex_encode(),
                    state: state.hex_encode(),
                    entry_point: entry_point.hex_encode(),
                    chain_state: zswap_state.hex_encode(),
                    transaction_id,
                    contract_action_id: id,
                    raw_address: address,
                    _s: PhantomData,
                })
            }

            domain::ContractAttributes::Update => ContractAction::Update(ContractUpdate {
                address: address.hex_encode(),
                state: state.hex_encode(),
                chain_state: zswap_state.hex_encode(),
                transaction_id,
                contract_action_id: id,
                _s: PhantomData,
            }),
        }
    }
}

/// A contract deployment.
#[derive(Debug, Clone, SimpleObject)]
#[graphql(complex)]
pub struct ContractDeploy<S: Storage> {
    address: HexEncoded,

    state: HexEncoded,

    chain_state: HexEncoded,

    #[graphql(skip)]
    transaction_id: u64,

    #[graphql(skip)]
    contract_action_id: u64,

    #[graphql(skip)]
    _s: PhantomData<S>,
}

#[ComplexObject]
impl<S: Storage> ContractDeploy<S> {
    async fn transaction(&self, cx: &Context<'_>) -> async_graphql::Result<Transaction<S>> {
        get_transaction_by_id(self.transaction_id, cx).await
    }

    /// Unshielded token balances held by this contract.
    /// According to the architecture, deployed contracts must have zero balance.
    async fn unshielded_balances(
        &self,
        cx: &Context<'_>,
    ) -> async_graphql::Result<Vec<ContractBalance>> {
        let storage = cx.get_storage::<S>();
        let balances = storage
            .get_unshielded_balances_by_action_id(self.contract_action_id)
            .await
            .internal("get contract balances by action id")?;

        Ok(balances.into_iter().map(Into::into).collect())
    }
}

/// A contract call.
#[derive(Debug, Clone, SimpleObject)]
#[graphql(complex)]
pub struct ContractCall<S: Storage> {
    address: HexEncoded,

    state: HexEncoded,

    chain_state: HexEncoded,

    entry_point: HexEncoded,

    #[graphql(skip)]
    transaction_id: u64,

    #[graphql(skip)]
    contract_action_id: u64,

    #[graphql(skip)]
    raw_address: ByteVec,

    #[graphql(skip)]
    _s: PhantomData<S>,
}

#[ComplexObject]
impl<S: Storage> ContractCall<S> {
    async fn transaction(&self, cx: &Context<'_>) -> async_graphql::Result<Transaction<S>> {
        get_transaction_by_id(self.transaction_id, cx).await
    }

    async fn deploy(&self, cx: &Context<'_>) -> async_graphql::Result<ContractDeploy<S>> {
        let action = cx
            .get_storage::<S>()
            .get_contract_deploy_by_address(&self.raw_address)
            .await
            .internal("cannot get contract deploy by address")?
            .expect("contract call has contract deploy");

        let deploy = match ContractAction::from(action) {
            ContractAction::Deploy(deploy) => deploy,
            _ => panic!("unexpected contract action"),
        };

        Ok(deploy)
    }

    /// Unshielded token balances held by this contract.
    async fn unshielded_balances(
        &self,
        cx: &Context<'_>,
    ) -> async_graphql::Result<Vec<ContractBalance>> {
        let storage = cx.get_storage::<S>();
        let balances = storage
            .get_unshielded_balances_by_action_id(self.contract_action_id)
            .await
            .internal("get contract balances by action id")?;

        Ok(balances.into_iter().map(Into::into).collect())
    }
}

/// A contract update.
#[derive(Debug, Clone, SimpleObject)]
#[graphql(complex)]
pub struct ContractUpdate<S: Storage> {
    address: HexEncoded,

    state: HexEncoded,

    chain_state: HexEncoded,

    #[graphql(skip)]
    transaction_id: u64,

    #[graphql(skip)]
    contract_action_id: u64,

    #[graphql(skip)]
    _s: PhantomData<S>,
}

#[ComplexObject]
impl<S: Storage> ContractUpdate<S> {
    async fn transaction(&self, cx: &Context<'_>) -> async_graphql::Result<Transaction<S>> {
        get_transaction_by_id(self.transaction_id, cx).await
    }

    /// Unshielded token balances held by this contract after the update.
    async fn unshielded_balances(
        &self,
        cx: &Context<'_>,
    ) -> async_graphql::Result<Vec<ContractBalance>> {
        let storage = cx.get_storage::<S>();
        let balances = storage
            .get_unshielded_balances_by_action_id(self.contract_action_id)
            .await
            .internal("get contract balances by action id")?;

        Ok(balances.into_iter().map(Into::into).collect())
    }
}

/// Either a block offset or a transaction offset.
#[derive(Debug, OneofObject)]
pub enum ContractActionOffset {
    /// Either a block hash or a block height.
    BlockOffset(BlockOffset),

    /// Either a transaction hash or a transaction identifier.
    TransactionOffset(TransactionOffset),
}

async fn get_transaction_by_id<S>(
    id: u64,
    cx: &Context<'_>,
) -> async_graphql::Result<Transaction<S>>
where
    S: Storage,
{
    let transaction = cx
        .get_storage::<S>()
        .get_transaction_by_id(id)
        .await
        .internal("cannot get transaction by ID")?
        .ok_or_else(|| NotFoundError(format!("transaction with ID {id}")))
        .internal("cannot get transaction by ID")?;

    Ok(transaction.into())
}
