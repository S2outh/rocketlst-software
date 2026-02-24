use embassy_net::tcp::client::TcpClient;


struct Nats {
    client: TcpClient<'static, 1>,
}

