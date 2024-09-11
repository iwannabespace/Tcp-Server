use std::net::TcpStream;

pub struct Client {
    pub id: u64,
    pub socket: TcpStream,
    pub buffer: Vec<u8>,
    pub read_bytes: usize 
}

impl Client {
    pub fn new(id: u64, socket: TcpStream) -> Self {
        Self { 
            id, 
            socket,
            buffer: vec![0; 8],
            read_bytes: 0
        }
    }
}
