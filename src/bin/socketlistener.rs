use std::{str,thread,fs,time};
use std::io::{Read,BufReader};
use std::net::{TcpListener,TcpStream,SocketAddr};
use std::sync::{Arc};

use rustls::{ServerConfig,ServerSession,Session,NoClientAuth};

fn main () {
    let listener = TcpListener::bind("127.0.0.1:97").unwrap();

    loop {
        match listener.accept() {
            Ok((stream, addr)) => { thread::spawn(move || { handle_client(stream, addr); }); },
            Err(e) => println!("Error: {}", e),
        }
    }
}

fn handle_client (mut stream: TcpStream, address: SocketAddr) {

    println!("New client address: {:?}", address);

    let mut config = ServerConfig::new(NoClientAuth::new());
    let certs = load_certs("C:\\Users\\ianc\\serverCertificateChain.pem");
    let privkey = load_private_key("C:\\Users\\ianc\\serverPrivKey.pem");
    config.set_single_cert(certs, privkey).expect("bad certificates/private key");
    let rc_config = Arc::new(config);
    let mut server = ServerSession::new(&rc_config);

    loop {
        if server.wants_read() {
            println!("Server wants read");
            match server.read_tls(&mut stream) {
                Ok(n) => println!("read_tls: read {} bytes", n),
                Err(err) => panic!("Error: {}", err),
            }
            match server.process_new_packets() {
                Ok(_) => println!("Server Processed new packets"),
                Err(err) => panic!("Error: {}", err),
            }

            let mut plaintext: Vec<u8> = Vec::new();
            server.read_to_end(&mut plaintext).unwrap();
            println!("Got a string length: {}", plaintext.len());
            // Echo everything to stdout
            match str::from_utf8(&plaintext[..]) {
                Ok(v) => println!("{}", v),
                Err(_) => panic!("invalid utf-8 sequence!"),
            };
        }

        if server.wants_write() {
            println!("Server wants write");
            match server.write_tls(&mut stream) {
                Ok(n) => println!("write_tls: wrote {} bytes", n),
                Err(err) => panic!("Error: {}", err),
            }
        }

        println!("Server sleeping 1000ms");
        thread::sleep(time::Duration::from_millis(1000));
    }


    /* Dumb socket listener
    let mut data = [0; 2]; //use a 2 byte buffer
    while match stream.read(&mut data) {
        Ok(size) => {
            // Echo everything to stdout
            match str::from_utf8(&data[0..size]) {
                Ok(v) => print!("{}", v),
                Err(_) => panic!("invalid utf-8 sequence!"),
            };
            true
        },
        Err(_) => {
            println!("Error occurred, closing socket");
            stream.shutdown(Shutdown::Both).unwrap();
            false
        }
    } {}
    */
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
