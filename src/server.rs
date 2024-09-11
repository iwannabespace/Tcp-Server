use std::io::{Read, Write};
use std::thread::sleep;
use std::time::Duration;
use std::{io, thread};
use std::net::{SocketAddr, TcpListener};
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{channel, Sender, Receiver};

use crate::client::Client;
use crate::custom_types::ChannelDataType;
use crate::logger;

type OnClientConnectedCallback = Arc<Mutex<Box<dyn Fn(u64, SocketAddr) + Send>>>;
type OnClientDisconnectedCallback = Arc<Mutex<Box<dyn Fn(u64) + Send>>>;
type OnMessageReceivedCallback = Arc<Mutex<Box<dyn Fn(u64, Vec<u8>) + Send>>>;

pub struct TcpServerData {
    listener: TcpListener,
    clients: Arc<Mutex<Vec<Client>>>,
    sender: Sender<ChannelDataType>,
    receiver: Mutex<Receiver<ChannelDataType>>,
    on_client_connected: OnClientConnectedCallback,
    on_client_disconnected : OnClientDisconnectedCallback,
    on_message_received: OnMessageReceivedCallback, 
}

impl TcpServerData {
    fn new(address: &str) -> Result<Self, String> {
        if let Ok(listener) = TcpListener::bind(address) {
            if listener.set_nonblocking(true).is_ok() {
                let (sender, receiver) = channel::<ChannelDataType>();
                Ok(
                    Self {
                        listener,
                        sender,
                        receiver: Mutex::new(receiver),
                        clients: Arc::new(Mutex::new(Vec::new())),
                        on_client_connected: Arc::new(Mutex::new(Box::new(|_, _| {}))),
                        on_client_disconnected: Arc::new(Mutex::new(Box::new(|_| {}))),
                        on_message_received: Arc::new(Mutex::new(Box::new(|_, _| {}))),
                    }
                )
            } else {
                Err("Listener couldn't be set to be non-blocking!".to_string())
            }
        } else {
            Err("Listener couldn't be bind to address!".to_string())
        }
    }
}

pub struct TcpServer {
    data: Arc<TcpServerData>,
    nonblocking: bool,
}

impl TcpServer {
    pub fn new(address: &str) -> Result<Self, String> {
        let data = TcpServerData::new(address)?;
        Ok(Self { 
            data: Arc::new(data),
            nonblocking: true,
        })
    }

    pub fn run(&self) {
        if self.nonblocking {
            self.run_nonblocking();
        } else {
            self.run_blocking();
        }
    }

    pub fn send(&self, client_id: u64, data: &[u8]) -> Result<(), String> {
        let data_ref = self.data.clone();

        if let Ok(clients) = data_ref.clients.lock() {
            if let Some(client) = clients.iter().find(|client| client.id == client_id) {
                let mut socket = &client.socket;
                let header = (data.len() as u64).to_le_bytes();
                let mut header_written: usize = 0;
                let mut body_written: usize = 0;
                
                while header_written < 8 {
                    match socket.write(&header[header_written..]) {
                        Ok(size) => {
                            if size > 0 {
                                header_written += size;
                            }
                        },
                        Err(e) => {
                            if e.kind() == io::ErrorKind::WouldBlock {
                                thread::sleep(Duration::from_millis(50));
                            } else {
                                return Err(e.kind().to_string());
                            }
                        }
                    }
                }

                while body_written < data.len() {
                    match socket.write(&data[body_written..]) {
                        Ok(size) => {
                            if size > 0 {
                               body_written += size;
                            }
                        },
                        Err(e) => {
                            if e.kind() == io::ErrorKind::WouldBlock {
                                thread::sleep(Duration::from_millis(50));
                            } else {
                                return Err(e.kind().to_string());
                            }
                        }
                    }
                }

                return Ok(());
            }
        }

        Err("Couldn't lock clients".to_string())
    }

    fn run_nonblocking(&self) {
        let data_ref = self.data.clone();

        thread::spawn(move || {
            let mut cids = 0;
            loop {
                match data_ref.listener.accept() {
                    Ok((socket, address)) => {
                        if let Ok(mut clients) = data_ref.clients.lock() {
                            if socket.set_nonblocking(true).is_ok() {
                                let client_id = cids;
                                cids += 1;

                                clients.push(Client::new(client_id, socket));

                                if let Ok(on_client_connected) = data_ref.on_client_connected.lock() {
                                    on_client_connected(client_id, address);
                                } else {
                                    logger::log_to_file("logs.txt", "OnClientConnectedCallback couldn't be locked!");
                                }
                            }
                        }
                    },
                    Err(e) => {
                        if e.kind() != io::ErrorKind::WouldBlock {
                            break;
                        }
                    }
                }

                if let Ok(mut clients) = data_ref.clients.lock() {
                    for client in clients.iter_mut() {
                        let mut socket_ref = &client.socket;
                        let mut amount_to_read: usize = 0;
                        let header_size = std::mem::size_of::<u64>();
                        
                        if client.read_bytes >= 8 {
                            let arr: [u8; 8] = client.buffer[0..header_size].try_into().unwrap();
                            amount_to_read = usize::from_le_bytes(arr);
                            
                            if client.buffer.len() != header_size + amount_to_read {
                                client.buffer.resize(header_size + amount_to_read, 0);
                            }
                        }

                        match socket_ref.read(&mut client.buffer[client.read_bytes..]) {
                            Ok(size) => {
                                if size == 0 {
                                    if let Err(e) = data_ref.sender.send(ChannelDataType::RemoveClient(client.id)) {
                                        logger::log_to_file("logs.txt", format!("Channel data couldn't be sent!: {e}").as_str());
                                    }
                                    if let Ok(on_client_disconnected) = data_ref.on_client_disconnected.lock() {
                                        on_client_disconnected(client.id);
                                    } else {
                                        logger::log_to_file("logs.txt", "OnClientDisconnectedCallback couldn't be locked");
                                    }
                                } else {
                                    client.read_bytes += size;

                                    if amount_to_read > 0 && client.read_bytes == header_size + amount_to_read {
                                        if let Ok(on_message_received) = data_ref.on_message_received.lock() {
                                            on_message_received(client.id, client.buffer[header_size..].to_vec());
                                            client.buffer.resize(8, 0);
                                            client.read_bytes = 0;
                                        } else {
                                            logger::log_to_file("logs.txt", "OnMessageReceivedCallback couldn't be locked!");
                                        }
                                    }
                                }
                            },
                            Err(e) => {
                                if e.kind() == io::ErrorKind::WouldBlock {
                                    thread::sleep(Duration::from_millis(50));
                                }
                            }
                        }
                    }
                }

                if let Ok(receiver) = data_ref.receiver.lock() {
                    if let Ok(data) = receiver.try_recv() {
                        if let ChannelDataType::RemoveClient(id) = data {
                            if let Ok(mut clients) = data_ref.clients.lock() {
                                clients.retain(|cli| cli.id != id);
                            }
                        } else if let ChannelDataType::Other = data {
                            todo!();
                        }
                    }
                }

                sleep(Duration::from_millis(100));
            }
        });
    }

