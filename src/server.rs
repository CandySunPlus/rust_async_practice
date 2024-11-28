use std::{
    io::{self, Read as _, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    task::Poll,
    time::Duration,
};

use crate::{sleep, spawn};

async fn one_response(mut socket: TcpStream, n: u64) -> io::Result<()> {
    let start_msg = format!("start {n}\n");
    write_all(start_msg.as_bytes(), &mut socket).await?;
    sleep(Duration::from_secs(1)).await;
    let end_msg = format!("end {n}\n");
    write_all(end_msg.as_bytes(), &mut socket).await?;
    Ok(())
}

async fn server_main(mut listener: TcpListener) -> io::Result<()> {
    let mut n = 1;
    loop {
        let (socket, _) = accept(&mut listener).await?;
        spawn(async move { one_response(socket, n).await.unwrap() });
        n += 1;
    }
}

pub async fn async_main() -> io::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:8080")?;
    listener.set_nonblocking(true)?;
    spawn(async { server_main(listener).await.unwrap() });
    let mut task_handles = Vec::new();

    for _ in 1..=10 {
        task_handles.push(spawn(client_main()));
    }

    for handle in task_handles {
        handle.await?;
    }
    Ok(())
}

async fn client_main() -> io::Result<()> {
    let mut socket = TcpStream::connect("127.0.0.1:8080")?;
    socket.set_nonblocking(true)?;
    print_all(&mut socket).await?;
    Ok(())
}

async fn print_all(stream: &mut TcpStream) -> io::Result<()> {
    std::future::poll_fn(|context| loop {
        let mut buf = [0; 1024];
        match stream.read(&mut buf) {
            Ok(0) => return Poll::Ready(Ok(())),
            Ok(n) => io::stdout().write_all(&buf[..n])?,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                context.waker().wake_by_ref();
                return Poll::Pending;
            }
            Err(e) => return Poll::Ready(Err(e)),
        }
    })
    .await
}

async fn accept(listener: &mut TcpListener) -> io::Result<(TcpStream, SocketAddr)> {
    std::future::poll_fn(|context| match listener.accept() {
        Ok((stream, addr)) => {
            stream.set_nonblocking(true)?;
            Poll::Ready(Ok((stream, addr)))
        }
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
            context.waker().wake_by_ref();
            Poll::Pending
        }
        Err(e) => Poll::Ready(Err(e)),
    })
    .await
}

async fn write_all(mut buf: &[u8], stream: &mut TcpStream) -> io::Result<()> {
    std::future::poll_fn(|context| {
        while !buf.is_empty() {
            match stream.write(buf) {
                Ok(0) => {
                    let e = io::Error::from(io::ErrorKind::WriteZero);
                    return Poll::Ready(Err(e));
                }
                Ok(n) => buf = &buf[n..],
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    context.waker().wake_by_ref();
                    return Poll::Pending;
                }
                Err(e) => return Poll::Ready(Err(e)),
            }
        }
        Poll::Ready(Ok(()))
    })
    .await
}
