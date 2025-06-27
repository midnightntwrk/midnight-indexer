use crate::domain::{
    ContractBalance, NetworkId, PROTOCOL_VERSION_000_013_000, ProtocolVersion, RawTokenType,
    ledger::{Error, NetworkIdExt, SerializableV5Ext},
};
use fastrace::trace;
use midnight_coin_structure::coin::TokenType as TokenTypeV5;
use midnight_onchain_runtime::state::ContractState as ContractStateV5;
use midnight_serialize::deserialize as deserialize_v5;
use midnight_storage::{DefaultDB as DefaultDBV5, arena::Sp as SpV5};

/// Facade for `ContractState` from `midnight_ledger` across supported (protocol) versions.
#[derive(Debug, Clone)]
pub enum ContractState {
    V5(ContractStateV5<DefaultDBV5>),
}

impl ContractState {
    /// Deserialize the given raw contract state using the given protocol version and network ID.
    #[trace(properties = {
        "network_id": "{network_id}",
        "protocol_version": "{protocol_version}"
    })]
    pub fn deserialize(
        contract_state: impl AsRef<[u8]>,
        network_id: NetworkId,
        protocol_version: ProtocolVersion,
    ) -> Result<Self, Error> {
        if protocol_version.is_compatible(PROTOCOL_VERSION_000_013_000) {
            let contract_state =
                deserialize_v5(&mut contract_state.as_ref(), network_id.into_ledger_v5())
                    .map_err(|error| Error::Io("cannot deserialize ContractStateV5", error))?;
            Ok(Self::V5(contract_state))
        } else {
            Err(Error::InvalidProtocolVersion(protocol_version))
        }
    }

    /// Get the token balances for this contract.
    pub fn balances(&self, network_id: NetworkId) -> Result<Vec<ContractBalance>, Error> {
        match self {
            ContractState::V5(contract_state) => {
                contract_state
                    .balance
                    .iter()
                    .filter_map(|entry| {
                        let (token_type_sp, amount_sp) = SpV5::into_inner(entry)?;
                        let token_type = SpV5::into_inner(token_type_sp)?;
                        let amount = SpV5::into_inner(amount_sp)?;

                        (amount > 0).then_some((token_type, amount))
                    })
                    .map(|(token_type, amount)| {
                        match token_type {
                            // For unshielded tokens extract the type directly.
                            TokenTypeV5::Unshielded(unshielded) => Ok(ContractBalance {
                                token_type: unshielded.0.0.into(),
                                amount,
                            }),

                            // For other tokens we serialize the type.
                            _ => {
                                let token_type =
                                    token_type.serialize(network_id).map_err(|error| {
                                        Error::Io("cannot serialize TokenTypeV5", error)
                                    })?;

                                let len = token_type.len();
                                let token_type = RawTokenType::try_from(token_type)
                                    .map_err(|_| Error::TokenTypeLen(len))?;

                                Ok(ContractBalance { token_type, amount })
                            }
                        }
                    })
                    .collect()
            }
        }
    }
}
