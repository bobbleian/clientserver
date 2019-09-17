use std::{net,str,thread,fs};
use std::io::{self,BufReader,Read,Write};
use std::sync::{mpsc,Arc};

use mio::{Events, Poll, Ready, PollOpt, Token, net::TcpStream};

use rustls::{ClientSession,Session};

const TALKER: Token = mio::Token(0);

fn main () {

    // rustls configuration
    let mut config = rustls::ClientConfig::new();
    let certfile = fs::File::open("C:\\Users\\ianc\\rootCertificate.pem").expect("Cannot open CA file");
    let mut reader = BufReader::new(certfile);
    config.root_store.add_pem_file(&mut reader).unwrap();
    let rc_config = Arc::new(config);
    let example_com = webpki::DNSNameRef::try_from_ascii_str("localhost").unwrap();
    let mut client = ClientSession::new(&rc_config, example_com);

    let poll = Poll::new().unwrap();


    let addr: net::SocketAddr = "127.0.0.1:97".parse().unwrap();
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
                            tx.send(buffer).unwrap();
                        },
                        Err(err) => {
                            println!("Error: {}", err);
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
                        //println!("read_line read {} bytes", n);
                        //println!("{}", buffer);
                        match client.write(buffer.as_bytes()) {
                            Ok(n) => println!("client wrote {} bytes", n),
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
                            println!("Wrote {} bytes", size);
                        }
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                            // Socket is not ready anymore, stop reading
                            println!("Would block");
                        }
                        Err(ref e) if e.kind() == io::ErrorKind::ConnectionReset => {
                            // Socket is not ready anymore, stop reading
                            println!("Connection reset breaking");
                            break 'outer;
                        }
                        e => panic!("err={:?}", e), // Unexpected error
                    }
                }

                // Handle readable event
                if event.readiness().is_readable() && client.wants_read() {
                    //match stream.read(&mut data) {
                    match client.read_tls(&mut stream) {
                        Ok(n) => {
                            println!("read_tls: {} bytes", n);
                            client.process_new_packets().unwrap();
                            // Echo everything to stdout
                            let mut plaintext = Vec::new();
                            match client.read_to_end(&mut plaintext) {
                                Ok(0) => (),
                                Ok(n) => {
                                    println!("read_to_end: {}", n);
                                    println!("Got a string length: {}", plaintext.len());
                                    // Echo everything to stdout
                                    match str::from_utf8(&plaintext[..]) {
                                        Ok(v) => {
                                            print!("{}", v);
                                        }
                                        Err(_) => panic!("invalid utf-8 sequence!"),
                                    };
                                },
                                Err(e) => panic!(e),
                            }
                        }
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                            // Socket is not ready anymore, stop reading
                            println!("Would block");
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

