use std::io;

fn main() -> io::Result<()> {
    let mut socket = std::net::TcpStream::connect("127.0.0.1:8080")?;
    io::copy(&mut socket, &mut io::stdout())?;
    Ok(())
}
