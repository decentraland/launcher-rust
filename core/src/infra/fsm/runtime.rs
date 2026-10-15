use log::{error, info, warn};
use std::fmt::Debug;
use std::future::Future;
use tokio::runtime::Handle;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

pub trait CmdExecutor<Cmd, Msg> {
    fn execute(
        &mut self,
        cmd: Cmd,
        inbox: &UnboundedSender<Msg>,
    ) -> impl Future<Output = anyhow::Result<()>> + Send;
}

/// Pure
pub type Update<Model, Msg, Cmd> = fn(Model, Msg) -> (Model, Cmd);

pub struct FsmRuntime<Model, Msg, Cmd, E> {
    tag: &'static str,
    inbox: UnboundedSender<Msg>,
    queue: Option<UnboundedReceiver<Msg>>,
    model: watch::Sender<Model>,
    update: Update<Model, Msg, Cmd>,
    executor: Option<E>,
    tokio: Handle,
    cancellation: CancellationToken,
}

impl<Model, Msg, Cmd, E> FsmRuntime<Model, Msg, Cmd, E>
where
    Model: Clone + Debug + Send + Sync + 'static,
    Msg: Debug + Send + 'static,
    Cmd: Debug + Send + 'static,
    E: CmdExecutor<Cmd, Msg> + Send + 'static,
{
    pub fn new(
        tag: &'static str,
        init_model: Model,
        update: Update<Model, Msg, Cmd>,
        executor: E,
        tokio: Handle,
    ) -> Self {
        let (inbox, queue) = unbounded_channel();

        Self {
            tag,
            inbox,
            queue: Some(queue),
            model: watch::Sender::new(init_model),
            update,
            executor: Some(executor),
            tokio,
            cancellation: CancellationToken::new(),
        }
    }

    pub const fn tag(&self) -> &'static str {
        self.tag
    }

    pub fn inbox(&self) -> UnboundedSender<Msg> {
        self.inbox.clone()
    }

    pub fn snapshot(&self) -> Model {
        self.model.borrow().clone()
    }

    pub fn watch(&self) -> watch::Receiver<Model> {
        self.model.subscribe()
    }

    pub fn start(&mut self) {
        let (Some(mut queue), Some(mut executor)) = (self.queue.take(), self.executor.take())
        else {
            warn!("[{}] already started", self.tag);
            return;
        };

        let tag = self.tag;
        let inbox = self.inbox.clone();
        let model = self.model.clone();
        let update = self.update;
        let cancellation = self.cancellation.clone();

        self.tokio.spawn(async move {
            loop {
                let msg = tokio::select! {
                    () = cancellation.cancelled() => return,
                    msg = queue.recv() => match msg {
                        Some(msg) => msg,
                        None => return,
                    },
                };

                info!("[{}] msg: {:?}", tag, msg);

                let (next, cmd) = update(model.borrow().clone(), msg);
                info!("[{}] model: {:?} cmd: {:?}", tag, next, cmd);
                model.send_replace(next);

                if let Err(e) = executor.execute(cmd, &inbox).await {
                    error!("[{}] cmd failed: {:#}", tag, e);
                }
            }
        });
    }

    pub fn stop(&self) {
        self.cancellation.cancel();
    }
}

impl<Model, Msg, Cmd, E> Drop for FsmRuntime<Model, Msg, Cmd, E> {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::bail;
    use std::thread;
    use std::time::Duration;
    use tokio::time::timeout;

