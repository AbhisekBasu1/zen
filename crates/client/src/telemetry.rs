use gpui::Task;
use std::{path::PathBuf, sync::Arc};

pub static MINIDUMP_ENDPOINT: std::sync::LazyLock<Option<String>> =
    std::sync::LazyLock::new(|| None);

static TELEMETRY: std::sync::LazyLock<Arc<Telemetry>> =
    std::sync::LazyLock::new(|| Arc::new(Telemetry));

#[derive(Clone)]
pub struct Telemetry;

impl Telemetry {
    pub fn global() -> &'static Arc<Self> {
        &TELEMETRY
    }

    pub fn log_file_path() -> PathBuf {
        PathBuf::new()
    }

    pub fn diagnostics_enabled(self: &Arc<Self>) -> bool {
        false
    }

    pub fn log_edit_event(self: &Arc<Self>, _: &'static str, _: bool) {}

    pub fn report_discovered_project_type_events<T, U>(&self, _: T, _: U) {}

    pub fn installation_id(self: &Arc<Self>) -> Option<Arc<str>> {
        None
    }

    pub fn flush_events(self: &Arc<Self>) -> Task<()> {
        Task::ready(())
    }
}
