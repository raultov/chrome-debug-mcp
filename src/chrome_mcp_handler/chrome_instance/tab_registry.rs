use crate::chrome_mcp_handler::cdp_domains::log::LogState;
use crate::chrome_mcp_handler::cdp_domains::tracing::TracingState;
use crate::chrome_mcp_handler::cdp_domains::webmcp::WebmcpState;
use crate::chrome_mcp_handler::{CustomState, DebuggerState, NetworkState};
use cdp_browser_lite::Tab;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

pub type TabId = String;

pub(crate) struct TabEntry {
    pub(crate) tab: Tab,
    pub(crate) label: Option<String>,
    pub(crate) url: String,
    pub(crate) debugger_state: Arc<Mutex<DebuggerState>>,
    pub(crate) network_state: Arc<Mutex<NetworkState>>,
    pub(crate) log_state: Arc<Mutex<LogState>>,
    pub(crate) tracing_state: Arc<Mutex<TracingState>>,
    pub(crate) custom_state: Arc<Mutex<CustomState>>,
    pub(crate) webmcp_state: Arc<Mutex<WebmcpState>>,
}

pub(crate) struct TabRegistry {
    pub(crate) tabs: HashMap<TabId, TabEntry>,
    pub(crate) active_tab_id: Option<TabId>,
    pub(crate) max_tabs: usize,
    pub(crate) counter: usize,
}

impl TabRegistry {
    pub(crate) fn new(max_tabs: usize) -> Self {
        Self {
            tabs: HashMap::new(),
            active_tab_id: None,
            max_tabs,
            counter: 1,
        }
    }

    pub(crate) fn register_tab(
        &mut self,
        tab: Tab,
        label: Option<String>,
        url: String,
    ) -> Result<TabId, String> {
        if self.tabs.len() >= self.max_tabs {
            return Err(format!("Tab limit reached ({})", self.max_tabs));
        }

        if let Some(ref lbl) = label {
            for entry in self.tabs.values() {
                if entry.label.as_ref() == Some(lbl) {
                    return Err(format!("Label '{}' is already in use by another tab", lbl));
                }
            }
        }

        let tab_id = format!("tab-{}", self.counter);
        self.counter += 1;

        let entry = TabEntry {
            tab,
            label,
            url,
            debugger_state: Arc::new(Mutex::new(DebuggerState::default())),
            network_state: Arc::new(Mutex::new(NetworkState::default())),
            log_state: Arc::new(Mutex::new(LogState::default())),
            tracing_state: Arc::new(Mutex::new(TracingState::default())),
            custom_state: Arc::new(Mutex::new(CustomState::default())),
            webmcp_state: Arc::new(Mutex::new(WebmcpState::default())),
        };

        self.tabs.insert(tab_id.clone(), entry);
        if self.active_tab_id.is_none() {
            self.active_tab_id = Some(tab_id.clone());
        }

        Ok(tab_id)
    }

    pub(crate) fn remove_tab(&mut self, tab_id: &str) -> Option<TabEntry> {
        let removed = self.tabs.remove(tab_id);
        if removed.is_some() && self.active_tab_id.as_deref() == Some(tab_id) {
            // Find first remaining tab key in alphabetical/insertion order or just any key
            self.active_tab_id = self.tabs.keys().next().cloned();
        }
        removed
    }

    pub(crate) fn switch_tab(&mut self, tab_id: &str) -> Result<(), String> {
        if !self.tabs.contains_key(tab_id) {
            return Err(format!("Tab with ID '{}' not found", tab_id));
        }
        self.active_tab_id = Some(tab_id.to_string());
        Ok(())
    }
}
