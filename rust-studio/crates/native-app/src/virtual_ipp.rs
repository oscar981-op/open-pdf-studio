//! One-shot, loopback-only IPP printer for exercising the native print path.
//! It never discovers or connects to an installed printer.
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::time::{Duration, Instant};

const MAX_TEST_PDF: usize = 64 * 1024 * 1024;
const PATH: &str = "/ipp/opds-test";

fn attribute(output: &mut Vec<u8>, tag: u8, name: &str, value: &str) {
    output.push(tag);
    output.extend_from_slice(&(name.len() as u16).to_be_bytes());
    output.extend_from_slice(name.as_bytes());
    output.extend_from_slice(&(value.len() as u16).to_be_bytes());
    output.extend_from_slice(value.as_bytes());
}

fn ipp_request(port: u16) -> Vec<u8> {
    let mut output = vec![1, 1, 0, 2, 0, 0, 0, 1, 1];
    attribute(&mut output, 0x47, "attributes-charset", "utf-8");
    attribute(&mut output, 0x48, "attributes-natural-language", "en-us");
    attribute(
        &mut output,
        0x45,
        "printer-uri",
        &format!("ipp://127.0.0.1:{port}{PATH}"),
    );
    attribute(&mut output, 0x42, "requesting-user-name", "Open PDF Studio");
    attribute(&mut output, 0x49, "document-format", "application/pdf");
    output.push(3);
    output
}

fn ipp_response() -> Vec<u8> {
    let mut output = vec![1, 1, 0, 0, 0, 0, 0, 1, 1];
    attribute(&mut output, 0x47, "attributes-charset", "utf-8");
    attribute(&mut output, 0x48, "attributes-natural-language", "en-us");
    output.push(3);
    output
}

fn read_http(stream: &mut TcpStream, max_body: usize) -> Result<(String, Vec<u8>), String> {
    let mut headers = Vec::new();
    let mut byte = [0];
    while !headers.ends_with(b"\r\n\r\n") {
        if headers.len() >= 8192 {
            return Err("Virtual IPP HTTP header is too large".into());
        }
        stream.read_exact(&mut byte).map_err(|e| e.to_string())?;
        headers.push(byte[0]);
    }
    let headers = String::from_utf8(headers).map_err(|e| e.to_string())?;
    let mut content_length = None;
    let mut content_type = false;
    for line in headers.lines().skip(1) {
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("Content-Length") {
                content_length = Some(value.trim().parse::<usize>().map_err(|e| e.to_string())?);
            }
            if name.eq_ignore_ascii_case("Content-Type") {
                content_type = value.trim().eq_ignore_ascii_case("application/ipp");
            }
        }
    }
    if !content_type {
        return Err("Virtual IPP requires application/ipp".into());
    }
    let length = content_length.ok_or("Virtual IPP requires Content-Length")?;
    if length > max_body {
        return Err("Virtual IPP message exceeds test limit".into());
    }
    let mut body = vec![0; length];
    stream.read_exact(&mut body).map_err(|e| e.to_string())?;
    Ok((headers, body))
}

fn receive_one(listener: TcpListener, prefix: Vec<u8>) -> Result<Vec<u8>, String> {
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let (mut stream, peer) = loop {
        match listener.accept() {
            Ok(connection) => break connection,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(format!("Virtual IPP accept failed: {e}")),
        }
    };
    if !peer.ip().is_loopback() {
        return Err("Virtual IPP accepts loopback only".into());
    }
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    let (headers, body) = read_http(&mut stream, MAX_TEST_PDF + prefix.len())?;
    if !headers.starts_with(&format!("POST {PATH} HTTP/1.1\r\n"))
        || !body.starts_with(&prefix)
        || !body[prefix.len()..].starts_with(b"%PDF-")
    {
        return Err("Virtual IPP rejected Print-Job request".into());
    }
    let pdf = body[prefix.len()..].to_vec();
    let response = ipp_response();
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/ipp\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.len()
    )
    .map_err(|e| e.to_string())?;
    stream.write_all(&response).map_err(|e| e.to_string())?;
    Ok(pdf)
}

/// Send exactly one PDF through a temporary local IPP server and return what
/// the server received. No caller-provided address is accepted.
pub fn print_roundtrip(pdf: &[u8]) -> Result<Vec<u8>, String> {
    if !pdf.starts_with(b"%PDF-") || pdf.len() > MAX_TEST_PDF {
        return Err("Virtual IPP requires a PDF of at most 64 MiB".into());
    }
    let listener =
        TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let prefix = ipp_request(port);
    let server = std::thread::spawn(move || receive_one(listener, prefix));
    let client_result = (|| -> Result<(), String> {
        let mut stream = TcpStream::connect(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
            .map_err(|e| e.to_string())?;
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| e.to_string())?;
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| e.to_string())?;
        let prefix = ipp_request(port);
        write!(
            stream,
            "POST {PATH} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/ipp\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            prefix.len() + pdf.len()
        )
        .map_err(|e| e.to_string())?;
        stream.write_all(&prefix).map_err(|e| e.to_string())?;
        stream.write_all(pdf).map_err(|e| e.to_string())?;
        let (headers, body) = read_http(&mut stream, 8192)?;
        if !headers.starts_with("HTTP/1.1 200 OK\r\n")
            || body.len() < 9
            || body[..8] != [1, 1, 0, 0, 0, 0, 0, 1]
            || body[8] != 1
        {
            return Err("Virtual IPP did not accept Print-Job".into());
        }
        Ok(())
    })();
    let received = server
        .join()
        .map_err(|_| "Virtual IPP server thread stopped".to_owned())??;
    client_result?;
    if received != pdf {
        return Err("Virtual IPP PDF differs from submitted data".into());
    }
    Ok(received)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_print_job_preserves_pdf_bytes() {
        let pdf = b"%PDF-1.4\n1 0 obj<<>>endobj\n%%EOF\n";
        assert_eq!(print_roundtrip(pdf).unwrap(), pdf);
        assert!(print_roundtrip(b"not a PDF").is_err());
    }
}
