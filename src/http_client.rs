use std::sync::Arc;
use std::time::Duration;

use reqwest::Client;

pub static HTTP_CLIENT: once_cell::sync::Lazy<Arc<Client>> = once_cell::sync::Lazy::new(|| {
    Arc::new(
        Client::builder()
            .timeout(Duration::from_secs(120))
            .pool_max_idle_per_host(16)
            .pool_idle_timeout(Duration::from_secs(90))
            .tcp_keepalive(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("HTTP client build failed"),
    )
});
