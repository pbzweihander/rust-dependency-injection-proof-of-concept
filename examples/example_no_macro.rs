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

struct ServiceImpl<Ctx: Context> {
    ctx: Ctx,
    repo: Arc<dyn Repository<Context = Ctx>>,
    some_string: String,
}

impl<Ctx: Context> Service for ServiceImpl<Ctx> {
    fn serve(&mut self) -> usize {
        println!("some_string: {}", self.some_string);
        self.repo.get(&mut self.ctx)
    }
}

impl<'a, M, Ctx> Provider<'a, M> for ServiceImpl<Ctx>
where
    M: Sync + 'a,
    M: HasProvider<'a, BoxFuture<'a, Result<Ctx, Error>>>,
    M: HasProvider<'a, Arc<dyn Repository<Context = Ctx>>>,
    Ctx: Context,
{
    type Interface = BoxFuture<'a, Result<Box<dyn Service>, Error>>;

    fn provide(module: &'a M) -> Self::Interface {
        Box::pin(async move {
            let ctx: BoxFuture<'_, Result<Ctx, Error>> = module.provide();
            let ctx = ctx.await?;
            let repo = module.provide();
            let some_string = Default::default();
            Ok(Box::new(Self {
                ctx,
                repo,
                some_string,
            }) as Box<dyn Service>)
        })
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
    fn provide(&'a self) -> BoxFuture<'a, Result<ContextImpl, Error>> {
        Box::pin(self.try_get_context())
    }
}

impl<'a> HasProvider<'a, BoxFuture<'a, Result<Box<dyn Service>, Error>>> for MyModule {
    fn provide(&'a self) -> BoxFuture<'a, Result<Box<dyn Service>, Error>> {
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
