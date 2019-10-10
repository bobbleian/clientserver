use std::{net,str,thread,fs};
use std::io::{self,BufReader,Read,Write};
use std::sync::{mpsc,Arc};

use log::{debug, warn};

use mio::{Events, Poll, Ready, PollOpt, Token, net::TcpStream};

use clientserver::game::GameData;

use rustls::{ClientSession,Session};
use console::Term;

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

    let mut game_data: Option<GameData> = None;

    let poll = Poll::new().unwrap();

    let addr: net::SocketAddr = "127.0.0.1:9797".parse().unwrap();
    match TcpStream::connect(&addr) {
        Ok(mut stream) => {
            // Spawn thread to read user input
            let (tx, rx) = mpsc::channel();

            thread::spawn(move || {
                loop {
                    let stdin = io::stdin();
                    let mut buffer = String::new();


                    match stdin.read_line(&mut buffer) {
                        Ok(_n) => {
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

            let mut user_name = String::new();
            let mut update_user_prompt = true;

            'outer: loop {

                // Display user-prompt
                if update_user_prompt {
                    if user_name.is_empty() {
                        print!("Please enter user name: ");
                    } else {
                        // Echo board to stdout
                        let term = Term::stdout();
                        term.clear_screen().unwrap();
                        println!("Player: {}", user_name);
                        if let Some(ref game_data) = game_data {
                            println!("Max Move: {}", game_data.get_max_move());
                            println!("Game Board: {:?} ", game_data.get_game_board());
                            println!("Game Total: {} ", game_data.get_game_board().len());
                        }
                        print!("> ");
                    }
                    io::stdout().flush().unwrap();
                    update_user_prompt = false;
                }

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
                        if user_name.is_empty() {
                            user_name = buffer.clone().trim().to_string();
                        }
                        update_user_prompt = true;
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
                                    debug!("Got a data: {:?}", data);

                                    // process the data
                                    let mut i = 0;
                                    while i < data.len() {
                                        game_data = process_server_data(data[i], data[i+1], &data[i+2..(i+2+(data[i+1] as usize))], game_data);
                                        i = i + 2 + data[i+1] as usize;
                                        debug!("Incremented i: {}", i);
                                    }
                                    update_user_prompt = true;
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


fn process_server_data(control_byte: u8, data_len: u8, data: &[u8], game_data: Option<GameData>) -> Option<GameData> {
    println!("Processing [control_byte: {}; data_len: {}; data: {:?}]", control_byte, data_len, data);
    match control_byte {
        // 0: Opponent has disconnected
        0 => {
            println!("Your chat partner has ended the conversation...");
            None
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
            game_data
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
            game_data
        },

        // 3: Game Board update
        3 => {
            //game_board.clear();
            //game_board.extend_from_slice(data);
            game_data
        },

        // 3: Game Data update
        4 => {
            // Game Data only contains 3 fields:
            // max_players
            // max_move
            // game_board_size
            match data.len() {
                3 => {
                    Some(GameData::new(data[0], data[1], data[2]))
                },
                _ => unreachable!(),
            }
        },

        // Unknown control byte; do nothing?
        _ => {
            println!("Unknown control byte");
            game_data
        }
    }
}
