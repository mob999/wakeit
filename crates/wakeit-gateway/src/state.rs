use crate::{config::GatewayConfig, transport::authority};
use std::sync::Arc;
#[derive(Clone)]
pub struct App {
    pub pool: sqlx::PgPool,
    pub js: async_nats::jetstream::Context,
    pub client: async_nats::Client,
    pub config: Arc<GatewayConfig>,
    pub authority: authority::Authority,
    pub shutdown: tokio_util::sync::CancellationToken,
}
