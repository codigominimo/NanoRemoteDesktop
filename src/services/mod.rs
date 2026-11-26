#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "windows")]
mod windows;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ServiceStatus {
    pub status: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SystemService {
    pub name: String,
    pub label: String,
    pub status: String,
}

pub trait ServiceManager: Send + Sync {
    fn status(&self, name: &str) -> ServiceStatus;
    fn start(&self, name: &str) -> Result<(), String>;
    fn stop(&self, name: &str) -> Result<(), String>;
    fn restart(&self, name: &str) -> Result<(), String>;
    fn list_system_services(&self) -> Result<Vec<SystemService>, String>;
}

pub fn create_manager() -> Box<dyn ServiceManager> {
    #[cfg(target_os = "linux")]
    {
        Box::new(linux::LinuxServiceManager::new())
    }
    #[cfg(target_os = "windows")]
    {
        Box::new(windows::WindowsServiceManager)
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        Box::new(UnsupportedServiceManager)
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
struct UnsupportedServiceManager;

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
impl ServiceManager for UnsupportedServiceManager {
    fn status(&self, _name: &str) -> ServiceStatus {
        ServiceStatus {
            status: "unsupported".to_string(),
            detail: Some("Service management not supported on this OS".to_string()),
        }
    }
    fn start(&self, _name: &str) -> Result<(), String> {
        Err("unsupported OS".to_string())
    }
    fn stop(&self, _name: &str) -> Result<(), String> {
        Err("unsupported OS".to_string())
    }
    fn restart(&self, _name: &str) -> Result<(), String> {
        Err("unsupported OS".to_string())
    }
    fn list_system_services(&self) -> Result<Vec<SystemService>, String> {
        Err("unsupported OS".to_string())
    }
}
