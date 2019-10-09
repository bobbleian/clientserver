use std::{str,fs};
use std::io::{self, Read, Write, BufReader};
use std::net;
use std::collections::{HashMap};
use std::sync::{Arc};

use mio::{Events, Poll, Ready, PollOpt, Token};
use mio::net::{TcpListener, TcpStream};

use log::{debug};

use rustls;
use rustls::{ServerConfig,ServerSession,Session,NoClientAuth};

use clientserver::game::GameData;

use slab::Slab;

const MAX_SOCKETS: usize = 1024;
const LISTENER: Token = Token(MAX_SOCKETS);

// Enumeration to store client state
enum ClientState {
    Connected,
    WaitingOnOpponent,
    GameInProgress(Token),
}

struct SocketData {
    player_name: String,
    socket: TcpStream,
    session: ServerSession,
    state: ClientState,
}

fn main () {
    // Used to store the sockets.
    let mut sockets: Slab<SocketData> = Slab::with_capacity(MAX_SOCKETS);
    let mut games: Vec<GameData> = Vec::new();

    // Used to store client status
    let mut client_status: HashMap<Token, ClientState> = HashMap::new();

    // rustls configuration
    let mut config = ServerConfig::new(NoClientAuth::new());
    let certs = load_certs("/home/ianc/rootCertificate.pem");
    let privkey = load_private_key("/home/ianc/rootPrivkey.pem");
    config.set_single_cert(certs, privkey).expect("bad certificates/private key");
    let rc_config = Arc::new(config);

    let addr: net::SocketAddr = "127.0.0.1:9797".parse().unwrap();
    let listener = TcpListener::bind(&addr).unwrap();

    let poll = Poll::new().unwrap();
    let mut events = Events::with_capacity(1024);

    poll.register(&listener, LISTENER, Ready::readable(), PollOpt::level()).unwrap();

    // Outgoing message queue
    let message_queue = &mut Vec::new();

    loop {

        poll.poll(&mut events, None).unwrap();

        for event in &events {
            match event.token() {
                LISTENER => {
                    match listener.accept() {
                        Ok((socket, addr)) => {
                            println!("Accepting new connection from {:?}", addr);
                            // check max connections
                            if sockets.len() >= MAX_SOCKETS {
                                println!("Max connections reached {}" , MAX_SOCKETS);
                                socket.shutdown(net::Shutdown::Both).unwrap();
                                break;
                            }

                            let socket_entry = sockets.vacant_entry();
                            let token = Token(socket_entry.key());
                            poll.register(&socket, token, Ready::readable() | Ready::writable(), PollOpt::level()).unwrap();
                            socket_entry.insert(SocketData{player_name: String::from(""), socket: socket, session: ServerSession::new(&rc_config), state: ClientState::Connected});
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
                    let socket_data = &mut sockets.get_mut(usize::from(token)).unwrap();
                    if event.readiness().is_readable() && socket_data.session.wants_read() {
                        match socket_data.session.read_tls(&mut socket_data.socket) {
                            Ok(0) => {
                                // Client disconnected, find partner client if it exists
                                let mut partner_token: Option<Token> = None;
                                match client_status.get(&token).unwrap() {
                                    ClientState::GameInProgress(temp_partner_token) => {
                                        partner_token = Some(*temp_partner_token);
                                    },
                                    _ => (),
                                }

                                // Current client no longer has status
                                client_status.remove(&token);

                                // Clean up state of partner if necessary
                                if let Some(partner_token) = partner_token {
                                    match client_status.get(&partner_token).unwrap() {
                                        ClientState::GameInProgress(_token) => {
                                            println!("Client disconnected, update partner");
                                            let staged_data: [u8; 2] = [0, 0];
                                            sockets.get_mut(usize::from(partner_token)).unwrap().session.write(&staged_data).unwrap();
                                            sockets.get_mut(usize::from(partner_token)).unwrap().state = ClientState::WaitingOnOpponent;
                                            *client_status.get_mut(&partner_token).unwrap() = ClientState::WaitingOnOpponent;
                                        },
                                        _ => unreachable!(),
                                    }
                                }

                                // Socket is closed
                                println!("Socket closed");
                                poll.deregister(& sockets.get(usize::from(token)).unwrap().socket).unwrap();
                                sockets.remove(usize::from(token));
                                client_status.remove(&token);
                                break;
                            }
                            Ok(n) => {
                                println!("read_tls: {} bytes", n);
                                // Process packets
                                match socket_data.session.process_new_packets() {
                                    Ok(_) => {
                                        let mut plaintext = Vec::<u8>::new();
                                        match socket_data.session.read_to_end(&mut plaintext) {
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
                                                                    ClientState::WaitingOnOpponent;
                                                                socket_data.state = ClientState::WaitingOnOpponent;
                                                                socket_data.player_name = v.to_string();

                                                            },
                                                            ClientState::WaitingOnOpponent => {
                                                                println!("Still waiting on opponent for client: {:?}", token);
                                                            },
                                                            ClientState::GameInProgress(partner_token) => {
                                                                // send name to partner
                                                                let buffer_data = socket_data.player_name.as_bytes();
                                                                let mut staged_data: Vec<u8> = Vec::with_capacity(buffer_data.len() + 2);
                                                                staged_data.push(2);
                                                                staged_data.push(buffer_data.len() as u8);
                                                                staged_data.extend_from_slice(&buffer_data[..]);
                                                                message_queue.push((usize::from(*partner_token), staged_data.clone()));

                                                                // forward packet from sender
                                                                println!("Send string to partner");
                                                                message_queue.push((usize::from(*partner_token), v.as_bytes().to_vec()));

                                                                // Get the game data
                                                                if let Some(game_data) = games.iter_mut().find(|game| game.game_has_player(usize::from(token))) {
                                                                    // Parse the player move
                                                                    match v.split_off(2).parse::<u8>() {
                                                                        Ok(player_move) => {
                                                                            // make the player move
                                                                            game_data.move_player(usize::from(token), player_move);

                                                                            // send the game board
                                                                let game_board = game_data.get_game_board();
                                                                            println!("Sending board update: {:?}", game_board);
                                                                let mut s_data: Vec<u8> = Vec::with_capacity(game_board.len() + 2);
                                                                s_data.push(3);
                                                                s_data.push(game_board.len() as u8);
                                                                s_data.extend_from_slice(&game_board[..]);
                                                                message_queue.push((usize::from(*partner_token), s_data.clone()));
                                                                message_queue.push((usize::from(token), s_data.clone()));
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
                                poll.deregister(& sockets.get(usize::from(token)).unwrap().socket).unwrap();
                                sockets.remove(usize::from(token));
                                break;
                            }
                            e => panic!("err={:?}", e), // Unexpected error
                        }
                    }

                    if event.readiness().is_writable() && socket_data.session.wants_write() {

                        match socket_data.session.write_tls(&mut socket_data.socket) {
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
                            ClientState::WaitingOnOpponent => {
                                // Found a client waiting for an opponent
                                if partner1_token == None {
                                    debug!("Found first client who is waiting for a game");
                                    partner1_token = Some(*check_token);
                                    continue;
                                } else if partner2_token == None {
                                    debug!("Found second client who is waiting for a game");
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
                            let partner1_name = sockets.get_mut(usize::from(partner1_token)).unwrap().player_name.to_string();
                            let partner2_name = sockets.get_mut(usize::from(partner2_token)).unwrap().player_name.to_string();
                            // Update client status
                            if let Some(partner1_status) = client_status.get_mut(&partner1_token) {
                                *partner1_status = ClientState::GameInProgress(partner2_token);
                                sockets.get_mut(usize::from(partner1_token)).unwrap().state = ClientState::GameInProgress(partner2_token);
                            }
                            if let Some(partner2_status) = client_status.get_mut(&partner2_token) {
                                *partner2_status = ClientState::GameInProgress(partner1_token);
                                sockets.get_mut(usize::from(partner2_token)).unwrap().state = ClientState::GameInProgress(partner1_token);
                            }

                            // Build a new Game object
                            let mut game_data = GameData::new(2, 3, 10);
                            game_data.add_player(usize::from(partner1_token), &partner1_name);
                            game_data.add_player(usize::from(partner2_token), &partner2_name);

                            // Add the Game object to the global store
                            games.push(game_data);

                        }
                    }

                    // Clear out message queue
                    message_queue.retain(|message| {
                        println!("Message: [token={}; data={:?}", message.0, message.1);
                        sockets.get_mut(message.0).unwrap().session.write(message.1.as_slice()).unwrap();
                        false
                    });

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

