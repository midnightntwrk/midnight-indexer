#[derive(Debug, Clone)]
pub struct Validator {
    pub epoch_no: u64,
    pub position: u64,
    pub sidechain_pubkey: String,
}

#[derive(Debug, Clone)]
pub struct ValidatorMembership {
    pub spo_sk: String,
    pub sidechain_pubkey: String,
    pub epoch_no: u64,
    pub position: u64,
    pub expected_slots: u32,
}
