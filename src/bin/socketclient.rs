use std::{io,fs,};
use std::net::TcpStream;
use std::io::{Write};
use std::sync::{Arc};
use std::io::{BufReader};

use rustls::{ClientSession,Session};

fn main () {
    let mut config = rustls::ClientConfig::new();
    let certfile = fs::File::open("C:\\Users\\ianc\\rootCertificate.pem").expect("Cannot open CA file");
    let mut reader = BufReader::new(certfile);
    config.root_store.add_pem_file(&mut reader).unwrap();
    let rc_config = Arc::new(config);
    let example_com = webpki::DNSNameRef::try_from_ascii_str("localhost").unwrap();
    let mut client = ClientSession::new(&rc_config, example_com);

    match TcpStream::connect("127.0.0.1:97") {
        Ok(mut stream) => {
            /*
            loop {
                if client.wants_read() {
                    println!("Client wants read");
                    client.read_tls(&mut stream).unwrap();
                    client.process_new_packets().unwrap();

                    let mut plaintext = Vec::new();
                    client.read_to_end(&mut plaintext).unwrap();
                }


                println!("Client sleeping 1000ms");
                thread::sleep(time::Duration::from_millis(1000));
            }
            */

            loop {
                let stdin = io::stdin();
                let mut buffer = String::new();

                match stdin.read_line(&mut buffer) {
                    Ok(_n) => {
                        //println!("read_line read {} bytes", n);
                        //println!("{}", buffer);
                        //match stream.write(buffer.as_bytes()) {
                        match client.write(buffer.as_bytes()) {
                            Ok(_n) => {
                                if client.wants_write() {
                                    println!("Client wants write");
                                    match client.write_tls(&mut stream) {
                                        Ok(n) => println!("write_tls: wrote {} bytes", n),
                                        Err(err) => panic!("Error: {}", err),
                                    }
                                }
                            },
                            Err(err) => {
                                println!("Error: {}", err);
                                break;
                            },
                        }
                    },
                    Err(err) => {
                        println!("Error: {}", err);
                        break;
                    },
                }

                if client.wants_read() {
                    println!("Client wants read");
                    match client.read_tls(&mut stream) {
                        Ok(n) => println!("read_tls: read {} bytes", n),
                        Err(err) => panic!("Error: {}", err),
                    }
                    client.process_new_packets().unwrap();
                }

            }
        },
        Err(_) => println!("Couldn't connect to server..."),
    }
}

