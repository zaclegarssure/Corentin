//use std::future::Future;
//
//use crate::rework::executor::msg::SignalId;
//
//use super::scope::Scope;
//
//#[must_use = "futures do nothing unless you `.await` or poll them"]
//pub struct AwaitSignal<'a> {
//    scope: &'a mut Scope,
//    id: SignalId,
//}
//
//impl<'a> AwaitSignal<'a> {
//    pub fn new(scope: &'a mut Scope, id: SignalId) -> Self {
//        AwaitSignal { scope, id }
//    }
//}
//
//impl Future for AwaitSignal<'_> {
//    type Output = T;
//
//    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
//        todo!()
//    }
//}
