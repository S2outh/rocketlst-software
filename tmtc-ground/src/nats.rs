use core::net::SocketAddr;

use alloc::{format, string::String, vec::Vec};
use defmt::{error, info, warn};
use embedded_io_async::{Read, ReadExactError, Write};
use embedded_nal_async::TcpConnect;

const CARR_RETURN: [u8; 2] = *b"\r\n";

#[derive(serde::Deserialize)]
struct NatsInfoMsg {
    server_id: String,
    server_name: String,
    version: String,
    go: String,
    host: String,
    port: i32,
    headers: bool,
    max_payload: i32,
    proto: i32,
}
#[derive(serde::Serialize)]
struct NatsConnectMsg {
    verbose: bool,
    pedantic: bool,
    tls_required: bool,
    user: String,
    pass: String,
    lang: String,
    name: String,
    version: String,
}

impl NatsConnectMsg {
    fn new(user: &str, pwd: &str) -> Self {
        NatsConnectMsg {
            verbose: false,
            pedantic: false,
            tls_required: false,
            user: String::from(user),
            pass: String::from(pwd),
            name: String::from(env!("CARGO_PKG_NAME")),
            lang: String::from("rust"),
            version: String::from(env!("CARGO_PKG_VERSION")),
        }
    }
}

pub struct NatsStack<C: TcpConnect> {
    client: C,
    address: SocketAddr,
}

impl<C: TcpConnect> NatsStack<C> {
    pub fn new(client: C, address: SocketAddr) -> Self {
        Self { client, address }
    }
    pub async fn connect_with_default(&self) -> Result<NatsCon<C>, C::Error> {
        let con = NatsCon::new(self.client.connect(self.address).await?);
        
        Ok(con)
    }
}

pub struct NatsCon<'d, C: 'd + TcpConnect> {
    con: C::Connection<'d>,
    connected: bool,
    user: &'static str,
    pwd: &'static str,
}
#[derive(defmt::Format)]
pub enum NatsError<C: TcpConnect> {
    IOError(ReadExactError<C::Error>),
    NatsErr,
    ParsingErr,
}

pub struct NatsRelayMsg {

}
pub enum NatsMsg {
    Info(NatsInfoMsg),
    Relay(String),
}
impl<'d, C: 'd + TcpConnect> NatsCon<'d, C> {
    fn new(con: C::Connection<'d>) -> Self {
        NatsCon {
            con,
            connected: false,
            user: "nats",
            pwd: "nats"
        }
    }
    async fn sync_frame(&mut self) -> Result<String, ReadExactError<C::Error>> {
        let mut buf = String::new();
        let mut magic_pos = 0;
        loop {
            let mut byte: u8 = 0;
            self.con
                .read_exact(core::slice::from_mut(&mut byte))
                .await?;
            if byte == CARR_RETURN[magic_pos] {
                magic_pos += 1;
                if magic_pos == CARR_RETURN.len() {
                    return Ok(buf);
                }
            } else {
                magic_pos = 0;
                buf.push(byte.into());
            }
        }
    }
    async fn process_next(&mut self) -> Result<(), NatsError<C>> {
        let packet = self.sync_frame().await
                    .map_err(NatsError::IOError)?;
        let (cmd, msg) = packet.split_once(' ').unwrap_or((&packet.trim(), ""));
        match cmd {
            "PING" => {
                self.con.write_all("PONG\r\n".as_bytes()).await
                    .map_err(|e| NatsError::IOError(e.into()))?;
            }
            "INFO" => {
                if let Ok(info) = serde_json::from_str::<NatsInfoMsg>(msg) {
                    info!("connected to server: {}", info.server_name);
                    let answer = format!(
                        "CONNECT {}\r\n",
                        serde_json::to_string(&NatsConnectMsg::new(&self.user, &self.pwd))
                            .unwrap()
                    );
                    self.con.write_all(answer.as_bytes()).await
                        .map_err(|e| NatsError::IOError(e.into()))?;
                } else {
                    warn!("could not decode nats info")
                }
            }
            "-ERR" => {
                error!("nats disconnected ({})", msg);
                return Err(NatsError::NatsErr);
            }
            "MSG" => {
                let Some((_topic, msg)) = msg.split_once(' ') else {
                    return Err(NatsError::ParsingErr);
                };
                let Some((sid, _bytes)) = msg.split_once(' ') else {
                    return Err(NatsError::ParsingErr);
                };
                let Ok(_sid) = sid.parse::<i32>() else {
                    return Err(NatsError::ParsingErr);
                };
                //let mut msg = String::new();
                //let Ok(_) = reader.read_line(&mut msg) else {
                //    return Err(NatsReadError::ParsingErr);
                //};
                //let nats_msg = NatsMsg {
                //    topic: String::from(topic),
                //    data: String::from(msg),
                //};
            }
            default => {
                warn!("unknown nats cmd {}", default);
            }
        }
        Ok(())
    }
    pub async fn publish(&mut self, address: &str, data: Vec<u8>) -> Result<(), NatsError<C>> {
        let msg = String::from_utf8(data).map_err(|_| NatsError::ParsingErr)?;
        self.con.write_all(format!("PUB {} {}\r\n{}\r\n", address, msg.len(), msg).as_bytes()).await
            .map_err(|e| NatsError::IOError(e.into()))?;
        Ok(())
    }
}
