export const CONTRACT_ACTION_LIGHT_BODY = `
__typename
address
... on ContractDeploy {
    unshieldedBalances {
        tokenType
        amount
    }
}
... on ContractUpdate {
    unshieldedBalances {
        tokenType
        amount
    }
}
... on ContractCall {
    deploy {
        address
        unshieldedBalances {  
            tokenType
            amount
        }
    }
    entryPoint
    unshieldedBalances {
        tokenType
        amount
    }
}`;

export const GET_CONTRACT_ACTION = `query 
GetContractAction($ADDRESS: String!) {
    contractAction(address: $ADDRESS) {
        ${CONTRACT_ACTION_LIGHT_BODY}
    }
}
`;

export const GET_CONTRACT_ACTION_BY_OFFSET = `query 
GetContractActionByOffset($ADDRESS: String!, $OFFSET: Int) {
    contractAction(address: $ADDRESS, offset: $OFFSET) {
        ${CONTRACT_ACTION_LIGHT_BODY}
    }
}
`;
