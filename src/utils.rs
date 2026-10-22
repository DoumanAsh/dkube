//!Utilities module for various functions within crate

use core::future::{self, Future};

pub use tokio::time;

///Wait method for [Retry]
pub trait WaitMethod {
    ///Returns future that awaits required delay time
    ///
    ///`count` is attempt number
    fn create_future(&self, count: u16) -> impl Future<Output = ()> + 'static;
}

impl WaitMethod for () {
    #[inline(always)]
    fn create_future(&self, _: u16) -> impl Future<Output = ()> + 'static {
        future::ready(())
    }
}

impl WaitMethod for time::Duration {
    #[inline(always)]
    fn create_future(&self, _: u16) -> impl Future<Output = ()> + 'static {
        time::sleep(*self)
    }
}

///Generic retry mechanism
pub struct Retry<T> {
    max_retries: u16,
    wait_method: T,
}

impl Retry<()> {
    ///Creates new [Retry] with specified number of `max_retries` and no waiting between
    pub const fn new(max_retries: u16) -> Self {
        Self {
            max_retries,
            wait_method: (),
        }
    }

    ///Configures [Retry] to specified T as [WaitMethod]
    pub const fn with_wait<T: WaitMethod>(self, wait_method: T) -> Retry<T> {
        Retry {
            max_retries: self.max_retries,
            wait_method
        }
    }
}

impl<W: WaitMethod> Retry<W> {
    #[inline(always)]
    ///Returns reference to [WaitMethod]
    pub const fn wait_method(&self) -> &W {
        &self.wait_method
    }

    ///Runs future created by specified callback `cb` using `classifier` to determine if error can be retried or not
    pub async fn run<T, E, Task: Future<Output = Result<T, E>>, Classifier: Fn(&E) -> bool, CB: Fn() -> Task>(&self, cb: CB, classifier: Classifier) -> Result<T, E> {
        let mut count = 0u16;

        loop {
            match (cb)().await {
                Ok(result) => break Ok(result),
                Err(error) if count < self.max_retries && (classifier)(&error) => {
                    count = count.saturating_add(1);
                    self.wait_method.create_future(count).await;
                },
                Err(error) => break Err(error)
            }
        }
    }
}
