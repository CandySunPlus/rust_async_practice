use std::{
    collections::BTreeMap,
    future::Future,
    pin::Pin,
    sync::Mutex,
    task::{Context, Poll, Waker},
    thread::{self, sleep},
    time::{Duration, Instant},
};

fn foo(n: u64) -> Foo {
    let started = false;
    let duration = Duration::from_secs(1);
    let sleep = Box::pin(sleep(duration));
    Foo { n, started, sleep }
}

struct Foo {
    n: u64,
    started: bool,
    sleep: Pin<Box<Sleep>>,
}

impl Future for Foo {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if !self.started {
            println!("start {}", self.n);
            self.started = true;
        }

        if self.sleep.as_mut().poll(cx).is_pending() {
            return Poll::Pending;
        }
        println!("end {}", self.n);
        Poll::Ready(())
    }
}

fn join_all<F: Future>(futures: Vec<F>) -> JoinAll<F> {
    JoinAll {
        futures: futures.into_iter().map(Box::pin).collect(),
    }
}

struct JoinAll<F> {
    futures: Vec<Pin<Box<F>>>,
}

impl<F> Future for JoinAll<F>
where
    F: Future,
{
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let is_pendding = |future: &mut Pin<Box<F>>| future.as_mut().poll(cx).is_pending();
        self.futures.retain_mut(is_pendding);
        if self.futures.is_empty() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

fn sleep(duration: Duration) -> Sleep {
    let wake_time = Instant::now() + duration;
    Sleep { wake_time }
}

struct Sleep {
    wake_time: Instant,
}

static WAKE_TIMES: Mutex<BTreeMap<Instant, Vec<Waker>>> = Mutex::new(BTreeMap::new());

impl Future for Sleep {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if Instant::now() >= self.wake_time {
            Poll::Ready(())
        } else {
            let mut wake_times = WAKE_TIMES.lock().unwrap();
            let wakers_vec = wake_times.entry(self.wake_time).or_default();
            wakers_vec.push(cx.waker().clone());
            Poll::Pending
        }
    }
}

struct Timeout<F> {
    sleep: Pin<Box<Sleep>>,
    inner: Pin<Box<F>>,
}

impl<F: Future> Future for Timeout<F> {
    type Output = Option<F::Output>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Poll::Ready(output) = self.inner.as_mut().poll(cx) {
            return Poll::Ready(Some(output));
        }

        if self.sleep.as_mut().poll(cx).is_ready() {
            return Poll::Ready(None);
        }

        Poll::Pending
    }
}

fn timeout<F>(duration: Duration, inner: F) -> Timeout<F>
where
    F: Future,
{
    Timeout {
        sleep: Box::pin(sleep(duration)),
        inner: Box::pin(inner),
    }
}

#[tokio::main]
async fn main() {
    println!("Hello, world!");
    let mut futures = vec![];
    for i in 0..10 {
        futures.push(foo(i));
    }
    let mut joined_future = Box::pin(join_all(futures));
    let waker = futures::task::noop_waker();
    let mut cx = Context::from_waker(&waker);
    while joined_future.as_mut().poll(&mut cx).is_pending() {
        // Busy loop
        let mut wake_times = WAKE_TIMES.lock().unwrap();
        let next_wake = wake_times.keys().next().expect("sleep forever?");
        thread::sleep(next_wake.saturating_duration_since(Instant::now()));
        while let Some(entry) = wake_times.first_entry() {
            if *entry.key() <= Instant::now() {
                entry.remove().into_iter().for_each(Waker::wake);
            } else {
                break;
            }
        }
    }
}
