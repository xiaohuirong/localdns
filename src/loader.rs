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

pub struct DnsCache {
    pub exact_matches: HashMap<String, Vec<Ipv4Addr>>,
    pub wildcards: Vec<(String, Ipv4Addr)>, // Stores patterns like "*.example.com."
}

impl Default for DnsCache {
    fn default() -> Self {
        Self {
            exact_matches: HashMap::new(),
            wildcards: Vec::new(),
        }
    }
}

pub fn load_records(
    dhcp_path: &Path,
    hosts_path: &Path,
    suffix: &str,
) -> Result<DnsCache> {
    let mut cache = DnsCache::default();
    let mut exact_records_temp: HashMap<String, HashSet<Ipv4Addr>> = HashMap::new();

    // 1. Load DHCP records
    if dhcp_path.exists() {
        let content = fs::read_to_string(dhcp_path)
            .with_context(|| format!("Failed to read DHCP file: {:?}", dhcp_path))?;
        
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
                         
                         let safe_suffix = if suffix.starts_with('.') || suffix.is_empty() {
                             suffix.to_string()
                         } else {
                             format!(".{}", suffix)
                         };
                         
                         let fqdn = format!("{}{}.", lease.hostname, safe_suffix).to_lowercase();
                         exact_records_temp.entry(fqdn.clone()).or_default().insert(ip);
                         
                         // Add wildcard for DHCP entry
                         let wildcard_pattern = format!("*.{}.", fqdn.trim_end_matches('.')); // Remove trailing dot, then add *. and a dot
                         cache.wildcards.push((wildcard_pattern, ip));
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

            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 2 {
                continue;
            }

            if let Ok(ip) = parts[0].parse::<Ipv4Addr>() {
                for hostname in &parts[1..] {
                    if hostname.starts_with('#') {
                        break;
                    }
                    let mut domain = hostname.to_lowercase();
                    if !domain.ends_with('.') {
                        domain.push('.');
                    }

                    if domain.starts_with("*.") {
                        cache.wildcards.push((domain, ip));
                    } else {
                        exact_records_temp.entry(domain).or_default().insert(ip);
                    }
                }
            }
        }
    } else {
        eprintln!("Warning: Hosts file not found at {:?}", hosts_path);
    }

    // Convert HashSet to Sorted Vec for exact matches
    for (domain, ips) in exact_records_temp {
        let mut ip_vec: Vec<Ipv4Addr> = ips.into_iter().collect();
        ip_vec.sort();
        cache.exact_matches.insert(domain, ip_vec);
    }

    Ok(cache)
}
