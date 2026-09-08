use crate::chrome_mcp_handler::BrowserSession;
use crate::chrome_mcp_handler::chrome_instance::cookie_seed::SeedReport;
use std::sync::Arc;

pub(crate) async fn relaunch_seeded(
    session: &Arc<BrowserSession>,
    seed: SeedReport,
) -> Result<(), String> {
    // 1. Clear cached CDP client and active tabs
    session.reset_connection_state().await;

    // 2. Stop current instance, assign seed profile, and launch fresh instance
    let mut manager = session.chrome_manager.lock().await;
    manager
        .stop_instance()
        .await
        .map_err(|e| format!("Failed to stop Chrome instance for cookie seeding: {e}"))?;

    manager.set_seed_profile(Some(seed));

    manager
        .ensure_instance()
        .await
        .map_err(|e| format!("Failed to start Chrome instance with seeded cookies: {e}"))?;

    Ok(())
}
