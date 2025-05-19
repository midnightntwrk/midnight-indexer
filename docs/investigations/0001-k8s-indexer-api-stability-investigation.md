# Investigating `indexer-api` Stability with Local Kubernetes and entryscript Setup

**Date:** October 25, 2024  
**Author:** Sean Kwak

## Overview

This document outlines a comprehensive investigation into the stability issues observed with the `indexer-api` pod within our Kubernetes cluster. The primary focus was to assess the effectiveness of the existing `entrypoint.sh` script and `Dockerfile` in handling termination signals and ensuring consistent pod operations across various scenarios, including normal starts, forced deletions, and `CrashLoopBackOff` states.

## Table of Contents

- [Steps Undertaken](#steps-undertaken)
    - [1. Local Kubernetes Deployment](#1-local-kubernetes-deployment)
    - [2. Signal Handling Verification](#2-signal-handling-verification)
    - [3. Database Credentials Misconfiguration Test](#3-database-credentials-misconfiguration-test)
    - [4. Force Deletion Test](#4-force-deletion-test)
    - [5. `running` File Verification Across Scenarios](#5-running-file-verification-across-scenarios)
- [Findings](#findings)
    - [Signal Handling](#signal-handling)
    - [`running` File Management](#running-file-management)
    - [`CrashLoopBackOff` Scenarios](#crashloopbackoff-scenarios)
    - [Liveness Probe](#liveness-probe)
- [Conclusion](#conclusion)
- [Recommendations](#recommendations)
- [Supporting Evidence](#supporting-evidence)
    - [Logs Demonstrating `running` File Management](#logs-demonstrating-running-file-management)
    - [`running` File Verification](#running-file-verification)
- [Notes](#notes)
- [Commands Used](#commands-used)

---

## Steps Undertaken

### 1. Local Kubernetes Deployment

- **Deployment:** Deployed the `indexer-api` using the main branch's `Dockerfile` and original `entrypoint.sh` script in a standalone Kubernetes environment.
- **Verification:** Confirmed that the application started successfully with the `running` file present in `/var/run/indexer-api/`.

### 2. Signal Handling Verification

- **Pod Termination:** Executed `kubectl delete pod indexer-api-[ID]` to simulate pod termination.
- **Logs:** Observed the `Trap executed` message in the logs, indicating that the `EXIT` trap was triggered correctly.
- **Cleanup Confirmation:** Verified that the `running` file was removed upon pod termination and recreated in the new pod instance.

### 3. Database Credentials Misconfiguration Test

- **Altered Credentials:** Changed the environment variable `APP__STORAGE__USER` from `postgres` to `wronguser`.
- **Deployment Update:** Applied the updated Kubernetes manifests.
- **Result:** Observed the pod entering `CrashLoopBackOff` due to authentication failures, as expected.
- **Reversion:** Restored the correct database credentials and confirmed that the pod returned to a healthy `Running` state without further restarts.

### 4. Force Deletion Test

- **Forced Pod Deletion:** Executed `kubectl delete pod indexer-api-[ID] --grace-period=0 --force` to simulate an abrupt termination.
- **Logs:** Confirmed that the `Trap executed` message appeared in the logs, indicating that the `EXIT` trap was triggered even during force deletions.
- **Cleanup Confirmation:** Verified that the `running` file was removed upon force deletion and recreated in the new pod instance.

### 5. `running` File Verification Across Scenarios

- **Normal Start:**
    - **Action:** Deployed the pod normally.
    - **Observation:** The `running` file was successfully created (`ls -l /var/run/indexer-api/running` returned a file).

- **Forced Deletion:**
    - **Action:** Force-deleted the pod.
    - **Observation:** Upon recreation, the `running` file was recreated, indicating successful cleanup from the previous instance.

- **`CrashLoopBackOff`:**
    - **Action:** Introduced incorrect database credentials to trigger application crashes.
    - **Observation:** The `running` file was removed upon each crash, and a new pod instance successfully recreated the `running` file upon restart.

## Findings

### Signal Handling

- The original `entrypoint.sh` script effectively handles termination signals via the `EXIT` trap.
- Upon both normal and force pod deletions, the `Trap executed` message confirms that the cleanup function runs as intended.
- The inclusion of `SIGINT` and `SIGTERM` in the trap was found to be redundant since the `EXIT` trap captures all exit scenarios, including those triggered by signals.

### `running` File Management

- The `running` file is consistently created during pod startup and removed upon termination, regardless of how the pod is terminated.
- Each pod instance manages its own `running` file, ensuring no residual files persist across pod restarts.

### `CrashLoopBackOff` Scenarios

- Application crashes due to misconfigurations (e.g., wrong database credentials) lead to pod restarts as expected.
- The `entrypoint.sh` script ensures that the `running` file is cleaned up upon each crash, preventing false positives in liveness probes.

### Liveness Probe

- No liveness probe failures were observed during successful deployments.
- The `running` file is reliably managed, ensuring the liveness probe accurately reflects the pod's health.

## Conclusion

The investigation confirms that the existing `entrypoint.sh` script and `Dockerfile` are functioning correctly in handling termination signals and managing the `running` file across various pod lifecycle events. The `EXIT` trap is sufficient for cleanup without explicitly trapping `SIGINT` and `SIGTERM`, as it effectively captures all exit scenarios, including those initiated by signals.

## Recommendations

### Maintain Current Entrypoint Configuration

- The existing `entrypoint.sh` script adequately handles signal trapping and resource cleanup.
- No further modifications are necessary for signal handling unless additional requirements emerge.

## Supporting Evidence

### Logs Demonstrating `running` File Management

#### Normal Start:

```plaintext
Starting entrypoint.sh
Trap set for EXIT
Created /var/run/indexer-api/running
{"timestamp":"2024-10-25T15:05:25.481620Z","level":"INFO","message":"starting","config":"Config { ... }","target":"indexer_api"}
{"timestamp":"2024-10-25T15:05:25.482399Z","level":"WARN","message":"Failed to open `.pgpass` file: ...","path":"/nonexistent/.pgpass","target":"sqlx_postgres::options::pgpass"}
{"timestamp":"2024-10-25T15:05:25.529178Z","level":"DEBUG","message":"created pool","pool":"PostgresPool(Pool { ... })","target":"indexer_common::infra::pool::postgres"}
{"timestamp":"2024-10-25T15:05:25.531429Z","level":"INFO","message":"relation \"_sqlx_migrations\" already exists, skipping","target":"sqlx::postgres::notice"}
{"timestamp":"2024-10-25T15:05:25.537865Z","level":"INFO","message":"event: connected","target":"async_nats"}
{"timestamp":"2024-10-25T15:05:25.539773Z","level":"INFO","message":"listening to TCP connections","address":"0.0.0.0","port":8088,"target":"indexer_api::infra::api"}
```

#### Forced Deletion:

```plaintext
Starting entrypoint.sh
Trap set for EXIT
Created /var/run/indexer-api/running
[Application Logs Same As Before]
Trap executed
```

#### `CrashLoopBackOff`:

```plaintext
Starting entrypoint.sh
Trap set for EXIT
Created /var/run/indexer-api/running
{"timestamp":"2024-10-25T17:19:38.311304Z","level":"INFO","message":"starting","config":"Config { ... }","target":"indexer_api"}
{"timestamp":"2024-10-25T17:19:38.311800Z","level":"WARN","message":"Failed to open `.pgpass` file: ...","path":"/nonexistent/.pgpass","target":"sqlx_postgres::options::pgpass"}
{"timestamp":"2024-10-25T17:19:38.80.9.0-rc2Z","level":"ERROR","message":"process exited with ERROR","error":"create DB pool for Postgres: ...","backtrace":"disabled backtrace","target":"indexer_api"}
Trap executed
```

### `running` File Verification

#### After Normal Start:

```bash
kubectl exec -it indexer-api-6cf76bf9c7-sp6s2 -- ls /var/run/indexer-api/
running
```

#### After Pod Deletion:

```bash
kubectl delete pod indexer-api-6cf76bf9c7-hrfww
# pod "indexer-api-6cf76bf9c7-hrfww" deleted

kubectl exec -it indexer-api-6cf76bf9c7-8ppxg -- ls /var/run/indexer-api/
running
```

**Explanation:**

The `running` file exists in the new pod instance, indicating successful cleanup from the previous pod termination and proper recreation in the new pod.

## Notes

- The Kubernetes YAML files were generated using `kompose convert -f docker-compose.yaml -o k8s-manifests/`.
- Minor modifications were made to the manifests to ensure successful deployments in the standalone environment.
- For the infrastructure files and to review the changes made during this investigation, please refer to PR [#164](https://github.com/input-output-hk/midnight-indexer/pull/164).

## Commands Used

A list of key commands used during the investigation:

```plaintext
# Set context to Docker Desktop's Kubernetes cluster
kubectl config use-context docker-desktop

# Convert Docker Compose file to Kubernetes manifests
kompose convert -f docker-compose.yaml -o k8s-manifests/

# Apply Kubernetes manifests
kubectl apply -f k8s-manifests/

# Build Docker image for standalone testing
docker build --build-arg "RUST_VERSION=$(grep channel rust-toolchain.toml | sed -r 's/channel = \"(.*)\"/\1/')" --build-arg "PROFILE=dev" --secret id=netrc,src=$HOME/.netrc -t indexer-api:local -f indexer-api/Dockerfile .

# Get pods with a specific label and watch for changes
kubectl get pods -l io.kompose.service=indexer-api -w

# Delete a pod forcefully
kubectl delete pod indexer-api-84ffc5b9fc-l8crz --grace-period=0 --force

# Execute a command inside a pod
kubectl exec -it indexer-api-84ffc5b9fc-65kjz -- sh

# Check logs of a pod
kubectl logs -f indexer-api-f566c84df-m8zsd

# Apply updates after modifying manifests
kubectl apply -f k8s-manifests/
```