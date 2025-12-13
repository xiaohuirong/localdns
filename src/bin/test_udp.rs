use tokio::net::UdpSocket;
use std::io::Write;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let socket = UdpSocket::bind("0.0.0.0:10055").await?;
    println!("Simple Server Listening on 10055");
    std::io::stdout().flush().unwrap();

    let mut buf = [0u8; 1024];
    loop {
        let (len, addr) = socket.recv_from(&mut buf).await?;
        println!("Received {} bytes from {}", len, addr);
        std::io::stdout().flush().unwrap();
        socket.send_to(&buf[..len], addr).await?;
    }
}
