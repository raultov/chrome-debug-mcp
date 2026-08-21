use crate::chrome_mcp_handler::BrowserSession;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock;

pub type InstanceId = String;

#[derive(Debug, Clone, ::serde::Serialize, ::serde::Deserialize)]
pub struct InstanceDescriptor {
    pub id: InstanceId,
    pub label: Option<String>,
    pub host: String,
    pub port: u16,
    pub profile_dir: Option<PathBuf>,
    pub features: Vec<String>,
    pub is_default: bool,
}

pub(crate) struct Registry {
    sessions: Arc<RwLock<HashMap<InstanceId, Arc<BrowserSession>>>>,
    descriptors: Arc<RwLock<HashMap<InstanceId, InstanceDescriptor>>>,
    counter: std::sync::atomic::AtomicUsize,
    pub(crate) max_instances: std::sync::atomic::AtomicUsize,
}

impl Registry {
    pub fn register_descriptor(&self, desc: InstanceDescriptor) {
        self.descriptors
            .write()
            .unwrap()
            .insert(desc.id.clone(), desc);
    }
    pub fn new(max_instances: usize) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            descriptors: Arc::new(RwLock::new(HashMap::new())),
            counter: std::sync::atomic::AtomicUsize::new(2), // start at 2 since "default" is 1 conceptually
            max_instances: std::sync::atomic::AtomicUsize::new(max_instances),
        }
    }

    pub fn get_session(&self, id: &str) -> Option<Arc<BrowserSession>> {
        self.sessions.read().unwrap().get(id).cloned()
    }

    pub fn add_session(
        &self,
        desc: InstanceDescriptor,
        session: Arc<BrowserSession>,
    ) -> Result<(), String> {
        let mut sessions = self.sessions.write().unwrap();
        if sessions.len() >= self.max_instances.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(format!(
                "Instance limit reached ({})",
                self.max_instances.load(std::sync::atomic::Ordering::SeqCst)
            ));
        }
        if sessions.contains_key(&desc.id) {
            return Err(format!("Instance id '{}' already exists", desc.id));
        }

        // Also check if label is already used by another instance
        let descriptors = self.descriptors.read().unwrap();
        if let Some(ref label) = desc.label {
            for existing in descriptors.values() {
                if existing.label.as_ref() == Some(label) {
                    return Err(format!(
                        "Label '{}' is already in use by instance '{}'",
                        label, existing.id
                    ));
                }
            }
        }
        drop(descriptors);

        self.descriptors
            .write()
            .unwrap()
            .insert(desc.id.clone(), desc.clone());
        sessions.insert(desc.id, session);
        Ok(())
    }

    pub fn remove_session(&self, id: &str) -> Option<Arc<BrowserSession>> {
        self.descriptors.write().unwrap().remove(id);
        self.sessions.write().unwrap().remove(id)
    }

    pub fn list_descriptors(&self) -> Vec<InstanceDescriptor> {
        let descriptors = self.descriptors.read().unwrap();
        descriptors.values().cloned().collect()
    }

    pub fn generate_id(&self) -> InstanceId {
        let num = self
            .counter
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        format!("chrome-{}", num)
    }
}
