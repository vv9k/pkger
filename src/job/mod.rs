mod build;
mod oneshot;

pub use build::BuildCtx;
pub use oneshot::OneShotCtx;

pub trait Ctx {
    fn id(&self) -> &str;
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

pub enum JobCtx<'j> {
    Build(BuildCtx),
    OneShot(OneShotCtx<'j>),
}

pub struct JobRunner<'j> {
    pub ctx: JobCtx<'j>,
}

impl<'j> JobRunner<'j> {
    pub fn new<J: Into<JobCtx<'j>>>(ctx: J) -> JobRunner<'j> {
        JobRunner { ctx: ctx.into() }
    }

    pub async fn run(mut self) -> JobResult {
        match &mut self.ctx {
            JobCtx::Build(ctx) => {
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
            JobCtx::OneShot(ctx) => {
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
