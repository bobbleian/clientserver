use std::{net,str,thread,fs};
use std::io::{self,BufReader,Read,Write};
use std::sync::{mpsc,Arc};

use log::{debug, warn};

use mio::{Events, Poll, Ready, PollOpt, Token, net::TcpStream};

use clientserver::game::GameData;

use rustls::{ClientSession,Session};
use console::{Term, style, Style};

use dirs::home_dir;

const TALKER: Token = mio::Token(0);

fn main () {



    // rustls configuration
    let mut config = rustls::ClientConfig::new();
    let mut path_buf = home_dir().unwrap();
    path_buf.push("ca.cheese.crt.pem");

    let certfile = fs::File::open(path_buf).expect("Cannot open CA file");
    let mut reader = BufReader::new(certfile);
    config.root_store.add_pem_file(&mut reader).unwrap();
    let rc_config = Arc::new(config);
    let example_com = webpki::DNSNameRef::try_from_ascii_str("localhost").unwrap();
    let mut client = ClientSession::new(&rc_config, example_com);

    let mut game_data: Option<GameData> = None;
    let mut user_name = String::new();
    let mut user_id = std::usize::MAX;

    let winning_player_style = Style::new().green().blink().reverse();
    let losing_player_style = Style::new().red().blink().reverse();
    let active_player_style = Style::new().green();
    let inactive_player_style = Style::new().red();

    let poll = Poll::new().unwrap();

    let addr: net::SocketAddr = "192.168.7.67:9797".parse().unwrap();
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

            let mut update_user_prompt = true;
            let term = Term::stdout();

            'outer: loop {

                // Display user-prompt
                if update_user_prompt {
                    term.clear_screen().unwrap();
                    if user_name.is_empty() {
                        print!("Please enter user name: ");
                    } else {
                        // Echo board to stdout
                        if let Some(ref mut game_data) = game_data {
                            if game_data.is_game_over() {
                                if game_data.get_active_player_id() == user_id as u8 {
                                    println!("Player: {} [{}]", losing_player_style.apply_to(user_name.clone()), user_id);
                                } else {
                                    println!("Player: {} [{}]", winning_player_style.apply_to(user_name.clone()), user_id);
                                }
                            } else {
                                if game_data.get_active_player_id() == user_id as u8 {
                                    println!("Player: {} [{}]", active_player_style.apply_to(user_name.clone()), user_id);
                                } else {
                                    println!("Player: {} [{}]", inactive_player_style.apply_to(user_name.clone()), user_id);
                                }
                            }
                            println!("Active Player: {}", game_data.get_active_player_id());
                            println!("Max Move: {}", game_data.get_max_move());
                            println!("Game Board: {:?} ", game_data.get_game_board());
                            println!("Game Total: {} ", game_data.get_game_board().len());
                            println!("Game Players: {:?} ", game_data.get_player_names());
                            if game_data.is_game_over() {
                                print!("Play again (yes/no)? ");
                            } else if game_data.get_active_player_id() == user_id as u8 {
                                print!("Enter next move (1,2,3) ");
                            } else {
                                println!("Waiting for other player to move");
                            }
                        } else {
                            println!("Player: {} [{}]", style(user_name.clone()).blue(), user_id);
                            println!("Status: Waiting for opponent");
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

                        // Check to see if we have a user name
                        if user_name.is_empty() {
                            // Set user name in the client
                            user_name = buffer.clone().trim().to_string();

                            // Send User_Name message
                            // control_byte: 0
                            // data_len: n
                            // data[0..n] = user name
                            let mut user_name_message: Vec<u8> = Vec::with_capacity(buffer_data.len() + 2);
                            user_name_message.push(0);
                            user_name_message.push(buffer_data.len() as u8);
                            user_name_message.extend_from_slice(&buffer_data[..]);
                            client.write(&user_name_message).unwrap();
                        } else {
                            if let Some(ref mut game_data) = game_data {
                                if game_data.is_game_over() {
                                    if "yes".eq_ignore_ascii_case(&buffer) {
                                        // Send Restart_Game message
                                        // control_byte: 2
                                        // data_len: 0
                                        let restart_game_message: Vec<u8> = [2, 0].to_vec();
                                        client.write(&restart_game_message).unwrap();
                                    } else if "no".eq_ignore_ascii_case(&buffer) {
                                        // Send End_Game message
                                        // control_byte: 3
                                        // data_len: 0
                                        let end_game_message: Vec<u8> = [3, 0].to_vec();
                                        client.write(&end_game_message).unwrap();
                                    }
                                } else {
                                    // Have a game, client has entered a move
                                    // Parse the player move
                                    match buffer.parse::<u8>() {
                                        Ok(player_move) => {
                                            // Send Player_Move message
                                            // control_byte: 1
                                            // data_len: 1
                                            // data[0]: player_move
                                            let mut player_move_message: Vec<u8> = Vec::with_capacity(3);
                                            player_move_message.push(1);
                                            player_move_message.push(1);
                                            player_move_message.push(player_move);
                                            client.write(&player_move_message).unwrap();
                                        },
                                        Err(e) => { println!("Cannot parse player move '{}': {}", buffer, e); }
                                    }
                                }
                            }
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
                                        process_server_data(data[i], data[i+1], &data[i+2..(i+2+(data[i+1] as usize))], &mut game_data, &mut user_id);
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


fn process_server_data(control_byte: u8, data_len: u8, data: &[u8], game_data: &mut Option<GameData>, user_id: &mut usize) {
    println!("Processing [control_byte: {}; data_len: {}; data: {:?}]", control_byte, data_len, data);
    match control_byte {
        // 0: Opponent_Disconnect message
        0 => {
            println!("Your chat partner has ended the conversation...");

            // Get rid of the game
            *game_data = None;
        },

        // 1: New message from opponent (TODO - chat?)
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

        // 2: Partner name message (legacy)
        2 => {
            // Echo everything to stdout
            match str::from_utf8(data) {
                Ok(v) => {
                    print!("[{}] ", v);
                }
                Err(_) => panic!("invalid utf-8 sequence!"),
            };
        },

        // 3: Game Board update (legacy)
        3 => {
            //game_board.clear();
            //game_board.extend_from_slice(data);
        },

        // 4: Game Data update
        4 => {
            // Game Data only contains 3 fields:
            // max_players
            // max_move
            // game_board_size
            match data.len() {
                3 => {
                    *game_data = Some(GameData::new(data[0], data[1], data[2]))
                },
                _ => unreachable!(),
            }
        },

        // 5: Add_Player message
        // data[0] - player id
        // data[1..] - player name
        5 => {
            // Process add player only if we already have a GameData struct
            if let Some(ref mut game_data) = game_data {
                let player_id = data[0] as usize;
                let player_name = str::from_utf8(&data[1..]).unwrap();
                game_data.add_player(player_id, player_name);
            }
        },

        // 6: Move_Player message
        // data[0] - player_id
        // data[1] - player_move
        6 => {
            // Process add player only if we already have a GameData struct
            if let Some(ref mut game_data) = game_data {
                let player_id = data[0] as usize;
                let player_move = data[1];
                game_data.move_player(player_id, player_move);
            }
        },

        // 7: Set_Active_Player message
        // data[0] - player_id
        7 => {
            // Process set active player only if we already have a GameData struct
            if let Some(ref mut game_data) = game_data {
                let player_id = data[0] as usize;
                game_data.set_active_player(player_id);
            }
        },

        // 8: Welcome message
        // data[0] - player_id
        8 => {
            // Set user_id to player_id
            *user_id = data[0] as usize;
        },


        // Unknown control byte; do nothing?
        unknown => {
            println!("Unknown control byte: {}", unknown);
        }
    }
}
