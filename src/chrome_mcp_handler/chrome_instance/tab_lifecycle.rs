use crate::chrome_mcp_handler::chrome_instance::tab_registry::TabRegistry;
use cdp_browser_lite::{BrowserClient, CdpResult, WsResponse};
use std::sync::{Arc, RwLock};
use tokio_stream::StreamExt;

pub(crate) fn start_tab_lifecycle_listener(
    browser_client: BrowserClient,
    tabs: Arc<RwLock<TabRegistry>>,
) {
    let mut target_events = browser_client.client().on_domain("Target");
    let bc = browser_client.clone();
    let tabs_clone = tabs.clone();

    tokio::spawn(async move {
        // Habilitamos el descubrimiento de targets para recibir targetCreated/targetDestroyed
        let _ = bc.set_discover_targets(true).await;

        while let Some(item) = target_events.next().await {
            match item {
                Ok(event) => {
                    let _ = process_target_event(&event, &bc, &tabs_clone).await;
                }
                Err(e) => {
                    eprintln!("[chrome-debug-mcp] Target lifecycle event stream error: {e}");
                }
            }
        }
    });
}

async fn process_target_event(
    event: &WsResponse,
    browser_client: &BrowserClient,
    tabs: &Arc<RwLock<TabRegistry>>,
) -> CdpResult<()> {
    let method = match event.method.as_deref() {
        Some(m) => m,
        None => return Ok(()),
    };

    let params = match &event.params {
        Some(p) => p,
        None => return Ok(()),
    };

    match method {
        "Target.targetCreated" => {
            if let Some(info) = params.get("targetInfo")
                && let Some(target_id) = info.get("targetId").and_then(|v| v.as_str())
                && let Some(ty) = info.get("type").and_then(|v| v.as_str())
                && let Some(url) = info.get("url").and_then(|v| v.as_str())
                && ty == "page"
                && !url.starts_with("chrome-extension://")
            {
                // Verificamos si ya está en nuestro registro
                let already_exists = {
                    let registry = tabs.read().unwrap();
                    registry
                        .tabs
                        .values()
                        .any(|entry| entry.tab.target_id() == target_id)
                };

                if !already_exists {
                    // Nos adjuntamos a la pestaña
                    if let Ok(tab) = browser_client.attach(target_id).await {
                        // Habilitamos los dominios CDP necesarios para esta pestaña
                        let _ = tab
                            .send_raw_command("Runtime.enable", cdp_browser_lite::NoParams)
                            .await;
                        let _ = tab
                            .send_raw_command("Page.enable", cdp_browser_lite::NoParams)
                            .await;
                        let _ = tab
                            .send_raw_command("Network.enable", cdp_browser_lite::NoParams)
                            .await;
                        let _ = tab
                            .send_raw_command("Log.enable", cdp_browser_lite::NoParams)
                            .await;
                        let _ = tab
                            .send_raw_command("Debugger.enable", cdp_browser_lite::NoParams)
                            .await;
                        let _ = tab
                            .send_raw_command("WebMCP.enable", cdp_browser_lite::NoParams)
                            .await;

                        let states = {
                            let mut registry = tabs.write().unwrap();
                            if let Ok(tab_id) =
                                registry.register_tab(tab.clone(), None, url.to_string())
                            {
                                registry.tabs.get(&tab_id).map(|entry| {
                                    (
                                        entry.debugger_state.clone(),
                                        entry.network_state.clone(),
                                        entry.log_state.clone(),
                                        entry.tracing_state.clone(),
                                        entry.webmcp_state.clone(),
                                    )
                                })
                            } else {
                                None
                            }
                        };

                        if let Some((dbg, net, log, trace, webmcp)) = states {
                            let target =
                                crate::chrome_mcp_handler::cdp_domains::cdp_target::CdpTarget::Tab(
                                    tab.clone(),
                                );
                            crate::chrome_mcp_handler::cdp_domains::debugger::start_debugger_listener(&target, dbg);
                            crate::chrome_mcp_handler::cdp_domains::network::start_network_listener(
                                &target, net,
                            );
                            crate::chrome_mcp_handler::cdp_domains::log::start_log_listener(
                                &target, log,
                            );
                            crate::chrome_mcp_handler::cdp_domains::tracing::start_tracing_listener(
                                &target, trace,
                            );
                            crate::chrome_mcp_handler::cdp_domains::webmcp::start_webmcp_listener(
                                &target, webmcp,
                            );
                        }
                    }
                }
            }
        }
        "Target.targetDestroyed" => {
            if let Some(target_id) = params.get("targetId").and_then(|v| v.as_str()) {
                let tab_id_to_remove = {
                    let registry = tabs.read().unwrap();
                    registry
                        .tabs
                        .iter()
                        .find(|(_, entry)| entry.tab.target_id() == target_id)
                        .map(|(id, _)| id.clone())
                };

                if let Some(tab_id) = tab_id_to_remove {
                    let mut registry = tabs.write().unwrap();
                    registry.remove_tab(&tab_id);
                }
            }
        }
        _ => {}
    }

    Ok(())
}
