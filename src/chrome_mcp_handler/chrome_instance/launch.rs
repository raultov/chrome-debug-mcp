use cdp_browser_lite::{BrowserConfig, LaunchMode, ProfileMode};
use rust_mcp_sdk::macros;

/// Curated Chrome capability presets an MCP client may request when restarting
/// the browser.
///
/// This is deliberately a closed enum rather than free-form command line
/// arguments: it keeps `restart_chrome` from becoming an arbitrary Chrome-flag
/// injection point (`--disable-web-security`, `--load-extension`, ...).
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    ::serde::Serialize,
    ::serde::Deserialize,
    macros::JsonSchema,
)]
pub enum ChromeFeature {
    /// Experimental WebMCP surface, used by sites that expose tools to the browser.
    #[serde(rename = "WEB_MCP")]
    WebMcp,
    /// Software (SwiftShader) rasterization for WebGL, for GPU-less environments.
    #[serde(rename = "WEBGL_SOFTWARE")]
    WebglSoftware,
}

impl ChromeFeature {
    /// Stable client-facing name of this preset, matching the published schema.
    pub(crate) fn as_name(self) -> &'static str {
        match self {
            Self::WebMcp => "WEB_MCP",
            Self::WebglSoftware => "WEBGL_SOFTWARE",
        }
    }

    /// Chrome command line switches this preset expands to.
    pub(crate) fn switches(self) -> &'static [&'static str] {
        match self {
            Self::WebMcp => &[
                "--enable-features=WebMCPTesting",
                "--categoryExperimentalWebmcp=true",
            ],
            Self::WebglSoftware => &[
                "--use-gl=angle",
                "--use-angle=swiftshader",
                "--enable-unsafe-swiftshader",
            ],
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct LaunchParams {
    host: String,
    port: u16,
    headless: bool,
    enable_automation: bool,
    pub(crate) user_profile: bool,
    pub(crate) secondary: bool,
    proxy: Option<String>,
    features: Vec<ChromeFeature>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LaunchPlan {
    pub(crate) mode: LaunchMode,
    pub(crate) profile: ProfileMode,
    pub(crate) extra_args: Vec<String>,
}

impl LaunchParams {
    pub(crate) fn new(
        host: String,
        port: u16,
        enable_automation: bool,
        headless: bool,
        user_profile: bool,
    ) -> Self {
        Self {
            host,
            port,
            enable_automation,
            headless,
            user_profile,
            secondary: false,
            proxy: None,
            features: Vec::new(),
        }
    }

    pub(crate) fn set_features(&mut self, features: Vec<ChromeFeature>) {
        self.features = features;
    }

    pub(crate) fn features(&self) -> &[ChromeFeature] {
        &self.features
    }

    pub(crate) fn set_proxy(&mut self, proxy: Option<String>) {
        self.proxy = proxy;
    }

    pub(crate) fn configured_port(&self) -> u16 {
        self.port
    }

    pub(crate) fn plan(&self) -> LaunchPlan {
        let mode = if self.secondary {
            LaunchMode::LaunchNew
        } else if is_local_host(&self.host) {
            LaunchMode::Auto
        } else {
            LaunchMode::AttachOnly
        };
        let profile = if self.user_profile {
            ProfileMode::UserDefault
        } else {
            // Fresh profile per launch: no cookies, storage, or session state
            // bleed between MCP sessions, and no crash-restore bubble from a
            // previously unclean shutdown. The directory is removed when the
            // browser stops (or swept by cdp-browser-lite after an abrupt kill).
            ProfileMode::Ephemeral
        };

        let _port = if self.secondary { 0 } else { self.port };

        let mut extra_args = if self.enable_automation {
            Vec::new()
        } else {
            vec!["--disable-infobars".to_string()]
        };
        // Presets may overlap, and a client may repeat one; emit each switch once.
        for switch in self.features.iter().flat_map(|f| f.switches()) {
            let switch = (*switch).to_string();
            if !extra_args.contains(&switch) {
                extra_args.push(switch);
            }
        }
        LaunchPlan {
            mode,
            profile,
            extra_args,
        }
    }

    pub(crate) fn resolve_port(&self) -> u16 {
        if self.secondary { 0 } else { self.port }
    }

    pub(crate) fn to_config(&self, plan: &LaunchPlan) -> BrowserConfig {
        let mut builder = BrowserConfig::builder()
            .mode(plan.mode.clone())
            .host(self.host.clone())
            .port(self.resolve_port())
            .headless(self.headless)
            .enable_automation(self.enable_automation)
            .profile(plan.profile.clone())
            .args(plan.extra_args.iter().cloned())
            .keep_alive_on_drop(false)
            .connect_timeout(std::time::Duration::from_secs(10))
            .command_timeout(std::time::Duration::from_secs(10))
            .startup_timeout(std::time::Duration::from_secs(10));
        if let Some(proxy) = &self.proxy {
            builder = builder.proxy(proxy.clone());
        }
        builder.build()
    }
}

fn is_local_host(host: &str) -> bool {
    let h = host.trim();
    matches!(h, "127.0.0.1" | "::1" | "[::1]") || h.eq_ignore_ascii_case("localhost")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_params() -> LaunchParams {
        LaunchParams::new("127.0.0.1".into(), 9222, false, true, false)
    }

    #[test]
    fn given_secondary_params_when_planning_then_mode_is_launch_new_with_ephemeral_port() {
        let mut params = LaunchParams::new("127.0.0.1".into(), 9222, false, false, false);
        params.secondary = true;
        let plan = params.plan();
        assert_eq!(plan.mode, LaunchMode::LaunchNew);
        assert_eq!(params.resolve_port(), 0);
        assert_eq!(plan.profile, ProfileMode::Ephemeral);
    }

    #[test]
    fn given_primary_params_on_local_host_when_planning_then_mode_is_auto() {
        let params = LaunchParams::new("127.0.0.1".into(), 9222, false, false, false);
        let plan = params.plan();
        assert_eq!(plan.mode, LaunchMode::Auto);
    }

    #[test]
    fn given_local_host_when_planning_then_mode_is_auto() {
        for host in ["127.0.0.1", "localhost", "::1", "LOCALHOST"] {
            let params = LaunchParams::new(host.into(), 9222, false, false, false);
            assert_eq!(
                params.plan().mode,
                LaunchMode::Auto,
                "host {host} should map to Auto"
            );
        }
    }

    #[test]
    fn given_remote_host_when_planning_then_mode_is_attach_only() {
        for host in [
            "10.0.0.5",
            "192.168.1.1",
            "example.com",
            "host.docker.internal",
        ] {
            let params = LaunchParams::new(host.into(), 9222, false, false, false);
            assert_eq!(
                params.plan().mode,
                LaunchMode::AttachOnly,
                "host {host} should map to AttachOnly"
            );
        }
    }

    #[test]
    fn given_user_profile_when_planning_then_profile_mode_is_user_default() {
        let params = LaunchParams::new("127.0.0.1".into(), 9222, false, false, true);
        assert_eq!(params.plan().profile, ProfileMode::UserDefault);
    }

    #[test]
    fn given_managed_profile_when_planning_then_profile_is_ephemeral() {
        let params = default_params();
        let plan = params.plan();
        assert_eq!(plan.profile, ProfileMode::Ephemeral);
    }

    #[test]
    fn given_automation_disabled_when_planning_then_disable_infobars_is_added() {
        let params = default_params();
        assert_eq!(
            params.plan().extra_args,
            vec!["--disable-infobars".to_string()]
        );
    }

    #[test]
    fn given_automation_enabled_when_planning_then_no_extra_args() {
        let params = LaunchParams::new("127.0.0.1".into(), 9222, true, false, false);
        assert!(params.plan().extra_args.is_empty());
    }

    #[test]
    fn given_plan_when_building_config_then_validate_succeeds() {
        let params = default_params();
        let plan = params.plan();
        let config = params.to_config(&plan);
        config
            .validate()
            .expect("default params + Auto + 127.0.0.1 + ephemeral chrome must validate");
    }

    #[test]
    fn given_proxy_set_when_building_config_then_port_accessor_matches_configured_port() {
        let mut params = default_params();
        params.set_proxy(Some("http://proxy.example.com:8080".to_string()));
        let plan = params.plan();
        let config = params.to_config(&plan);
        assert_eq!(config.port(), 9222);
    }

    #[test]
    fn given_web_mcp_feature_when_planning_then_its_switches_are_appended() {
        let mut params = default_params();
        params.set_features(vec![ChromeFeature::WebMcp]);
        assert_eq!(
            params.plan().extra_args,
            vec![
                "--disable-infobars".to_string(),
                "--enable-features=WebMCPTesting".to_string(),
                "--categoryExperimentalWebmcp=true".to_string(),
            ]
        );
    }

    #[test]
    fn given_webgl_software_feature_when_planning_then_its_switches_are_appended() {
        let mut params = LaunchParams::new("127.0.0.1".into(), 9222, true, true, false);
        params.set_features(vec![ChromeFeature::WebglSoftware]);
        assert_eq!(
            params.plan().extra_args,
            vec![
                "--use-gl=angle".to_string(),
                "--use-angle=swiftshader".to_string(),
                "--enable-unsafe-swiftshader".to_string(),
            ]
        );
    }

    #[test]
    fn given_several_features_when_planning_then_all_switches_are_present() {
        let mut params = default_params();
        params.set_features(vec![ChromeFeature::WebMcp, ChromeFeature::WebglSoftware]);
        let extra_args = params.plan().extra_args;
        for expected in ChromeFeature::WebMcp
            .switches()
            .iter()
            .chain(ChromeFeature::WebglSoftware.switches())
        {
            assert!(
                extra_args.contains(&(*expected).to_string()),
                "{expected} must be present in {extra_args:?}"
            );
        }
    }

    #[test]
    fn given_repeated_feature_when_planning_then_switches_are_not_duplicated() {
        let mut params = default_params();
        params.set_features(vec![ChromeFeature::WebMcp, ChromeFeature::WebMcp]);
        let extra_args = params.plan().extra_args;
        let occurrences = extra_args
            .iter()
            .filter(|a| *a == "--enable-features=WebMCPTesting")
            .count();
        assert_eq!(occurrences, 1, "got {extra_args:?}");
    }

    #[test]
    fn given_no_features_when_planning_then_no_feature_switches_are_added() {
        let params = default_params();
        assert_eq!(
            params.plan().extra_args,
            vec!["--disable-infobars".to_string()]
        );
    }

    #[test]
    fn given_feature_names_when_deserializing_then_screaming_snake_case_is_accepted() {
        let features: Vec<ChromeFeature> =
            serde_json::from_str(r#"["WEB_MCP","WEBGL_SOFTWARE"]"#).unwrap();
        assert_eq!(
            features,
            vec![ChromeFeature::WebMcp, ChromeFeature::WebglSoftware]
        );
    }

    #[test]
    fn given_unknown_feature_name_when_deserializing_then_it_is_rejected() {
        let parsed: Result<ChromeFeature, _> = serde_json::from_str(r#""DISABLE_WEB_SECURITY""#);
        assert!(parsed.is_err(), "unknown presets must not deserialize");
    }
}
