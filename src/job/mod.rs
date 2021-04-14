mod build;
mod container;
mod oneshot;

pub use build::BuildCtx;
pub use oneshot::OneShotCtx;

use async_trait::async_trait;

#[macro_export]
macro_rules! cleanup {
    ($ctx:ident, $span: ident) => {
        if $ctx.check_is_running().instrument($span.clone()).await? {
            return Err(anyhow!("job interrupted by ctrl-c signal"));
        }
    };
}

#[async_trait]
pub trait Ctx {
    type JobResult;

    fn id(&self) -> &str;
    async fn run(&mut self) -> Self::JobResult;
}

pub enum JobResult {
    Success { id: String },
    Failure { id: String, reason: String },
}

impl JobResult {
    pub fn success<I>(id: I) -> Self
    where
        I: Into<String>,
    {
        Self::Success { id: id.into() }
    }

    pub fn failure<I, E>(id: I, err: E) -> Self
    where
        I: Into<String>,
        E: Into<String>,
    {
        Self::Failure {
            id: id.into(),
            reason: err.into(),
        }
    }
}

pub enum JobCtx<'job> {
    Build(BuildCtx),
    OneShot(OneShotCtx<'job>),
}

impl<'job> JobCtx<'job> {
    pub async fn run(self) -> JobResult {
        match self {
            JobCtx::Build(mut ctx) => {
                if let Err(e) = ctx.run().await {
                    let reason = match e.downcast::<moby::Error>() {
                        Ok(err) => match err {
                            moby::Error::Fault { code: _, message } => message,
                            e => e.to_string(),
                        },
                        Err(e) => e.to_string(),
                    };
                    JobResult::failure(ctx.id(), reason)
                } else {
                    JobResult::success(ctx.id())
                }
            }
            JobCtx::OneShot(mut ctx) => {
                if let Err(e) = ctx.run().await {
                    let reason = match e.downcast::<moby::Error>() {
                        Ok(err) => match err {
                            moby::Error::Fault { code: _, message } => message,
                            e => e.to_string(),
                        },
                        Err(e) => e.to_string(),
                    };
                    JobResult::failure(ctx.id(), reason)
                } else {
                    JobResult::success(ctx.id())
                }
            }
        }
    }
}
