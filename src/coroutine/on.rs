//use pin_project::pin_project;
//use std::future::Future;
//use std::pin::Pin;
//use std::task::Context;
//use std::task::Poll;
//
//use crate::prelude::Fib;
//
//use super::Coroutine;
//
//#[pin_project]
//pub struct On<'a, F>
//where
//    F: Future<Output = ()> + 'static + Send + Sync,
//{
//    fib: &'a Fib,
//    #[pin]
//    coroutine: F,
//}
//
//impl<'a, F> On<'a, F>
//where
//    F: Future<Output = ()> + 'static + Send + Sync,
//{
//    pub(crate) fn new(fib: &'a Fib, coroutine: F) -> Self {
//        On { fib, coroutine }
//    }
//}
//
//impl<'a, F> Future for On<'a, F>
//where
//    F: Coroutine<'static>,
//{
//    type Output = ();
//
//    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
//        let this = self.project();
//
//        this.coroutine.poll(cx)
//    }
//}
//
//impl<'cx, F: Coroutine<'static>> PrimitiveVoid<'cx> for On<'cx, F> {
//    fn get_context(&self) -> &super::Fib {
//        self.fib
//    }
//}
