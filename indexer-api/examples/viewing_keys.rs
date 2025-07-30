use indexer_api::infra::api::v1::viewing_key::ViewingKey;
use indexer_common::domain::{ByteArray, NetworkId, PROTOCOL_VERSION_000_013_000};

/// Print the Bech32m-encoded viewing keys and their session IDs for the prefunded wallets (root
/// seeds) for all network IDs except MainNet.
fn main() {
    print_viewing_keys(NetworkId::Undeployed);
    print_viewing_keys(NetworkId::DevNet);
    print_viewing_keys(NetworkId::TestNet);
}

fn print_viewing_keys(network_id: NetworkId) {
    println!("# {network_id}");

    for n in 1..=4 {
        let viewing_key =
            ViewingKey::derive_for_testing(seed(n), network_id, PROTOCOL_VERSION_000_013_000);
        println!("  {n:02}:\n    {viewing_key}");

        let session_id = viewing_key
            .try_into_domain(network_id, PROTOCOL_VERSION_000_013_000)
            .unwrap()
            .to_session_id();
        println!("    {session_id}");
    }
}

fn seed(n: u8) -> ByteArray<32> {
    let mut seed = [0; 32];
    seed[31] = n;
    seed.into()
}
