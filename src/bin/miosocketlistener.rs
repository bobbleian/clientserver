use std::{str,fs};
use std::io::{self, Read, Write, BufReader};
use std::net;
use std::collections::{HashMap};
use std::sync::{Arc};

use mio::{Events, Poll, Ready, PollOpt, Token};
use mio::net::{TcpListener};

use log::{debug};

use rustls;
use rustls::{ServerConfig,ServerSession,Session,NoClientAuth};

use clientserver::GameData;

const LISTENER: Token = Token(0);

// Enumeration to store client state
enum ClientState {
    Connected,
    WaitingOnOpponent(String),
    GameInProgress(String, Token),
}

fn main () {
    // Used to store the sockets.
    let mut sockets = HashMap::new();
    let mut tls_servers = HashMap::<Token, ServerSession>::new();
    let mut games = Vec::<GameData>::new();

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
                            println!("Listening socket would block");
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
                                // Client disconnected, find partner client if it exists
                                let mut partner_token: Option<Token> = None;
                                match client_status.get(&token).unwrap() {
                                    ClientState::GameInProgress(_name, temp_partner_token) => {
                                        partner_token = Some(*temp_partner_token);
                                    },
                                    _ => (),
                                }

                                // Current client no longer has status
                                client_status.remove(&token);

                                // Clean up state of partner if necessary
                                if let Some(partner_token) = partner_token {
                                    match client_status.get(&partner_token).unwrap() {
                                        ClientState::GameInProgress(partner_name, _token) => {
                                            println!("Client disconnected, update partner: {}", partner_name);
                                            let staged_data: [u8; 2] = [0, 0];
                                            tls_servers.get_mut(&partner_token).unwrap().write(&staged_data).unwrap();
                                            *client_status.get_mut(&partner_token).unwrap() = ClientState::WaitingOnOpponent(partner_name.to_string());
                                        },
                                        _ => unreachable!(),
                                    }
                                }

                                // Socket is closed
                                println!("Socket closed");
                                poll.deregister(sockets.get(&token).unwrap()).unwrap();
                                sockets.remove(&token);
                                client_status.remove(&token);
                                break;
                            }
                            Ok(n) => {
                                println!("read_tls: {} bytes", n);
                                // Process packets
                                match tls_servers.get_mut(&token).unwrap().process_new_packets() {
                                    Ok(_) => {
                                        let mut plaintext = Vec::<u8>::new();
                                        match tls_servers.get_mut(&token).unwrap().read_to_end(&mut plaintext) {
                                            Ok(0) => (),
                                            Ok(n) => {
                                                println!("read_to_end: {}", n);
                                                println!("Got a string length: {}", plaintext.len());
                                                // Echo everything to stdout
                                                match String::from_utf8(plaintext) {
                                                    Ok(mut v) => {
                                                        println!("{}: {:?}", v, v.clone().into_bytes());

                                                        // Got a string from the client, process it
                                                        // based on the client state
                                                        match client_status.get(&token).unwrap() {
                                                            ClientState::Connected => {
                                                                println!("Got client name, now WaitingOnOpponent");
                                                                // Update client status to
                                                                // WaitingOnOpponent
                                                                *client_status.get_mut(&token).unwrap() =
                                                                    ClientState::WaitingOnOpponent(v);

                                                            },
                                                            ClientState::WaitingOnOpponent(name) => {
                                                                println!("Still waiting on opponent for client: {}", name);
                                                            },
                                                            ClientState::GameInProgress(name, partner_token) => {
                                                                // send name to partner
                                                                let buffer_data = name.as_bytes();
                                                                let mut staged_data: Vec<u8> = Vec::with_capacity(buffer_data.len() + 2);
                                                                staged_data.push(2);
                                                                staged_data.push(buffer_data.len() as u8);
                                                                staged_data.extend_from_slice(&buffer_data[..]);
                                                                tls_servers.get_mut(&partner_token).unwrap().write(&staged_data).unwrap();

                                                                // forward packet from sender
                                                                println!("Send string to partner");
                                                                tls_servers.get_mut(&partner_token).unwrap().write(v.as_bytes()).unwrap();

                                                                // Get the game data
                                                                if let Some(game_data) = games.iter_mut().find(|game| game.game_has_player(name)) {
                                                                    // Parse the player move
                                                                    match v.split_off(2).parse::<usize>() {
                                                                        Ok(player_move) => {
                                                                            // make the player move
                                                                            game_data.move_player(name, player_move);
                                                                        },
                                                                        Err(e) => { println!("Error parsing '{}': {}", v, e); }
                                                                    }
                                                                }
                                                            },
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
                    let mut partner1_name = String::new();
                    let mut partner2_token: Option<Token> = None;
                    let mut partner2_name = String::new();
                    for (check_token, check_status) in client_status.iter() {
                        match check_status {
                            ClientState::WaitingOnOpponent(name) => {
                                // Found a client waiting for an opponent
                                if partner1_token == None {
                                    debug!("Found first client who is waiting for a game: {}", name);
                                    partner1_token = Some(*check_token);
                                    partner1_name = name.to_string();
                                    continue;
                                } else if partner2_token == None {
                                    debug!("Found second client who is waiting for a game: {}", name);
                                    partner2_token = Some(*check_token);
                                    partner2_name = name.to_string();
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
                            if let Some(partner1_status) = client_status.get_mut(&partner1_token) {
                                *partner1_status = ClientState::GameInProgress(partner1_name.to_string(), partner2_token);
                            }
                            if let Some(partner2_status) = client_status.get_mut(&partner2_token) {
                                *partner2_status = ClientState::GameInProgress(partner2_name.to_string(), partner1_token);
                            }

                            // Build a new Game object
                            let mut game_data = GameData::new(2, 3, 10);
                            game_data.add_player(&partner1_name);
                            game_data.add_player(&partner2_name);

                            // Add the Game object to the global store
                            games.push(game_data);

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

