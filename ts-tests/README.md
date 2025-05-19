# Configuration

The tests use node/jest/yarn to run, so make sure to have these setup

They will need `ENV_TYPE` environment variable to be set with the desired target environmet which could be
- `compose` - for local execution using docker containers managed in compose files
- `nodedev01` - used for early development (not really so much for testing)
- `qanet` - this is the main internal environment for testing before we release in the public testnet
- `testnet` - previous testnet, deprecated because it reached poor performance levels
- `testnet02` - current testnet

Also the `DEPLOYMENT` environment vailable is needed. It represent the topology of the indexer in terms of docker containers. It needs to be set to either of these 2:
- `standalone` - only 1 single indexer container (called `indexer-standalone`)
- `cloud` - a set of 3 docker containers used in cloud env that are able to scale (these are called `wallet-indexer`, `chain-indexer` and `indexer-api`)

For more details look at this file `ts-tests/environment/envConfig.ts`

As well as the `ENV_TYPE` and `DEPLOYMENT` you will need to have some other environment variables to be able to use the docker compose files. 
These would be typically provided through a `.midnight-indexer.envrc` file located in your home folder (`~/.midnight-indexer.envrc`). 
The required variables are:

- `POSTGRES_PASSWORD`
- `APP__INFRA__STORAGE__PASSWORD`
- `APP__INFRA__PUB_SUB__PASSWORD`
- `APP__INFRA__ZSWAP_STATE_STORAGE__PASSWORD`
- `APP__INFRA__SECRET`

Note that for security reasons, being all secrets, none of the values are provided in the repo. Please make sure to ask an indexer team member for these and add them to your envrc file

# Test Execution

For local execution please see the available scripts in `ts-tests/package.json` and invoke them with `yarn <script>`. Once the above variables have been setup you should be able to execute any of them fron your local environment.