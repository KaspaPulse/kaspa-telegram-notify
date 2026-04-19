// [INJECTED BY PHASE 3 SECURITY SCRIPT]
use tokio::task;
use anyhow::Result;

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
    ($val:expr, $default:expr, $msg:expr) => {
        match $val {
            Ok(v) => v,
            Err(e) => {
                tracing::error!("{} - Error: {:?}", $msg, e);
                $default
            }
        }
    };
    ($val:expr, $default:expr) => {
        match $val {
            Some(v) => v,
            None => {
                tracing::error!("Value was None, safely fell back to default.");
                $default
            }
        }
    };
}
