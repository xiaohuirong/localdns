# LocalDNS

A lightweight, local DNS server written in Rust. It automatically resolves hostnames by reading from:
1.  **systemd-networkd DHCP lease files** (JSON format).
2.  **Standard `/etc/hosts` style files**.

It features **hot-reloading**, meaning it watches the source files for changes and updates its internal DNS records automatically without needing a restart.

## Features

*   **Dual Source:** Combines static records from a hosts file and dynamic records from DHCP leases.
*   **Automatic Suffix:** Appends a configurable domain suffix (e.g., `.lan`) to DHCP hostnames.
*   **Wildcard Subdomains for DHCP:** Automatically resolves all subdomains of a DHCP-derived hostname (e.g., `a.mydevice.lan`, `b.c.mydevice.lan` will resolve to `mydevice.lan`'s IP).
*   **Wildcard Hosts File Support:** Supports wildcard entries in the hosts file (e.g., `1.2.3.4 *.example.com` will resolve `www.example.com` and `dev.example.com` to `1.2.3.4`). Exact matches take precedence over wildcards.
*   **Hot-Reloading:** Monitors the configured `dhcp_lease_file` and `hosts_file` for modification time changes (every 5 seconds) and reloads records instantly.
*   **Multiple IPs:** correctly handles multiple IP addresses for the same hostname (Round-robin/All returned).
*   **Lightweight:** Built with `tokio` and `hickory-proto` (formerly `trust-dns-proto`).

## Configuration

Configuration is handled via a `config.toml` file.

**Example `config.toml`:**

```toml
listen_address = "0.0.0.0"
listen_port = 10054
dhcp_lease_file = "./br0"      # Path to systemd-networkd lease file
hosts_file = "./hosts"         # Path to hosts file
domain_suffix = "lan"          # Suffix for DHCP hosts (e.g., hostname -> hostname.lan)
```

## Building and Running

### Prerequisites
*   Rust (latest stable recommended)

### Build
```bash
cargo build --release
```

### Run
```bash
# Run with default config (config.toml in current dir)
./target/release/localdns

# Run with custom config
./target/release/localdns --config /path/to/your/config.toml
```

### Run via Cargo (Development)
```bash
cargo run --bin localdns
```

## Testing

You can use `dig` to test the resolution:

```bash
# Query a host defined in the hosts file
dig @127.0.0.1 -p 10054 some-static-host.local

# Query a host from DHCP leases (assuming suffix is "lan")
dig @127.0.0.1 -p 10054 my-device.lan

# Query a subdomain of a DHCP host
dig @127.0.0.1 -p 10054 test.my-device.lan

# Query a wildcard entry from hosts file (assuming '1.2.3.4 *.example.com' is in hosts)
dig @127.0.0.1 -p 10054 www.example.com
dig @127.0.0.1 -p 10054 dev.example.com
```

## Project Structure

*   `src/main.rs`: Entry point. Sets up the UDP server, handles incoming queries, and manages the file-watching hot-reload loop.
*   `src/loader.rs`: Logic for parsing the systemd-networkd JSON lease file and the standard hosts file format.
*   `src/config.rs`: Configuration loading logic.
