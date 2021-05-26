pub trait Provider<'a, Module> {
    type Interface;

    fn provide(module: &'a Module) -> Self::Interface;
}

pub trait HasProvider<'a, I> {
    fn provide(&'a self) -> I;
}

pub mod boxed {
    pub type BoxFuture<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;
}

pub use dipoc_macros::*;
