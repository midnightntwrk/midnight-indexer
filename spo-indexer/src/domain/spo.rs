use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct SPO {
    pub spo_sk: String,
    pub pool_id: String,
    pub mainchain_pubkey: String,
    pub sidechain_pubkey: String,
    pub aura_pubkey: String,
}

#[derive(Debug, Clone)]
pub struct SPOEpochPerformance {
    pub spo_sk: String,
    pub epoch_no: u64,
    pub expected_blocks: u32,
    pub produced_blocks: u64,
    pub identity_label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SPOStatus {
    Valid,

    Invalid,
}

#[derive(Debug, Clone)]
pub struct SPOHistory {
    pub spo_sk: String,
    pub epoch_no: u64,
    pub status: SPOStatus,
}

impl fmt::Display for SPOStatus {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            SPOStatus::Valid => write!(f, "VALID"),
            SPOStatus::Invalid => write!(f, "INVALID"),
        }
    }
}
