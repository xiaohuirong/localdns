mod config;
mod loader;

use clap::Parser;
use std::io::Write;
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
    
    println!("Loaded {} unique domains.", initial_records.len());

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
                    Ok(new_records) => {
                        let count = new_records.len();
                        {
                            let mut writer = records_clone.write().await;
                            *writer = new_records;
                        }
                        println!("Reloaded records. Now serving {} domains.", count);
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
    records: Arc<RwLock<loader::DnsRecords>>,
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

        if query.query_type() == RecordType::A {
            if let Some(ips) = records_guard.get(&lookup_name) {
                for ip in ips {
                    let mut record = Record::with(name.clone(), RecordType::A, 60);
                    record.set_data(Some(RData::A(A(*ip))));
                    response.add_answer(record);
                }
                response.set_response_code(ResponseCode::NoError);
            } else {
                response.set_response_code(ResponseCode::NXDomain);
            }
        } else {
            if records_guard.contains_key(&lookup_name) {
                response.set_response_code(ResponseCode::NoError);
            } else {
                response.set_response_code(ResponseCode::NXDomain);
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