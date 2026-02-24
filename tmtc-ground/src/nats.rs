use embedded_nal_async::TcpConnect;

pub struct Nats<CON: TcpConnect> {
    client: CON,
}

impl<CON: TcpConnect> Nats<CON> {
    pub fn new(client: CON) -> Self {
        Self { client }
    }
}
