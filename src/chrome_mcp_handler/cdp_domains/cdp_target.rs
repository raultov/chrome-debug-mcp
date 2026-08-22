use cdp_browser_lite::{CdpClient, CdpResult, EventFilter, Tab, WsResponse};
use serde::Serialize;

#[derive(Clone)]
pub(crate) enum CdpTarget {
    Client(CdpClient),
    Tab(Tab),
}

impl std::fmt::Debug for CdpTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Client(_) => f.write_str("CdpTarget::Client"),
            Self::Tab(t) => write!(f, "CdpTarget::Tab(target_id: {})", t.target_id()),
        }
    }
}

impl CdpTarget {
    pub(crate) async fn send_raw_command<P: Serialize>(
        &self,
        method: &str,
        params: P,
    ) -> CdpResult<WsResponse> {
        match self {
            Self::Client(c) => c.send_raw_command(method, params).await,
            Self::Tab(t) => t.send_raw_command(method, params).await,
        }
    }

    pub(crate) fn on_domain(&self, domain: &'static str) -> EventFilter {
        match self {
            Self::Client(c) => c.on_domain(domain),
            Self::Tab(t) => t.on_domain(domain),
        }
    }
}
