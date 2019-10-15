use std::{str,fs};
use std::io::{self, Read, Write, BufReader};
use std::net;
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
    let mut message_queue = Vec::<(usize, Vec::<u8>)>::new();

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
                            // Send a Welcome message
                            // player_id (token)
                            let mut welcome_message: Vec<u8> = Vec::with_capacity(3);
                            welcome_message.push(8);
                            welcome_message.push(1);
                            welcome_message.push(usize::from(token) as u8);
                            message_queue.push((usize::from(token), welcome_message));

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
                    let mut socket_data = &mut sockets.get_mut(usize::from(token)).unwrap();
                    if event.readiness().is_readable() && socket_data.session.wants_read() {
                        match socket_data.session.read_tls(&mut socket_data.socket) {
                            Ok(0) => {
                                // Client disconnected, find partner client if it exists
                                let mut partner_token: Option<Token> = None;
                                match socket_data.state {
                                    ClientState::GameInProgress(temp_partner_token) => {
                                        partner_token = Some(temp_partner_token);
                                    },
                                    _ => (),
                                }

                                // Clean up state of partner if necessary
                                if let Some(partner_token) = partner_token {
                                    match sockets.get(usize::from(partner_token)).unwrap().state {
                                        ClientState::GameInProgress(_token) => {
                                            println!("Client disconnected, update partner");
                                            message_queue.push((usize::from(partner_token), [0, 0].to_vec()));
                                            sockets.get_mut(usize::from(partner_token)).unwrap().state = ClientState::WaitingOnOpponent;
                                        },
                                        _ => unreachable!(),
                                    }
                                }

                                // Socket is closed
                                println!("Socket closed");
                                poll.deregister(& sockets.get(usize::from(token)).unwrap().socket).unwrap();
                                sockets.remove(usize::from(token));

                                // Remove/Update GameData
                                games.retain(|game| {
                                    !game.game_has_player(usize::from(token))
                                });
                                break;
                            }
                            Ok(n) => {
                                println!("read_tls: {} bytes", n);
                                // Process packets
                                match socket_data.session.process_new_packets() {
                                    Ok(_) => {
                                        let mut data = Vec::<u8>::new();
                                        match socket_data.session.read_to_end(&mut data) {
                                            Ok(0) => (),
                                            Ok(1) => unreachable!(),
                                            Ok(n) => {
                                                debug!("read_to_end: {}", n);
                                                debug!("Got a data: {:?}", data);

                                                // process the data
                                                let mut i = 0;
                                                while i < data.len() {
                                                    process_client_data(data[i], data[i+1], &data[i+2..(i+2+(data[i+1] as usize))], token, &mut socket_data, &mut games, &mut message_queue);

                                                    i = i + 2 + data[i+1] as usize;
                                                    debug!("Incremented i: {}", i);
                                                }
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
                    for (check_token, check_socket_data) in sockets.iter() {
                        match check_socket_data.state {
                            ClientState::WaitingOnOpponent => {
                                // Found a client waiting for an opponent
                                if partner1_token == None {
                                    debug!("Found first client who is waiting for a game");
                                    partner1_token = Some(Token::from(check_token));
                                    continue;
                                } else if partner2_token == None {
                                    debug!("Found second client who is waiting for a game");
                                    partner2_token = Some(Token::from(check_token));
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
                            sockets.get_mut(usize::from(partner1_token)).unwrap().state = ClientState::GameInProgress(partner2_token);
                            sockets.get_mut(usize::from(partner2_token)).unwrap().state = ClientState::GameInProgress(partner1_token);

                            // Build a new Game object
                            let mut game_data = GameData::new(2, 3, 10);
                            game_data.add_player(usize::from(partner1_token), &partner1_name);
                            game_data.add_player(usize::from(partner2_token), &partner2_name);

                            // Send GameData message to clients:
                            // max_players, max_move, game_board_size
                            let mut game_data_message = Vec::<u8>::with_capacity(5);
                            game_data_message.extend_from_slice(&[4,3,2,3,10]);
                            message_queue.push((usize::from(partner1_token), game_data_message.clone()));
                            message_queue.push((usize::from(partner2_token), game_data_message));

                            // Send Add_Player messages to both clients
                            let mut add_player1_message = Vec::<u8>::new();
                            add_player1_message.push(5);
                            add_player1_message.push((partner1_name.as_bytes().len() + 1) as u8);
                            add_player1_message.push(usize::from(partner1_token) as u8);
                            add_player1_message.extend_from_slice(partner1_name.as_bytes());

                            let mut add_player2_message = Vec::<u8>::new();
                            add_player2_message.push(5);
                            add_player2_message.push((partner2_name.as_bytes().len() + 1) as u8);
                            add_player2_message.push(usize::from(partner2_token) as u8);
                            add_player2_message.extend_from_slice(partner2_name.as_bytes());

                            message_queue.push((usize::from(partner1_token), add_player1_message.clone()));
                            message_queue.push((usize::from(partner2_token), add_player1_message));

                            message_queue.push((usize::from(partner1_token), add_player2_message.clone()));
                            message_queue.push((usize::from(partner2_token), add_player2_message));

                            // Send Set_Active_Player message to both clients
                            // player_id
                            let mut set_active_player_message = Vec::<u8>::with_capacity(3);
                            set_active_player_message.push(7);
                            set_active_player_message.push(1);
                            set_active_player_message.push(game_data.get_active_player_id());

                            message_queue.push((usize::from(partner1_token), set_active_player_message.clone()));
                            message_queue.push((usize::from(partner2_token), set_active_player_message));

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

fn process_client_data(control_byte: u8, data_len: u8, data: &[u8], token: Token, socket_data: &mut SocketData, games: &mut Vec<GameData>, message_queue: &mut Vec::<(usize, Vec::<u8>)>) {
    println!("Processing [control_byte: {}; data_len: {}; data: {:?}]", control_byte, data_len, data);
    match control_byte {

        // 0: User_Name Message
        // control_byte: 0
        // data_len: n
        // data[..] = user_name
        0 => {
            let v = str::from_utf8(data).unwrap().to_string();
            println!("{}: {:?}", v, v.clone().into_bytes());

            // Got a string from the client, process it
            // based on the client state
            match socket_data.state {
                // Only process when client is in Connected state
                ClientState::Connected => {
                    println!("Got client name, now WaitingOnOpponent");
                    // Update client status to
                    // WaitingOnOpponent
                    socket_data.state = ClientState::WaitingOnOpponent;
                    socket_data.player_name = v;

                },
                _ => {
                    // Do nothing
                }
            }
        },

        // 1: Player_Move message
        // control_byte: 1
        // data_len: 1
        // data[0]: player_move
        1 => {
            match socket_data.state {
                ClientState::GameInProgress(partner_token) => {
                    // Get the game data
                    if let Some(game_data) = games.iter_mut().find(|game| game.game_has_player(usize::from(token))) {
                        // make the player move
                        let player_move = data[0];
                        game_data.move_player(usize::from(token), player_move);

                        // send the game board
                        let game_board = game_data.get_game_board();
                        println!("Sending board update: {:?}", game_board);
                        let mut s_data: Vec<u8> = Vec::with_capacity(game_board.len() + 2);
                        s_data.push(3);
                        s_data.push(game_board.len() as u8);
                        s_data.extend_from_slice(&game_board[..]);
                        message_queue.push((usize::from(partner_token), s_data.clone()));
                        message_queue.push((usize::from(token), s_data.clone()));

                        // Send Move_Player message
                        let mut move_player_message: Vec<u8> = Vec::with_capacity(4);
                        move_player_message.push(6);
                        move_player_message.push(2);
                        move_player_message.push(usize::from(token) as u8);
                        move_player_message.push(player_move);

                        message_queue.push((usize::from(partner_token), move_player_message.clone()));
                        message_queue.push((usize::from(token), move_player_message));
                    }
                },

                // Do nothing for any state other than GameInProgress
                _ => { }
            }
        },

        // 2: Restart_Game message
        // control_byte: 2
        // data_len: 0
        2 => {
            match socket_data.state {
                ClientState::GameInProgress(partner_token) => {
                    // Get the game data
                    if let Some(game_data) = games.iter_mut().find(|game| game.game_has_player(usize::from(token))) {

                        // Ensure game is over
                        if game_data.is_game_over() {
                            // Restart request, add_player
                            game_data.add_player(usize::from(token), "");

                            // Send Add_Player messages to both clients
                            let mut add_player_message = Vec::<u8>::new();
                            add_player_message.push(5);
                            add_player_message.push(1);
                            add_player_message.push(usize::from(token) as u8);

                            message_queue.push((usize::from(partner_token), add_player_message.clone()));
                            message_queue.push((usize::from(token), add_player_message));

                        } else {
                        }
                    } else {
                        // TODO: No game to restart
                    }
                },

                // Do nothing for any state other than GameInProgress
                _ => { }
            }
        },


        // Unknown control byte; do nothing?
        unknown => {
            println!("Unknown control byte: {}", unknown);
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

