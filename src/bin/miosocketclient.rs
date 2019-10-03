use std::{net,str,thread,fs};
use std::io::{self,BufReader,Read,Write};
use std::sync::{mpsc,Arc};

use log::{debug, info, warn};

use mio::{Events, Poll, Ready, PollOpt, Token, net::TcpStream};

use rustls::{ClientSession,Session};

const TALKER: Token = mio::Token(0);

fn main () {

    // rustls configuration
    let mut config = rustls::ClientConfig::new();
    let certfile = fs::File::open("/home/ianc/rootCertificate.pem").expect("Cannot open CA file");
    let mut reader = BufReader::new(certfile);
    config.root_store.add_pem_file(&mut reader).unwrap();
    let rc_config = Arc::new(config);
    let example_com = webpki::DNSNameRef::try_from_ascii_str("localhost").unwrap();
    let mut client = ClientSession::new(&rc_config, example_com);

    let poll = Poll::new().unwrap();


    let addr: net::SocketAddr = "127.0.0.1:9797".parse().unwrap();
    match TcpStream::connect(&addr) {
        Ok(mut stream) => {
            // Spawn thread to read user input
            let (tx, rx) = mpsc::channel();

            thread::spawn(move || {
                let mut user_name = String::new();
                loop {
                    let stdin = io::stdin();
                    let mut buffer = String::new();

                    // Display user-prompt
                    if user_name.is_empty() {
                        print!("Please enter user name: ");
                    } else {
                        print!("> ");
                    }
                    io::stdout().flush().unwrap();

                    match stdin.read_line(&mut buffer) {
                        Ok(_n) => {
                            if user_name.is_empty() {
                                user_name = buffer.clone().trim().to_string();
                            }
                            buffer = buffer.trim().to_string();
                            tx.send(buffer).unwrap();
                        },
                        Err(err) => {
                            warn!("Error: {}", err);
                            break;
                        },
                    }
                }
            });

            // Register the poll for reading
            poll.register(&stream, TALKER, Ready::readable() | Ready::writable(), PollOpt::level() | PollOpt::oneshot()).unwrap();
            let mut events = Events::with_capacity(1024);

            'outer: loop {

                // See if we have any user input from the reader thread
                match rx.try_recv() {
                    Ok(buffer) => {
                        debug!("{}", buffer);
                        let buffer_data = buffer.as_bytes();
                        let mut staged_data: Vec<u8> = Vec::with_capacity(buffer_data.len() + 2);
                        staged_data.push(1);
                        staged_data.push(buffer_data.len() as u8);
                        staged_data.extend_from_slice(&buffer_data[..]);
                        match client.write(&staged_data) {
                            Ok(n) => debug!("client wrote {} bytes", n),
                            Err(e) => panic!(e),
                        }
                    },
                    Err(_) => {
                        // Expected most of the time
                    },
                }

                // Poll for any stream events
    poll.poll(&mut events, None).unwrap();

    for event in &events {
        match event.token() {
            TALKER => {
                // Handle writable event
                if event.readiness().is_writable() && client.wants_write() {
                    match client.write_tls(&mut stream) {
                        Ok(size) => {
                            debug!("Wrote {} bytes", size);
                        }
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                            // Socket is not ready anymore, stop reading
                            debug!("Would block");
                        }
                        Err(ref e) if e.kind() == io::ErrorKind::ConnectionReset => {
                            // Socket is not ready anymore, stop reading
                            debug!("Connection reset breaking");
                            break 'outer;
                        }
                        Err(ref e) if e.kind() == io::ErrorKind::ConnectionRefused => {
                            // Unable to connect to server
                            println!("Server unavailable");
                            break 'outer;
                        }
                        e => panic!("err={:?}", e), // Unexpected error
                    }
                }

                // Handle readable event
                if event.readiness().is_readable() && client.wants_read() {
                    //match stream.read(&mut data) {
                    match client.read_tls(&mut stream) {
                        Ok(0) => {
                            // Socket is closed
                            debug!("Socket closed");
                            break 'outer;
                        }
                        Ok(n) => {
                            debug!("read_tls: {} bytes", n);
                            client.process_new_packets().unwrap();
                            // Echo everything to stdout
                            let mut data: Vec<u8> = Vec::new();
                            match client.read_to_end(&mut data) {
                                Ok(0) => (),
                                Ok(1) => unreachable!(),
                                Ok(n) => {
                                    debug!("read_to_end: {}", n);
                                    debug!("Got a data length: {}", data.len());

                                    // process the data
                                    process_server_data(data[0], data[1], &data[2..data.len()]);
                                },
                                Err(e) => panic!(e),
                            }
                        }
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                            // Socket is not ready anymore, stop reading
                            debug!("Would block");
                        }
                        Err(ref e) if e.kind() == io::ErrorKind::ConnectionReset => {
                            // Socket is not ready anymore, stop reading
                            println!("Connection reset breaking");
                            break 'outer;
                        }
                        e => panic!("err={:?}", e), // Unexpected error
                    }
                }
            },
            _ => unreachable!(),
        }
    }

                // Register the poll for reading OR reading & writing
                poll.reregister(&stream, TALKER, Ready::readable() | Ready::writable(), PollOpt::level() | PollOpt::oneshot()).unwrap();
            }
        },
        Err(_) => println!("Couldn't connect to server..."),
    }
}


fn process_server_data(control_byte: u8, data_len: u8, data: &[u8]) {
    info!("Processing [control_byte: {}; data_len: {}]", control_byte, data_len);
    match control_byte {
        // 0: Opponent has disconnected
        0 => {
            println!("Your chat partner has ended the conversation...");
        },

        // 1: New message from opponent
        1 => {
            // Echo everything to stdout
            match str::from_utf8(data) {
                Ok(v) => {
                    print!("{}> ", v);
                    io::stdout().flush().unwrap();
                }
                Err(_) => panic!("invalid utf-8 sequence!"),
            };
        },

        // 2: Partner name message
        2 => {
            // Echo everything to stdout
            match str::from_utf8(data) {
                Ok(v) => {
                    print!("[{}] ", v);
                }
                Err(_) => panic!("invalid utf-8 sequence!"),
            };
        },

        // Unknown control byte; do nothing?
        _ => {
            println!("Unknown control byte");
        }
    }
}
