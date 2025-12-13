use std::collections::{HashMap, HashSet};
use std::fs;
use std::net::Ipv4Addr;
use std::path::Path;
use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Deserialize, Debug)]
struct DhcpData {
    #[serde(rename = "Leases")]
    leases: Vec<DhcpLease>,
}

#[derive(Deserialize, Debug)]
struct DhcpLease {
    #[serde(rename = "Address")]
    address: [u8; 4],
    #[serde(rename = "Hostname")]
    hostname: String,
}

pub type DnsRecords = HashMap<String, Vec<Ipv4Addr>>;

pub fn load_records(
    dhcp_path: &Path,
    hosts_path: &Path,
    suffix: &str,
) -> Result<DnsRecords> {
    let mut records: HashMap<String, HashSet<Ipv4Addr>> = HashMap::new();

    // 1. Load DHCP records
    if dhcp_path.exists() {
        let content = fs::read_to_string(dhcp_path)
            .with_context(|| format!("Failed to read DHCP file: {:?}", dhcp_path))?;
        
        // Handle empty or malformed files gracefully
        if !content.trim().is_empty() {
             let dhcp_data: Result<DhcpData, _> = serde_json::from_str(&content);
             match dhcp_data {
                 Ok(data) => {
                     for lease in data.leases {
                         if lease.hostname.is_empty() {
                             continue;
                         }
                         let ip = Ipv4Addr::new(
                             lease.address[0],
                             lease.address[1],
                             lease.address[2],
                             lease.address[3],
                         );
                         
                         // Construct FQDN: hostname + suffix + .
                         // Ensure suffix starts with dot if not empty
                         let safe_suffix = if suffix.starts_with('.') || suffix.is_empty() {
                             suffix.to_string()
                         } else {
                             format!(".{}", suffix)
                         };
                         
                         let domain = format!("{}{}.", lease.hostname, safe_suffix).to_lowercase();
                         records.entry(domain).or_default().insert(ip);
                     }
                 },
                 Err(e) => eprintln!("Warning: Failed to parse DHCP JSON: {}", e),
             }
        }
    } else {
        eprintln!("Warning: DHCP file not found at {:?}", dhcp_path);
    }

    // 2. Load Hosts records
    if hosts_path.exists() {
        let content = fs::read_to_string(hosts_path)
            .with_context(|| format!("Failed to read Hosts file: {:?}", hosts_path))?;
        
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Split by whitespace
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 2 {
                continue;
            }

            // Parse IP
            if let Ok(ip) = parts[0].parse::<Ipv4Addr>() {
                // The rest are hostnames
                for hostname in &parts[1..] {
                    if hostname.starts_with('#') {
                        break; // Inline comment
                    }
                    let mut domain = hostname.to_lowercase();
                    if !domain.ends_with('.') {
                        domain.push('.');
                    }
                    records.entry(domain).or_default().insert(ip);
                }
            }
        }
    } else {
        eprintln!("Warning: Hosts file not found at {:?}", hosts_path);
    }

    // Convert HashSet to Sorted Vec
    let mut sorted_records: DnsRecords = HashMap::new();
    for (domain, ips) in records {
        let mut ip_vec: Vec<Ipv4Addr> = ips.into_iter().collect();
        ip_vec.sort();
        sorted_records.insert(domain, ip_vec);
    }

    Ok(sorted_records)
}
