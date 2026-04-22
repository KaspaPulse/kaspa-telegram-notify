// [INJECTED BY PHASE 3 SECURITY SCRIPT]
use anyhow::Result;
use tokio::task;

pub async fn run_heavy_task<F, T>(func: F) -> Result<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let res = task::spawn_blocking(func).await?;
    Ok(res)
}

#[macro_export]
macro_rules! safe_unwrap {
    ($val:expr, $msg:expr) => {
        match $val {
            Ok(v) => v,
            Err(e) => return Err(anyhow::anyhow!("{}: {:?}", $msg, e)),
        }
    };
    ($val:expr, $default:expr, $msg:expr) => {
        match $val {
            Ok(v) => v,
            Err(e) => {
                tracing::error!("{} - Error: {:?}", $msg, e);
                $default
            }
        }
    };
}
