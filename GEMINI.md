# GEMINI Context & Handover

## Project Overview
**Goal:** Create a local DNS server in Rust that resolves domain names based on `systemd-networkd` DHCP lease records and a standard `hosts` file.

**Current Status:** Functional prototype complete.

## Key Implementations

### 1. Core Logic (`src/main.rs`)
*   **UDP Server:** Uses `tokio::net::UdpSocket` bound to `0.0.0.0:10054` (configurable).
*   **DNS Protocol:** Uses `hickory-proto` for parsing and constructing DNS packets. Currently supports `A` records.
*   **Concurrency:**
    *   DNS Records are stored in an `Arc<RwLock<DnsRecords>>`.
    *   A background task runs an infinite loop (sleeping 5s) to check file modification times (`mtime`).
    *   When files change, a write lock is acquired to update the records.
    *   Incoming queries acquire a read lock to serve responses.

### 2. Data Loading (`src/loader.rs`)
*   **DHCP Leases:** Parses JSON files typically found in `/var/lib/systemd/network/dhcp-server-lease/`.
    *   Extracts `Hostname` and `Address`.
    *   Appends a configurable suffix (e.g., `.lan`).
    *   Example source: `br0`.
*   **Hosts File:** Parses standard `/etc/hosts` format.
    *   Ignores comments (`#`).
    *   Maps multiple hostnames to a single IP.
*   **Data Structure:** `HashMap<String, Vec<Ipv4Addr>>`. Only IPv4 is currently supported.

### 3. Configuration (`src/config.rs`, `config.toml`)
*   Uses `toml` crate for parsing.
*   Configurable fields: `listen_address`, `listen_port`, `dhcp_lease_file`, `hosts_file`, `domain_suffix`.

## Recent Operations
1.  **Codebase Analysis:** Read existing `Cargo.toml` and source files to understand the initial structure.
2.  **Feature Implementation:** Modified `src/main.rs` to add the hot-reloading logic using `tokio::spawn` and `std::fs::metadata`.
3.  **Verification:**
    *   Ran the server using `nohup`.
    *   Verified resolution of existing DHCP hosts (`rarch.lan`).
    *   Verified hot-reloading by appending to `hosts` and querying the new entry (`test.example.com`).
    *   Cleaned up test entries and stopped the server.

## Future To-Dos / Ideas
*   **IPv6 Support:** Add `AAAA` record support (currently only handles `Ipv4Addr`).
*   **System integration:** Create a systemd service file (`localdns.service`) for deployment.
*   **Optimization:** Replace polling loop with `notify` crate for event-driven file watching.
*   **Error Handling:** Improve robustness for malformed packets or edge cases in file parsing.
