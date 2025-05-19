export const environments: Environments = {
  compose: {
    indexer_http_url: 'http://localhost', // This is set in TestContainersComposeEnvironment.getUrl
    indexer_ws_url: 'ws://localhost', // This is set in TestContainersComposeEnvironment.getWsUrl
    substrate_node_ws_url: 'ws://node:9944',
    ledger_network_id: 'Undeployed',
    wallets: [
      {
        viewingKey:
          '030020c6275c3e5cfb56e91fa09004e69f0e67e471160e18b87d0fb05f7a8d1f0ddf02',
        viewingKeyBech32m:
          'mn_shield-esk_undeployed1qvqzp338tsl9e76kay06pyqyu60suelywytqux9c058mqhm6350smhczah53pj',
        address:
          'mn_shield-addr_undeployed1mjngjmnlutcq50trhcsk3hugvt9wyjnhq3c7prryd5nqmvtzva0sxqpvzkdy4k9u7eyffff53cge62tqylevq3wqps86tdjuahsquwvucsy9kffv',
        addressHex: "dca6896e7fe2f00a3d63be2168df8862cae24a770471e08c646d260db162675f03002c159a4ad8bcf64894a5348e119d296027f2c045c00c0fa5b65cede00e399cc4",
        seed: '0000000000000000000000000000000000000000000000000000000000000001',
        mnemonic: [
          'abandon',
          'abandon',
          'abandon',
          'abandon',
          'abandon',
          'abandon',
          'abandon',
          'abandon',
          'abandon',
          'abandon',
          'abandon',
          'abandon',
          'abandon',
          'abandon',
          'abandon',
          'abandon',
          'abandon',
          'abandon',
          'abandon',
          'abandon',
          'abandon',
          'abandon',
          'abandon',
          'diesel',
        ],
      },
      {
        viewingKey:
          '0300200cf5060d0165bbcd1b627432b5ec6a8ee84e826dcba638fa716298329bf88407',
        viewingKeyBech32m:
          'mn_shield-esk_undeployed1qvqzqr84qcxszedme5dkyapjkhkx4rhgf6pxmjax8ra8zc5cx2dl3pq8e0xp3m',
        address:
          'mn_shield-addr_undeployed1rx4cxy434g50r2sdlqjdnp4j6pe5y9x0taqlc6hkgufzlnxlv9uqxq873xnampxfvkrfqev8lrjkr4zarukfjwygxr6ry886k7kg3f42avhg83mk',
        addressHex: "19ab8312b1aa28f1aa0df824d986b2d0734214cf5f41fc6af647122fccdf61780300fe89a7dd84c96586906587f8e561d45d1f2c99388830f4321cfab7ac88a6aaeb",
        seed: '0000000000000000000000000000000000000000000000000000000000000002',
        mnemonic: [],
      },
      {
        viewingKey:
          '030020499414da48ec9ed37804f84dc2dbc668922d1ed8b7db60540ec882881d12a005',
        viewingKeyBech32m:
          'mn_shield-esk_undeployed1qvqzqjv5zndy3my76duqf7zdctduv6yj950d3d7mvp2qajyz3qw39gq9h62nt8',
        address:
          'mn_shield-addr_undeployed14yqrpse7mr484yz0dsyrdv43reyrzpu7y0342m6xj7xze70lphvsxqzvhuhmjmeqvnghtt0mm6dj7a4j3yq3pvzd5atgexlwc4k8rnyvus6t6afs',
        addressHex: "a90030c33ed8ea7a904f6c0836b2b11e4831079e23e3556f46978c2cf9ff0dd903004cbf2fb96f2064d175adfbde9b2f76b2890110b04da7568c9beec56c71cc8ce4",
        seed: '0000000000000000000000000000000000000000000000000000000000000003',
        mnemonic: [],
      },
      {
        viewingKey:
          '0300209d1dcd1c2d229d991cd8d6e1b3d8b1643be7f570694371dc3215939d6b4a6804',
        viewingKeyBech32m:
          'mn_shield-esk_undeployed1qvqzp8gae5wz6g5anywd34hpk0vtzepmul6hq62rw8wry9vnn44556qyju7y0g',
        address:
          'mn_shield-addr_undeployed1d60ldagxe0jhy0exyqg0n6utvtrl98lk8yqzn57q2e77dfkddepsxqp3d7d8dzjdg7tt9z3s4dqq8vr4kuux533gjyfye5j4am7w6ps2fvgxf7nc',
        addressHex: "6e9ff6f506cbe5723f262010f9eb8b62c7f29ff6390029d3c0567de6a6cd6e430300316f9a768a4d4796b28a30ab4003b075b7386a462891124cd255eefced060a4b",
        seed: '0000000000000000000000000000000000000000000000000000000000000004',
        mnemonic: [],
      },
    ],
  },
  nodedev01: {
    indexer_http_url: 'https://indexer-rs.node-dev-01.dev.midnight.network',
    indexer_ws_url: 'wss://indexer-rs.node-dev-01.dev.midnight.network',
    substrate_node_ws_url: 'wss://rpc.node-dev-01.dev.midnight.network',
    ledger_network_id: 'DevNet',
    wallets: [
      {
        viewingKey:
          '030020220dc923d4dfae698d9f3d153ba7f34f5badfe84e0c5c49cfc4d441e472ccb02',
        viewingKeyBech32m:
          'mn_shield-esk_dev1qvqzqgsdey3afhawdxxe70g48wnlxn6m4hlgfcx9cjw0cn2yrerjejczame9fw',
        address:
          'mn_shield-addr_dev1e4ky3t7t22qs34n07j4ue887kmjwdpppl3tfhg65kuhc8nyq5y6qxqyypfsqurjwhedt3xuz02tpzcyhvujklwl7ydzh8geqyc5xjhx2qqrn7kpp',
        addressHex: "cd6c48afcb528108d66ff4abcc9cfeb6e4e68421fc569ba354b72f83cc80a1340300840a600e0e4ebe5ab89b827a9611609767256fbbfe234573a3202628695cca00",
        seed: '0e9dbec2af2ff4d0cbb3441ddac3bf4c71798ee1c7d255f88f929e01d7d2a107',
        mnemonic: [
          'attend',
          'unknown',
          'radar',
          'furnace',
          'young',
          'half',
          'conduct',
          'hammer',
          'build',
          'stock',
          'used',
          'ocean',
          'bleak',
          'shuffle',
          'mandate',
          'where',
          'field',
          'settle',
          'tool',
          'despair',
          'buddy',
          'true',
          'lottery',
          'toast',
        ],
      },
    ],
  },
  qanet: {
    indexer_http_url: 'https://indexer-rs.qanet.dev.midnight.network',
    indexer_ws_url: 'wss://indexer-rs.qanet.dev.midnight.network',
    substrate_node_ws_url: 'wss://rpc.qanet.dev.midnight.network',
    ledger_network_id: 'DevNet',
    wallets: [
      {
        viewingKey:
          '030020220dc923d4dfae698d9f3d153ba7f34f5badfe84e0c5c49cfc4d441e472ccb02',
        viewingKeyBech32m:
          'mn_shield-esk_dev1qvqzqgsdey3afhawdxxe70g48wnlxn6m4hlgfcx9cjw0cn2yrerjejczame9fw',
        address:
          'mn_shield-addr_dev1e4ky3t7t22qs34n07j4ue887kmjwdpppl3tfhg65kuhc8nyq5y6qxqyypfsqurjwhedt3xuz02tpzcyhvujklwl7ydzh8geqyc5xjhx2qqrn7kpp',
        addressHex: "cd6c48afcb528108d66ff4abcc9cfeb6e4e68421fc569ba354b72f83cc80a1340300840a600e0e4ebe5ab89b827a9611609767256fbbfe234573a3202628695cca00",
        seed: '0e9dbec2af2ff4d0cbb3441ddac3bf4c71798ee1c7d255f88f929e01d7d2a107',
        mnemonic: [
          'attend',
          'unknown',
          'radar',
          'furnace',
          'young',
          'half',
          'conduct',
          'hammer',
          'build',
          'stock',
          'used',
          'ocean',
          'bleak',
          'shuffle',
          'mandate',
          'where',
          'field',
          'settle',
          'tool',
          'despair',
          'buddy',
          'true',
          'lottery',
          'toast',
        ],
      },
    ],
  },
  testnet: {
    indexer_http_url: 'https://indexer.indexer-rs.midnight.network',
    indexer_ws_url: 'wss://indexer.indexer-rs.midnight.network',
    substrate_node_ws_url: 'wss://rpc.testnet.midnight.network',
    ledger_network_id: 'TestNet',
    wallets: [
      {
        viewingKey:
          '020300387d7dea035e87baac2f27048f224d53ea1eedd4800311fa8cc977fd6a7ceb544ccc66cbd7d5a1c8fb7cbb4ea06af3f919234f2def3a0dad19',
        viewingKeyBech32m:
          'mn_shield-esk_test1qvqrs63jmkkgrr0cyrjst62m8080r6e3xyrsjqcv83sghnrup4a9qez776jfdsv8r2crj36pwywllqythdydjepewfq77xg3yc5dy',
        address:
          '0bfad7e7a764228bb4f296376ead2d0e92ddd05eee2260a029c6417969c59fbc|0300fd6797ced9aa6df1703456b0a3c00a8c14f80c22d903121724f653eae1ae3770b21dedc69d132bf3f4da329cf54c663d67ddcc9ee8a3b4a0',
        seed: '9cd62f47b976eb64407cec2152c238a597b91bc64b9b31826ead234e80afb889',
        mnemonic: [
          'orphan',
          'ramp',
          'spin',
          'indicate',
          'huge',
          'rate',
          'acid',
          'outside',
          'candy',
          'noodle',
          'mixed',
          'enroll',
          'knee',
          'mistake',
          'bomb',
          'inflict',
          'cover',
          'beach',
          'prize',
          'educate',
          'trend',
          'fit',
          'timber',
          'desert',
        ],
      },
    ],
  },
  testnet02: {
    indexer_http_url: 'https://indexer-rs.testnet-02.midnight.network',
    indexer_ws_url: 'wss://indexer-rs.testnet-02.midnight.network',
    substrate_node_ws_url: 'wss://rpc.testnet-02.midnight.network',
    ledger_network_id: 'TestNet',
    wallets: [
      {
        viewingKey:
          '020300387d7dea035e87baac2f27048f224d53ea1eedd4800311fa8cc977fd6a7ceb544ccc66cbd7d5a1c8fb7cbb4ea06af3f919234f2def3a0dad19',
        viewingKeyBech32m:
          'mn_shield-esk_test1qvqrs63jmkkgrr0cyrjst62m8080r6e3xyrsjqcv83sghnrup4a9qez776jfdsv8r2crj36pwywllqythdydjepewfq77xg3yc5dy',
        address:
          '0bfad7e7a764228bb4f296376ead2d0e92ddd05eee2260a029c6417969c59fbc|0300fd6797ced9aa6df1703456b0a3c00a8c14f80c22d903121724f653eae1ae3770b21dedc69d132bf3f4da329cf54c663d67ddcc9ee8a3b4a0',
        seed: '9cd62f47b976eb64407cec2152c238a597b91bc64b9b31826ead234e80afb889',
        mnemonic: [
          'orphan',
          'ramp',
          'spin',
          'indicate',
          'huge',
          'rate',
          'acid',
          'outside',
          'candy',
          'noodle',
          'mixed',
          'enroll',
          'knee',
          'mistake',
          'bomb',
          'inflict',
          'cover',
          'beach',
          'prize',
          'educate',
          'trend',
          'fit',
          'timber',
          'desert',
        ],
      },
    ],
  },
};

export interface Wallet {
  viewingKey: string;
  viewingKeyBech32m: string;
  address?: string;
  addressHex?: string;
  seed?: string;
  mnemonic?: string[];
}

export interface Environment {
  indexer_http_url: string;
  indexer_ws_url: string;
  substrate_node_ws_url: string;
  ledger_network_id: string;
  wallets: Wallet[];
}

export interface Environments {
  compose: Environment;
  qanet: Environment;
  testnet: Environment;
  testnet02: Environment;
  nodedev01: Environment;
}
