// Thin shim over the canonical QA environment module.
// Keep all env names / network IDs in one place (qa/tests/environment/model.ts).

import {
  networkIdByEnvName,
  // EnvironmentName, // available if you need the enum elsewhere
} from "../../../environment/model.ts";

/**
 * Union of supported chain IDs:
 * "undeployed" | "qanet" | "nodedev01" | "devnet" | "testnet" | "testnet02"
 */
export type ChainId = keyof typeof networkIdByEnvName;

/** Resolve Midnight network label ("Undeployed" | "Devnet" | "Testnet") from a chain id. */
export function getNetworkId(chain: ChainId): string {
  return networkIdByEnvName[chain];
}