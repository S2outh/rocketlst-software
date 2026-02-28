use core::net::SocketAddr;

use alloc::{format, string::String, vec::Vec};
use defmt::{error, info, warn};
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, mutex::Mutex};
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

pub struct NatsStack<'d, C: 'd + TcpConnect> {
    client: C,
    raw_con: Option<Mutex<ThreadModeRawMutex, <C as TcpConnect>::Connection<'d>>>,
    address: SocketAddr,
}

impl<'d, C: TcpConnect> NatsStack<'d, C> {
    pub fn new(client: C, address: SocketAddr) -> Self {
        Self { client, address, raw_con: None }
    }
    pub async fn connect_with_default(&'d mut self) -> Result<(NatsCon<'d, C>, NatsRunner<'d, C>), C::Error> {
        self.raw_con = Some(Mutex::new(self.client.connect(self.address).await?));
        let nats_con = NatsCon::new(&self.raw_con.as_ref().unwrap());
        let runner = NatsRunner::new(&self.raw_con.as_ref().unwrap());
        
        Ok((nats_con, runner))
    }
}
pub struct NatsCon<'d, C: 'd + TcpConnect> {
    con: &'d Mutex<ThreadModeRawMutex, C::Connection<'d>>,
}
impl<'d, C: 'd + TcpConnect> NatsCon<'d, C> {
    fn new(con: &'d Mutex<ThreadModeRawMutex, C::Connection<'d>>) -> Self {
        Self { con }
    }

    pub async fn publish(&mut self, address: &str, bytes: Vec<u8>) -> Result<(), NatsError<C>> {
        let str_header = format!("PUB {} {}\r\n", address, bytes.len());
        let header = str_header.as_bytes();
        let end = b"\r\n";

        let mut packet = Vec::with_capacity(header.len() + bytes.len() + end.len());
        packet.extend_from_slice(header);
        packet.extend_from_slice(&bytes);
        packet.extend_from_slice(end);

        self.con.lock().await.write_all(&packet).await
            .map_err(|e| NatsError::IOError(e.into()))
    }
}

pub struct NatsRunner<'d, C: 'd + TcpConnect> {
    con: &'d Mutex<ThreadModeRawMutex, C::Connection<'d>>,
    user: &'static str,
    pwd: &'static str,
}
#[derive(defmt::Format)]
pub enum NatsError<C: TcpConnect> {
    IOError(ReadExactError<C::Error>),
    NatsErr,
    ParsingErr,
}

impl<'d, C: 'd + TcpConnect> NatsRunner<'d, C> {
    fn new(con: &'d Mutex<ThreadModeRawMutex, C::Connection<'d>>) -> Self {
        Self {
            con,
            user: "nats",
            pwd: "nats"
        }
    }
    async fn sync_frame(&mut self) -> Result<Vec<u8>, ReadExactError<C::Error>> {
        let mut buf: Vec<u8> = Vec::new();
        let mut magic_pos = 0;
        loop {
            let mut byte: u8 = 0;
            self.con
                .lock()
                .await
                .read(core::slice::from_mut(&mut byte))
                .await?;
            if byte == CARR_RETURN[magic_pos] {
                magic_pos += 1;
                if magic_pos == CARR_RETURN.len() {
                    return Ok(buf);
                }
            } else {
                magic_pos = 0;
                buf.push(byte);
            }
        }
    }
    async fn poll_next(&mut self) -> Result<(), NatsError<C>> {
        let packet = self.sync_frame().await
                    .map_err(NatsError::IOError)?;
        let packet_str = String::from_utf8(packet).unwrap();
        let (cmd, msg) = packet_str.split_once(' ').unwrap_or((&packet_str.trim(), ""));
        match cmd {
            "PING" => {
                self.con.lock().await.write_all("PONG\r\n".as_bytes()).await
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
                    self.con.lock().await.write_all(answer.as_bytes()).await
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
                let Some((topic, msg)) = msg.split_once(' ') else {
                    return Err(NatsError::ParsingErr);
                };
                let Some((sid, _bytes)) = msg.split_once(' ') else {
                    return Err(NatsError::ParsingErr);
                };
                let Ok(sid) = sid.parse::<i32>() else {
                    return Err(NatsError::ParsingErr);
                };
                info!("A message :) {}, {}", topic, sid);
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
    pub async fn run(&mut self) -> ! {
        loop {
            if let Err(_) = self.poll_next().await {
                panic!("nats crashed");
            }
        }
    }
}
