use cdp_browser_lite::{BrowserConfig, LaunchMode, ProfileMode};

const PROFILE_ROOT_PREFIX: &str = "chrome-mcp-profile-";

#[derive(Debug, Clone)]
pub(crate) struct LaunchParams {
    host: String,
    port: u16,
    headless: bool,
    enable_automation: bool,
    user_profile: bool,
    proxy: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LaunchPlan {
    mode: LaunchMode,
    profile: ProfileMode,
    extra_args: Vec<String>,
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
            proxy: None,
        }
    }

    pub(crate) fn set_proxy(&mut self, proxy: Option<String>) {
        self.proxy = proxy;
    }

    pub(crate) fn configured_port(&self) -> u16 {
        self.port
    }

    pub(crate) fn plan(&self) -> LaunchPlan {
        let mode = if is_local_host(&self.host) {
            LaunchMode::Auto
        } else {
            LaunchMode::AttachOnly
        };
        let profile = if self.user_profile {
            ProfileMode::UserDefault
        } else {
            ProfileMode::PersistentPerPort {
                root: std::env::temp_dir(),
                prefix: PROFILE_ROOT_PREFIX.to_string(),
            }
        };
        let extra_args = if self.enable_automation {
            Vec::new()
        } else {
            vec!["--disable-infobars".to_string()]
        };
        LaunchPlan {
            mode,
            profile,
            extra_args,
        }
    }

    pub(crate) fn to_config(&self, plan: &LaunchPlan) -> BrowserConfig {
        let mut builder = BrowserConfig::builder()
            .mode(plan.mode.clone())
            .host(self.host.clone())
            .port(self.port)
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
    fn given_managed_profile_when_planning_then_profile_is_persistent_per_port() {
        let params = default_params();
        let plan = params.plan();
        assert_eq!(
            plan.profile,
            ProfileMode::PersistentPerPort {
                root: std::env::temp_dir(),
                prefix: PROFILE_ROOT_PREFIX.to_string(),
            }
        );
    }

    #[test]
    fn given_persistent_per_port_plan_when_resolving_dir_then_matches_configured_prefix() {
        let params = LaunchParams::new("127.0.0.1".into(), 12345, false, false, false);
        let plan = params.plan();
        let dir = plan.profile.dir_for_port(params.configured_port()).unwrap();
        assert_eq!(
            dir,
            std::env::temp_dir().join(format!("{PROFILE_ROOT_PREFIX}{}", params.configured_port()))
        );
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
}
