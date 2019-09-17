use std::{str,fs};
use std::io;
use std::io::{Read, Write};
use std::net;
use std::collections::HashMap;
use std::io::{BufReader};
use std::sync::{Arc};

use mio::{Events, Poll, Ready, PollOpt, Token};
use mio::net::{TcpListener};

use rustls;
use rustls::{ServerConfig,ServerSession,Session,NoClientAuth};

const LISTENER: Token = Token(0);

fn main () {
    // Used to store the sockets.
    let mut sockets = HashMap::new();
    let mut tls_servers = HashMap::new();
    let mut pending_data = HashMap::new();

    // rustls configuration
    let mut config = ServerConfig::new(NoClientAuth::new());
    let certs = load_certs("C:\\Users\\ianc\\rootCertificate.pem");
    let privkey = load_private_key("C:\\Users\\ianc\\rootPrivkey.pem");
    config.set_single_cert(certs, privkey).expect("bad certificates/private key");
    let rc_config = Arc::new(config);
    //let mut server = ServerSession::new(&rc_config);

    let addr: net::SocketAddr = "127.0.0.1:97".parse().unwrap();
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
                            pending_data.insert(token, Vec::<String>::new());
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

                                                        // Add string to pending data queue to be send back to all
                                                        // connected clients
                                                        for servers in tls_servers.values_mut() {
                                                            servers.write(v.as_bytes()).unwrap();
                                                        }
                                                        for val in pending_data.values_mut() {
                                                            val.push(v.to_string());
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

                        /* Deal with this later
                        if let Some(data) = pending_data.get_mut(&token) {
                            if !data.is_empty() {
                            println!("Writing back to stream");
                            match sockets.get_mut(&token).unwrap().write(data.remove(0).as_bytes()) {
                                Ok(size) => {
                                    println!("Wrote {} bytes", size);
                                    break;
                                }
                                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                                    // Socket is not ready anymore, stop reading
                                    println!("Would block, breaking loop");
                                    break;
                                }
                                Err(ref e) if e.kind() == io::ErrorKind::ConnectionReset => {
                                    // Socket is not ready anymore, stop reading
                                    println!("Connection reset");
                                    break;
                                }
                                e => panic!("err={:?}", e), // Unexpected error
                            }
                            }
                        }
                        */
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