    const TAG: &str = "CounterFsm";
    const THREADS: usize = 8;
    const SENDS_PER_THREAD: usize = 1000;
    const TIMEOUT: Duration = Duration::from_secs(5);

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum CounterMsg {
        Add(i32),
        AddThenEcho(i32),
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum CounterCmd {
        Observe(i32),
        Echo(i32),
    }

    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    struct Counter {
        total: i32,
    }

    fn update(model: Counter, msg: CounterMsg) -> (Counter, CounterCmd) {
        match msg {
            CounterMsg::Add(amount) => {
                let next = Counter {
                    total: model.total.saturating_add(amount),
                };
                (next, CounterCmd::Observe(next.total))
            }
            CounterMsg::AddThenEcho(amount) => {
                let next = Counter {
                    total: model.total.saturating_add(amount),
                };
                (next, CounterCmd::Echo(amount))
            }
        }
    }

    struct RecordingExecutor {
        executed: UnboundedSender<CounterCmd>,
        fail_on_observe: bool,
    }

    impl CmdExecutor<CounterCmd, CounterMsg> for RecordingExecutor {
        async fn execute(
            &mut self,
            cmd: CounterCmd,
            inbox: &UnboundedSender<CounterMsg>,
        ) -> anyhow::Result<()> {
            self.executed.send(cmd)?;
            match cmd {
                CounterCmd::Echo(amount) => {
                    tokio::task::yield_now().await;
                    inbox.send(CounterMsg::Add(amount))?;
                    Ok(())
                }
                CounterCmd::Observe(_) => {
                    if self.fail_on_observe {
                        bail!("Executor failure");
                    }
                    Ok(())
                }
            }
        }
    }

    type CounterRuntime = FsmRuntime<Counter, CounterMsg, CounterCmd, RecordingExecutor>;

    fn started(fail_on_observe: bool) -> (CounterRuntime, UnboundedReceiver<CounterCmd>) {
        let (executed, executed_rx) = unbounded_channel();
        let executor = RecordingExecutor {
            executed,
            fail_on_observe,
        };
        let mut runtime =
            FsmRuntime::new(TAG, Counter::default(), update, executor, Handle::current());
        runtime.start();
        (runtime, executed_rx)
    }

    async fn executed(rx: &mut UnboundedReceiver<CounterCmd>, count: usize) -> Vec<CounterCmd> {
        let mut cmds = Vec::with_capacity(count);
        for _ in 0..count {
            match timeout(TIMEOUT, rx.recv()).await {
                Ok(Some(cmd)) => cmds.push(cmd),
                _ => break,
            }
        }
        cmds
    }

    #[tokio::test]
    async fn applies_messages_in_arrival_order() {
        let (runtime, mut executed_rx) = started(false);
        let inbox = runtime.inbox();

        for amount in [1, 2, 3] {
            let _ = inbox.send(CounterMsg::Add(amount));
        }

        assert_eq!(
            executed(&mut executed_rx, 3).await,
            vec![
                CounterCmd::Observe(1),
                CounterCmd::Observe(3),
                CounterCmd::Observe(6)
            ]
        );
        assert_eq!(runtime.snapshot().total, 6);
    }

    #[tokio::test]
    async fn applies_messages_sent_by_executor_after_the_queued_ones() {
        let (runtime, mut executed_rx) = started(false);
        let inbox = runtime.inbox();

        let _ = inbox.send(CounterMsg::AddThenEcho(5));
        let _ = inbox.send(CounterMsg::Add(1));

        assert_eq!(
            executed(&mut executed_rx, 3).await,
            vec![
                CounterCmd::Echo(5),
                CounterCmd::Observe(6),
                CounterCmd::Observe(11)
            ]
        );
        assert_eq!(runtime.snapshot().total, 11);
    }

    #[tokio::test]
    async fn accepts_sends_from_multiple_threads() {
        let (runtime, mut executed_rx) = started(false);

        let threads: Vec<_> = (0..THREADS)
            .map(|_| {
                let inbox = runtime.inbox();
                thread::spawn(move || {
                    for _ in 0..SENDS_PER_THREAD {
                        let _ = inbox.send(CounterMsg::Add(1));
                    }
                })
            })
            .collect();
        for handle in threads {
            assert!(handle.join().is_ok());
        }

        let total = THREADS.saturating_mul(SENDS_PER_THREAD);
        assert_eq!(executed(&mut executed_rx, total).await.len(), total);
        assert_eq!(Ok(runtime.snapshot().total), i32::try_from(total));
    }

    #[tokio::test]
    async fn keeps_going_when_executor_fails() {
        let (runtime, mut executed_rx) = started(true);
        let inbox = runtime.inbox();

        let _ = inbox.send(CounterMsg::Add(1));
        let _ = inbox.send(CounterMsg::Add(2));

        assert_eq!(
            executed(&mut executed_rx, 2).await,
            vec![CounterCmd::Observe(1), CounterCmd::Observe(3)]
        );
        assert_eq!(runtime.snapshot().total, 3);
    }

    #[tokio::test]
    async fn stop_ends_the_task() {
        let (runtime, mut executed_rx) = started(false);

        runtime.stop();

        assert_eq!(timeout(TIMEOUT, executed_rx.recv()).await, Ok(None));
    }

    #[tokio::test]
    async fn drop_ends_the_task() {
        let (runtime, mut executed_rx) = started(false);

        drop(runtime);

        assert_eq!(timeout(TIMEOUT, executed_rx.recv()).await, Ok(None));
    }

    #[tokio::test]
    async fn second_start_is_ignored() {
        let (mut runtime, mut executed_rx) = started(false);
        let inbox = runtime.inbox();

        runtime.start();
        let _ = inbox.send(CounterMsg::Add(4));

        assert_eq!(
            executed(&mut executed_rx, 1).await,
            vec![CounterCmd::Observe(4)]
        );
    }
}
