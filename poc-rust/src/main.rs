use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

fn main() {
    // Test 1: Raw IPv4 listen + dial (same as Go diagnostic)
    println!("=== Rust IPv4 TCP Test ===");

    let raw_ln = TcpListener::bind("127.0.0.1:0").expect("raw listen failed");
    let raw_addr = raw_ln.local_addr().unwrap();
    println!("raw listener on {}", raw_addr);

    let handle = thread::spawn(move || {
        match TcpStream::connect_timeout(&raw_addr, Duration::from_secs(5)) {
            Ok(mut c) => {
                let mut buf = [0u8; 16];
                let n = c.read(&mut buf).unwrap_or(0);
                println!("raw dial OK, got {:?}", std::str::from_utf8(&buf[..n]));
            }
            Err(e) => println!("raw dial FAILED: {}", e),
        }
    });

    match raw_ln.accept() {
        Ok((mut c, addr)) => {
            println!("raw accept OK from {}", addr);
            let _ = c.write_all(b"hello");
        }
        Err(e) => println!("raw accept FAILED: {}", e),
    }
    handle.join().unwrap();
    drop(raw_ln);

    // Test 2: HTTP server on IPv4
    println!("\n=== HTTP server on 0.0.0.0:8275 ===");
    let http_ln = match TcpListener::bind("0.0.0.0:8275") {
        Ok(ln) => {
            println!("HTTP listen OK on {}", ln.local_addr().unwrap());
            ln
        }
        Err(e) => {
            println!("HTTP listen FAILED: {}", e);
            return;
        }
    };

    // Self-test: connect to our own HTTP listener
    thread::spawn(|| {
        thread::sleep(Duration::from_secs(1));
        match TcpStream::connect_timeout(
            &"127.0.0.1:8275".parse().unwrap(),
            Duration::from_secs(5),
        ) {
            Ok(_) => println!("HTTP self-test dial OK"),
            Err(e) => println!("HTTP self-test dial FAILED: {}", e),
        }
    });

    println!("Waiting for connections (Ctrl+C to stop)...");
    for stream in http_ln.incoming() {
        match stream {
            Ok(mut s) => {
                let peer = s.peer_addr().unwrap();
                println!("connection from {}", peer);
                let response = "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 19\r\n\r\nRust IPv4 works! :)";
                let _ = s.write_all(response.as_bytes());
            }
            Err(e) => println!("accept error: {}", e),
        }
    }
}
