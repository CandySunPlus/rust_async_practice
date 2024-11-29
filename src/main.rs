use std::{
    future::Future,
    io,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
    time::Instant,
};

use sample::{server, AwaitFlag, DynFuture, NEW_TASKS, POLL_FDS, POLL_WAKERS, WAKE_TIMES};

async fn async_main() -> io::Result<()> {
    server::async_main().await
}

fn main() -> io::Result<()> {
    println!("Hello, world!");

    let awake_flag = Arc::new(AwaitFlag(Mutex::new(false)));
    let waker = Waker::from(awake_flag.clone());
    let mut cx = Context::from_waker(&waker);
    let mut main_task = Box::pin(async_main());
    let mut other_tasks = Vec::new();
    loop {
        if let Poll::Ready(result) = main_task.as_mut().poll(&mut cx) {
            return result;
        }

        let is_pendding = |task: &mut DynFuture| task.as_mut().poll(&mut cx).is_pending();

        other_tasks.retain_mut(is_pendding);

        loop {
            let Some(mut task) = NEW_TASKS.lock().unwrap().pop() else {
                break;
            };

            // Polling this task could spawn more tasks, so it's important that NEW_TASKS is not locked here.
            if task.as_mut().poll(&mut cx).is_pending() {
                other_tasks.push(task);
            }
        }

        if awake_flag.check_and_clear() {
            continue;
        }

        let mut wake_times = WAKE_TIMES.lock().unwrap();
        let timeout_ms = if let Some(time) = wake_times.keys().next() {
            let duration = time.saturating_duration_since(Instant::now());
            duration.as_millis() as libc::c_int
        } else {
            -1
        };
        let mut poll_fds = POLL_FDS.lock().unwrap();
        let mut poll_wakers = POLL_WAKERS.lock().unwrap();
        let poll_error_code =
            unsafe { libc::poll(poll_fds.as_mut_ptr(), poll_fds.len() as u32, timeout_ms) };

        if poll_error_code < 0 {
            return Err(io::Error::last_os_error());
        }

        poll_fds.clear();
        poll_wakers.drain(..).for_each(Waker::wake);

        while let Some(entry) = wake_times.first_entry() {
            if *entry.key() <= Instant::now() {
                entry.remove().into_iter().for_each(Waker::wake);
            } else {
                break;
            }
        }
    }
}