    fn run_blocking(&self) {
        let data_ref = self.data.clone();
        let mut header: [u8; 8] = [0; 8];

        loop {
            match data_ref.listener.accept() {
                Ok((socket, address)) => {
                    if let Ok(mut clients) = data_ref.clients.lock() {
                        if socket.set_nonblocking(true).is_ok() {
                            let client_id = clients.len() as u64;
                            clients.push(Client::new(client_id, socket));

                            if let Ok(on_client_connected) = data_ref.on_client_connected.lock() {
                                on_client_connected(client_id, address);
                            } else {
                                logger::log_to_file("logs.txt", "OnClientConnectedCallback couldn't be locked!");
                            }
                        }
                    }
                },
                Err(e) => {
                    if e.kind() != io::ErrorKind::WouldBlock {
                        break;
                    }
                }
            }

            if let Ok(clients) = data_ref.clients.lock() {
                for client in clients.iter() {
                    let mut socket_ref = &client.socket;

                    match socket_ref.read_exact(&mut header) {
                        Ok(_) => {
                            let expected_bytes = usize::from_le_bytes(header);
                            let mut data = vec![0; expected_bytes];

                            match socket_ref.read_exact(&mut data) {
                                Ok(_) => {
                                    if let Ok(on_message_received) = data_ref.on_message_received.lock() {
                                        on_message_received(client.id, data);
                                    } else {
                                        logger::log_to_file("logs.txt", "OnMessageReceivedCallback couldn't be locked!");
                                    }
                                },
                                Err(e) => {
                                    if e.kind() == io::ErrorKind::UnexpectedEof {
                                        if let Err(e) = data_ref.sender.send(ChannelDataType::RemoveClient(client.id)) {
                                            logger::log_to_file("logs.txt", format!("Channel data couldn't be sent! {e}").as_str());
                                        }
                                        if let Ok(on_client_disconnected) = data_ref.on_client_disconnected.lock() {
                                            on_client_disconnected(client.id);
                                        } else {
                                            logger::log_to_file("logs.txt", "OnClientDisconnectedCallback couldn't be locked");
                                        }
                                    }
                                }
                            }
                        },
                        Err(e) => {
                            if e.kind() == io::ErrorKind::UnexpectedEof {
                                if let Err(e) = data_ref.sender.send(ChannelDataType::RemoveClient(client.id)) {
                                    println!("Channel data couldn't be sent!: {e}");
                                }
                                if let Ok(on_client_disconnected) = data_ref.on_client_disconnected.lock() {
                                    on_client_disconnected(client.id);
                                } else {
                                     logger::log_to_file("logs.txt", "OnClientDisconnectedCallback couldn't be locked");
                                }
                            }
                        }
                    }
                }
            }

            if let Ok(receiver) = data_ref.receiver.lock() {
                if let Ok(data) = receiver.try_recv() {
                    if let ChannelDataType::RemoveClient(id) = data {
                        if let Ok(mut clients) = data_ref.clients.lock() {
                            clients.retain(|cli| cli.id != id);
                        }
                    } else if let ChannelDataType::Other = data {
                        todo!();
                    }
                }
            }
        }
    }

    pub fn set_nonblocking(&mut self, nonblocking: bool) {
        self.nonblocking = nonblocking;
    }

    pub fn set_on_client_connected<F>(&mut self, callback: F)
    where
        F: Fn(u64, SocketAddr) + Send + 'static,
    {
        if let Ok(mut cb) = self.data.on_client_connected.lock() {
            *cb = Box::new(callback);
        }
    }

    pub fn set_on_client_disconnected<F>(&mut self, callback: F)
    where
        F: Fn(u64) + Send + 'static,
    {
        if let Ok(mut cb) = self.data.on_client_disconnected.lock() {
            *cb = Box::new(callback);
        }
    }

    pub fn set_on_message_received<F>(&mut self, callback: F)
    where
        F: Fn(u64, Vec<u8>) + Send + 'static,
    {
        if let Ok(mut cb) = self.data.on_message_received.lock() { 
           *cb = Box::new(callback);
        }
    }
}
