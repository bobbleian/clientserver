use std::{str,fs};
use std::io::{self, Read, Write, BufReader};
use std::net;
use std::collections::{HashMap};
use std::sync::{Arc};

use mio::{Events, Poll, Ready, PollOpt, Token};
use mio::net::{TcpListener};

use rustls;
use rustls::{ServerConfig,ServerSession,Session,NoClientAuth};

const LISTENER: Token = Token(0);

// Enumeration to store client state
enum ClientState {
    Connected,
    WaitingOnOpponent(String),
    GamePending,
    GameInProgress(Token),
}

fn main () {
    // Used to store the sockets.
    let mut sockets = HashMap::new();
    let mut tls_servers = HashMap::new();

    // Used to store client status
    let mut client_status: HashMap<Token, ClientState> = HashMap::new();

    // rustls configuration
    let mut config = ServerConfig::new(NoClientAuth::new());
    let certs = load_certs("/home/ianc/rootCertificate.pem");
    let privkey = load_private_key("/home/ianc/rootPrivkey.pem");
    config.set_single_cert(certs, privkey).expect("bad certificates/private key");
    let rc_config = Arc::new(config);
    //let mut server = ServerSession::new(&rc_config);

    let addr: net::SocketAddr = "127.0.0.1:9797".parse().unwrap();
    let listener = TcpListener::bind(&addr).unwrap();

    let poll = Poll::new().unwrap();
    let mut events = Events::with_capacity(1024);

    poll.register(&listener, LISTENER, Ready::readable(), PollOpt::level()).unwrap();

    loop {
        poll.poll(&mut events, None).unwrap();

        for event in &events {
            match event.token() {
                LISTENER => {
                    match listener.accept() {
                        Ok((socket, addr)) => {
                            println!("Accepting new connection from {:?}", addr);
                            let token = Token(usize::from(addr.port()));
                            poll.register(&socket, token, Ready::readable() | Ready::writable(), PollOpt::level()).unwrap();
                            sockets.insert(token, socket);
                            tls_servers.insert(token, ServerSession::new(&rc_config));
                            client_status.insert(token, ClientState::Connected);
                        },
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                            println!("Listeneing socket would block");
                            break;
                        },
                        e => panic!("Err={:?}", e),
                    }
                },
                token => {
                    //println!("Got a token");
                    if event.readiness().is_readable() && tls_servers.get_mut(&token).unwrap().wants_read() {
                        match tls_servers.get_mut(&token).unwrap().read_tls(sockets.get_mut(&token).unwrap()) {
                            Ok(0) => {
                                // Socket is closed
                                println!("Socket closed");
                                poll.deregister(sockets.get(&token).unwrap()).unwrap();
                                sockets.remove(&token);
                                break;
                            }
                            Ok(n) => {
                                println!("read_tls: {} bytes", n);
                                // Process packets
                                match tls_servers.get_mut(&token).unwrap().process_new_packets() {
                                    Ok(_) => {
                                        let mut plaintext = Vec::new();
                                        match tls_servers.get_mut(&token).unwrap().read_to_end(&mut plaintext) {
                                            Ok(0) => (),
                                            Ok(n) => {
                                                println!("read_to_end: {}", n);
                                                println!("Got a string length: {}", plaintext.len());
                                                // Echo everything to stdout
                                                match str::from_utf8(&plaintext[0..plaintext.len()]) {
                                                    Ok(v) => {
                                                        print!("{}", v);

                                                        // Got a string from the client, process it
                                                        // based on the client state
                                                        match client_status.get(&token).unwrap() {
                                                            ClientState::Connected => {
                                                                println!("Got client name, now WaitingOnOpponent");
                                                                // Update client status to
                                                                // WaitingOnOpponent
                                                                *client_status.get_mut(&token).unwrap() =
                                                                    ClientState::WaitingOnOpponent(v.to_string());

                                                            },
                                                            ClientState::WaitingOnOpponent(name) => {
                                                                println!("Still waiting on opponent for client: {}", name);
                                                            },
                                                            ClientState::GameInProgress(partner_token) => {
                                                                println!("Send string to partner");
                                                                tls_servers.get_mut(&partner_token).unwrap().write(v.as_bytes()).unwrap();
                                                            },
                                                            _ => (),
                                                        }

                                                    }
                                                    Err(_) => panic!("invalid utf-8 sequence!"),
                                                };
                                            },
                                            Err(e) => panic!(e),
                                        }
                                    }
                                    Err(err) => panic!("Error: {}", err),
                                }
                            }
                            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                                // Socket is not ready anymore, stop reading
                                println!("Read Would block");
                            }
                            Err(ref e) if e.kind() == io::ErrorKind::ConnectionReset => {
                                // Socket is not ready anymore, stop reading
                                println!("Connection reset breaking");
                                poll.deregister(sockets.get(&token).unwrap()).unwrap();
                                sockets.remove(&token);
                                break;
                            }
                            e => panic!("err={:?}", e), // Unexpected error
                        }
                    }

                    if event.readiness().is_writable() && tls_servers.get_mut(&token).unwrap().wants_write() {

                        match tls_servers.get_mut(&token).unwrap().write_tls(sockets.get_mut(&token).unwrap()) {
                            Ok(size) => {
                                println!("Wrote {} bytes", size);
                            }
                            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                                // Socket is not ready anymore, stop reading
                                println!("Write Would block");
                            }
                            Err(ref e) if e.kind() == io::ErrorKind::ConnectionReset => {
                                // Socket is not ready anymore, stop reading
                                println!("Connection reset breaking loop");
                                break;
                            }
                            e => panic!("err={:?}", e), // Unexpected error
                        }
                    }

                    // Check to see if there are two clients we can pair up in a game
                    // See if there are any other
                    // clients we could start a game
                    // with
                    let mut partner1_token: Option<Token> = None;
                    let mut partner2_token: Option<Token> = None;
                    for (check_token, check_status) in client_status.iter() {
                        match check_status {
                            ClientState::WaitingOnOpponent(name) => {
                                // Found a client waiting for an opponent
                                if partner1_token == None {
                                    println!("Found first client who is waiting for a game: {}", name);
                                    partner1_token = Some(*check_token);
                                    continue;
                                } else if partner2_token == None {
                                    println!("Found second client who is waiting for a game: {}", name);
                                    partner2_token = Some(*check_token);
                                    break;
                                } else {
                                    unreachable!();
                                }
                            }
                            _ => (),
                        }
                    }

                    if let Some(partner1_token) = partner1_token {
                        if let Some(partner2_token) = partner2_token {
                            // Have two clients to match up in a game
                            // Update client status
                            *client_status.get_mut(&partner1_token).unwrap() = ClientState::GameInProgress(partner2_token);
                            *client_status.get_mut(&partner2_token).unwrap() = ClientState::GameInProgress(partner1_token);
                        }
                    }

                },
            }

        }
    }
}

fn load_certs(filename: &str) -> Vec<rustls::Certificate> {
    let certfile = fs::File::open(filename).expect("cannot open certificate file");
    let mut reader = BufReader::new(certfile);
    rustls::internal::pemfile::certs(&mut reader).unwrap()
}

fn load_private_key(filename: &str) -> rustls::PrivateKey {
    let rsa_keys = {
        let keyfile = fs::File::open(filename)
            .expect("cannot open private key file");
        let mut reader = BufReader::new(keyfile);
        rustls::internal::pemfile::rsa_private_keys(&mut reader)
            .expect("file contains invalid rsa private key")
    };

    let pkcs8_keys = {
        let keyfile = fs::File::open(filename)
            .expect("cannot open private key file");
        let mut reader = BufReader::new(keyfile);
        rustls::internal::pemfile::pkcs8_private_keys(&mut reader)
            .expect("file contains invalid pkcs8 private key (encrypted keys not supported)")
    };

    // prefer to load pkcs8 keys
    if !pkcs8_keys.is_empty() {
        pkcs8_keys[0].clone()
    } else {
        assert!(!rsa_keys.is_empty());
        rsa_keys[0].clone()
    }
}

