use std::sync::Arc;

use dipoc::boxed::BoxFuture;
use dipoc::{HasProvider, Provider};

type Error = String;

trait Context: 'static {}

trait Repository {
    type Context: Context;
    fn get(&self, ctx: &mut Self::Context) -> usize;
}

trait Service {
    fn serve(&mut self) -> usize;
}

struct ContextImpl;

impl Context for ContextImpl {}

struct RepositoryImpl;

impl Repository for RepositoryImpl {
    type Context = ContextImpl;

    fn get(&self, _: &mut Self::Context) -> usize {
        42
    }
}

#[derive(Provider)]
#[provide(dyn Service, box, fallible(error = Error), async)]
struct ServiceImpl<Ctx: Context> {
    #[depend(await, try(error = Error))]
    ctx: Ctx,
    repo: Arc<dyn Repository<Context = Ctx>>,
    #[depend(default)]
    some_string: String,
}

impl<Ctx: Context> Service for ServiceImpl<Ctx> {
    fn serve(&mut self) -> usize {
        println!("some_string: {}", self.some_string);
        self.repo.get(&mut self.ctx)
    }
}

struct MyModule {
    repository: Arc<RepositoryImpl>,
}

impl MyModule {
    fn new() -> Self {
        Self {
            repository: Arc::new(RepositoryImpl),
        }
    }

    async fn try_get_context(&self) -> Result<ContextImpl, Error> {
        Ok(ContextImpl)
    }
}

impl<'a> HasProvider<'a, Arc<dyn Repository<Context = ContextImpl>>> for MyModule {
    fn provide(&'a self) -> Arc<dyn Repository<Context = ContextImpl>> {
        self.repository.clone()
    }
}

impl<'a> HasProvider<'a, BoxFuture<'a, Result<ContextImpl, Error>>> for MyModule {
    fn provide(&'a self) -> BoxFuture<Result<ContextImpl, Error>> {
        Box::pin(self.try_get_context())
    }
}

impl<'a> HasProvider<'a, BoxFuture<'a, Result<Box<dyn Service>, Error>>> for MyModule {
    fn provide(&'a self) -> BoxFuture<Result<Box<dyn Service>, Error>> {
        ServiceImpl::provide(self)
    }
}

#[async_std::main]
async fn main() {
    let module = MyModule::new();
    let service: BoxFuture<'_, Result<Box<dyn Service>, Error>> = module.provide();
    let mut service = service.await.unwrap();
    let served = service.serve();
    println!("served: {}", served);
}
