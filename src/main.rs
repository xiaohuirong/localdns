mod config;
mod loader;

use clap::Parser;
use std::io::Write;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration};
use hickory_proto::op::{Message, MessageType, ResponseCode};
use hickory_proto::rr::{RData, Record, RecordType};
use hickory_proto::rr::rdata::A;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the configuration file
    #[arg(short, long, default_value = "config.toml")]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    
    std::panic::set_hook(Box::new(|info| {
        eprintln!("Panic: {:?}", info);
    }));

    let args = Args::parse();

    // 1. Load Config
    println!("Loading config from {:?}", args.config);
    let config = config::Config::load(&args.config)?;
    
    // 2. Load DNS Records (Initial)
    println!("Loading DNS records...");
    let initial_records = loader::load_records(
        &config.dhcp_lease_file, 
        &config.hosts_file, 
        &config.domain_suffix
    )?;
    
    println!("Loaded {} exact domains and {} wildcard patterns.", initial_records.exact_matches.len(), initial_records.wildcards.len());

    let records = Arc::new(RwLock::new(initial_records));

    // Start file watcher task
    let records_clone = records.clone();
    let dhcp_path = config.dhcp_lease_file.clone();
    let hosts_path = config.hosts_file.clone();
    let suffix = config.domain_suffix.clone();

    tokio::spawn(async move {
        let mut last_dhcp_mtime = std::fs::metadata(&dhcp_path).and_then(|m| m.modified()).ok();
        let mut last_hosts_mtime = std::fs::metadata(&hosts_path).and_then(|m| m.modified()).ok();

        loop {
            sleep(Duration::from_secs(5)).await;

            let current_dhcp_mtime = std::fs::metadata(&dhcp_path).and_then(|m| m.modified()).ok();
            let current_hosts_mtime = std::fs::metadata(&hosts_path).and_then(|m| m.modified()).ok();

            let mut reload_needed = false;

            if current_dhcp_mtime != last_dhcp_mtime {
                println!("DHCP file changed. Reloading...");
                last_dhcp_mtime = current_dhcp_mtime;
                reload_needed = true;
            }

            if current_hosts_mtime != last_hosts_mtime {
                println!("Hosts file changed. Reloading...");
                last_hosts_mtime = current_hosts_mtime;
                reload_needed = true;
            }

            if reload_needed {
                match loader::load_records(&dhcp_path, &hosts_path, &suffix) {
                    Ok(new_cache) => {
                        let exact_count = new_cache.exact_matches.len();
                        let wildcard_count = new_cache.wildcards.len();
                        {
                            let mut writer = records_clone.write().await;
                            *writer = new_cache;
                        }
                        println!("Reloaded records. Now serving {} exact domains and {} wildcard patterns.", exact_count, wildcard_count);
                    },
                    Err(e) => eprintln!("Failed to reload records: {}", e),
                }
            }
        }
    });

    // 3. Bind UDP Socket
    let addr = format!("{}:{}", config.listen_address, config.listen_port);
    let socket = UdpSocket::bind(&addr).await?;
    println!("DNS Server listening on {}", addr);
    std::io::stdout().flush().unwrap();
    
    let socket = Arc::new(socket);

    // 4. Server Loop
    let mut buf = [0u8; 4096];
    println!("Entering server loop...");
    std::io::stdout().flush().unwrap();
    loop {
        let (len, src) = match socket.recv_from(&mut buf).await {
            Ok(x) => x,
            Err(e) => {
                eprintln!("Failed to receive UDP packet: {}", e);
                continue;
            }
        };

        let data = buf[..len].to_vec();
        let records = records.clone();
        let socket = socket.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_query(socket, data, src, records).await {
                eprintln!("Error handling query from {}: {}", src, e);
            }
        });
    }
}

async fn handle_query(
    socket: Arc<UdpSocket>,
    data: Vec<u8>,
    src: SocketAddr,
    records: Arc<RwLock<loader::DnsCache>>,
) -> anyhow::Result<()> {
    // Parse the query
    let request = match Message::from_vec(&data) {
        Ok(m) => m,
        Err(e) => {
            return Err(anyhow::anyhow!("Failed to parse message: {}", e));
        }
    };

    // Create a response based on the request ID and settings
    let mut response = Message::new();
    response.set_id(request.id());
    response.set_message_type(MessageType::Response);
    response.set_op_code(request.op_code());
    response.set_recursion_desired(request.recursion_desired());
    response.set_recursion_available(true);

    if let Some(query) = request.queries().first() {
        response.add_query(query.clone());
        
        let name = query.name();
        let lookup_name = name.to_string().to_lowercase();
        
        let records_guard = records.read().await;
        
        let mut found_ips: Vec<Ipv4Addr> = Vec::new();

        if query.query_type() == RecordType::A {
            // 1. Try exact match
            if let Some(ips) = records_guard.exact_matches.get(&lookup_name) {
                found_ips.extend(ips);
            }

            // 2. Try wildcard match (always check to merge results)
            for (pattern, ip) in &records_guard.wildcards {
                // Pattern is like "*.example.com."
                // lookup_name is like "sub.example.com." or "sub.sub.example.com."
                if pattern.starts_with("*.") {
                    let root_domain = &pattern[2..]; // e.g., "example.com."
                    // Check if lookup_name ends with the root_domain of the wildcard pattern
                    // and is not the root_domain itself (i.e., it's actually a subdomain)
                    if lookup_name.ends_with(root_domain) && lookup_name.len() > root_domain.len() {
                        found_ips.push(*ip);
                    }
                }
            }

            if !found_ips.is_empty() {
                // Remove duplicates and sort, though `Vec` might be fine for this limited case
                found_ips.sort_unstable();
                found_ips.dedup();

                for ip in found_ips {
                    let mut record = Record::with(name.clone(), RecordType::A, 60);
                    record.set_data(Some(RData::A(A(ip))));
                    response.add_answer(record);
                }
                response.set_response_code(ResponseCode::NoError);
            } else {
                response.set_response_code(ResponseCode::NXDomain);
            }
        } else {
            // For other record types, if the exact name exists, return NoError but no data.
            // If the name doesn't exist at all (even by wildcard), return NXDomain.
            if records_guard.exact_matches.contains_key(&lookup_name) {
                response.set_response_code(ResponseCode::NoError);
            } else {
                // Also check for wildcard match if not exact, for the purpose of NXDomain vs NoError
                let mut name_found_by_wildcard = false;
                for (pattern, _) in &records_guard.wildcards {
                    if pattern.starts_with("*.") {
                        let root_domain = &pattern[2..];
                        if lookup_name.ends_with(root_domain) && lookup_name.len() > root_domain.len() {
                            name_found_by_wildcard = true;
                            break;
                        }
                    }
                }
                if name_found_by_wildcard {
                    response.set_response_code(ResponseCode::NoError);
                } else {
                    response.set_response_code(ResponseCode::NXDomain);
                }
            }
        }
    } else {
        response.set_response_code(ResponseCode::FormErr);
    }

    // Serialize and send
    let response_bytes = response.to_vec()?;
    socket.send_to(&response_bytes, src).await?;

    Ok(())
}