use pin_project::pin_project;
use std::future::Future;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

#[pin_project]
pub struct On<F>
where
    F: Future<Output = ()> + 'static + Send + Sync,
{
    #[pin]
    coroutine: F,
}

impl<F> On<F>
where
    F: Future<Output = ()> + 'static + Send + Sync,
{
    pub(crate) fn new(coroutine: F) -> Self {
        On { coroutine }
    }
}

impl<F> Future for On<F>
where
    F: Future<Output = ()> + 'static + Send + Sync,
{
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let this = self.project();

        this.coroutine.poll(cx)
    }
}
