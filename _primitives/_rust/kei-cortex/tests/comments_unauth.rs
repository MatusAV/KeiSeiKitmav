//! Auth boundary check for `/api/v1/cortex/comments/*`.
//!
//! Lives in its own binary so the comments-primitive process-wide
//! `OnceLock` (`global_store`) used by `comments_smoke.rs` cannot leak
//! between tests. The unauth path never touches the store, but binary
//! isolation keeps the OnceLock contract simple.

mod common;

use common::{async_client, spawn};

#[tokio::test]
async fn comments_unauthenticated_returns_401() {
    let srv = spawn().await;
    let resp = async_client()
        .get(format!(
            "{}/api/v1/cortex/comments/by-page/some-page",
            srv.base_url
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}
