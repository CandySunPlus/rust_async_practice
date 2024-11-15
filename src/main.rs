use std::{
    collections::BTreeMap,
    future::Future,
    marker::Send,
    mem,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
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

#[allow(dead_code)]
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

#[allow(dead_code)]
fn timeout<F>(duration: Duration, inner: F) -> Timeout<F>
where
    F: Future,
{
    Timeout {
        sleep: Box::pin(sleep(duration)),
        inner: Box::pin(inner),
    }
}

type DynFuture = Pin<Box<dyn Future<Output = ()> + Send>>;

static NEW_TASKS: Mutex<Vec<DynFuture>> = Mutex::new(Vec::new());

/// Spawns a new asynchronous task and returns a `JoinHandle` that can be used to
/// wait for the completion of the task.
///
/// # Parameters
/// - `future`: The asynchronous task to spawn.
///
/// # Returns
/// - `JoinHandle<T>`: A `JoinHandle` that can be used to wait for the completion of the spawned task.
///
/// # Requirements
/// - `F` must implement `Future` with an output type of `T`.
/// - `F` must be `Send` and have a `'static` lifetime.
/// - `T` must be `Send` and have a `'static` lifetime.
fn spawn<F, T>(future: F) -> JoinHandle<T>
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let join_state = Arc::new(Mutex::new(JoinState::Unawaited));
    let join_handle = JoinHandle(join_state.clone());
    let task = Box::pin(wrap_with_join_state(future, join_state));
    NEW_TASKS.lock().unwrap().push(task);
    join_handle
}

async fn async_main() {
    let mut task_handles = Vec::new();
    for n in 1..=10 {
        task_handles.push(spawn(foo(n)));
    }

    for handle in task_handles {
        handle.await;
    }
}

/// Wraps a future with a join state and waits for its completion.
///
/// This function takes a future `future` and an `Arc<Mutex<JoinState<F::Output>>>`
/// representing the join state. It awaits the completion of the future, updates
/// the join state accordingly, and returns the value produced by the future.
///
/// # Parameters
/// - `future`: The future to be wrapped with a join state.
/// - `join_state`: The `Arc<Mutex<JoinState<F::Output>>>` representing the join state.
///
/// # Returns
/// - The value produced by the future after it has been awaited and completed.
async fn wrap_with_join_state<F: Future>(future: F, join_state: Arc<Mutex<JoinState<F::Output>>>) {
    let value = future.await;
    let mut guard = join_state.lock().unwrap();
    if let JoinState::Awaited(waker) = &*guard {
        waker.wake_by_ref();
    }
    *guard = JoinState::Ready(value)
}

enum JoinState<T> {
    Unawaited,
    Awaited(Waker),
    Ready(T),
    Done,
}

struct JoinHandle<T>(Arc<Mutex<JoinState<T>>>);

impl<T> Future for JoinHandle<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut guard = self.0.lock().unwrap();
        match mem::replace(&mut *guard, JoinState::Done) {
            JoinState::Unawaited | JoinState::Awaited(_) => {
                *guard = JoinState::Awaited(cx.waker().clone());
                Poll::Pending
            }
            JoinState::Ready(value) => Poll::Ready(value),
            JoinState::Done => unreachable!("poll called after completion"),
        }
    }
}

fn main() {
    println!("Hello, world!");

    let waker = futures::task::noop_waker();
    let mut cx = Context::from_waker(&waker);
    let mut main_task = Box::pin(async_main());
    let mut other_tasks = Vec::new();
    loop {
        if main_task.as_mut().poll(&mut cx).is_ready() {
            return;
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

        if other_tasks.is_empty() {
            break;
        }
    }
}
