use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleState {
    Installed,
    Disabled,
    Running,
    Ready,
    Degraded,
    Stopped,
    Error,
    Removed,
}

impl ModuleState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Installed => "installed",
            Self::Disabled => "disabled",
            Self::Running => "running",
            Self::Ready => "ready",
            Self::Degraded => "degraded",
            Self::Stopped => "stopped",
            Self::Error => "error",
            Self::Removed => "removed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModuleErrorRecord {
    pub scope: String,
    pub code: String,
    pub message: String,
    pub detail: String,
    pub ts_ms: u64,
}

impl ModuleErrorRecord {
    pub fn new(scope: &str, code: &str, message: &str, detail: &str) -> Self {
        Self {
            scope: scope.to_string(),
            code: code.to_string(),
            message: message.to_string(),
            detail: detail.to_string(),
            ts_ms: now_ms(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleRecord {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub version: String,
    pub enabled: bool,
    pub state: ModuleState,
    pub reason: String,
    pub main_cap: String,
    pub action_count: u32,
    pub auto_start: bool,
    pub installed_at_ms: u64,
    pub updated_at_ms: u64,
    pub last_error: Option<ModuleErrorRecord>,
}

impl ModuleRecord {
    pub fn new(id: impl Into<String>) -> Self {
        let now = now_ms();
        let id_text = id.into();
        Self {
            id: id_text.clone(),
            name: id_text,
            kind: "module".to_string(),
            version: "0".to_string(),
            enabled: true,
            state: ModuleState::Installed,
            reason: "-".to_string(),
            main_cap: String::new(),
            action_count: 0,
            auto_start: false,
            installed_at_ms: now,
            updated_at_ms: now,
            last_error: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleRegistryError {
    pub code: &'static str,
    pub message: &'static str,
    pub module_id: String,
}

impl ModuleRegistryError {
    fn with_module(code: &'static str, message: &'static str, module_id: &str) -> Self {
        Self {
            code,
            message,
            module_id: module_id.to_string(),
        }
    }

    pub fn internal() -> Self {
        Self::with_module("E_INTERNAL", "internal_error", "-")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModuleReloadAllResult {
    pub total: u32,
    pub failed: u32,
    pub failed_ids: Vec<String>,
}

#[derive(Debug, Default)]
pub struct ModuleRegistry {
    records: BTreeMap<String, ModuleRecord>,
    last_error: Option<ModuleErrorRecord>,
    event_seq: u64,
}

impl ModuleRegistry {
    pub fn count(&self) -> usize {
        self.records.len()
    }

    pub fn event_seq(&self) -> u64 {
        self.event_seq
    }

    pub fn list(&self) -> Vec<ModuleRecord> {
        self.records.values().cloned().collect()
    }

    pub fn get(&self, module_id: &str) -> Option<ModuleRecord> {
        self.records.get(module_id).cloned()
    }

    pub fn upsert(
        &mut self,
        mut record: ModuleRecord,
    ) -> Result<ModuleRecord, ModuleRegistryError> {
        let id = sanitize_module_id(&record.id)?;
        record.id = id.clone();
        let now = now_ms();
        if let Some(existing) = self.records.get(&id) {
            record.installed_at_ms = existing.installed_at_ms;
        } else if record.installed_at_ms == 0 {
            record.installed_at_ms = now;
        }
        record.updated_at_ms = now;
        self.records.insert(id, record.clone());
        self.bump_event_seq();
        Ok(record)
    }

    pub fn remove(&mut self, module_id: &str) -> bool {
        let removed = self.records.remove(module_id).is_some();
        if removed {
            self.bump_event_seq();
        }
        removed
    }

    pub fn reload_by_id(&mut self, module_id: &str) -> Result<ModuleRecord, ModuleRegistryError> {
        let now = now_ms();
        let Some(rec) = self.records.get_mut(module_id) else {
            let err = ModuleRegistryError::with_module(
                "E_MODULE_NOT_FOUND",
                "module_not_found",
                module_id,
            );
            self.last_error = Some(ModuleErrorRecord::new(
                "module.lifecycle",
                err.code,
                err.message,
                &format!("id={}", module_id),
            ));
            return Err(err);
        };

        if !rec.enabled || rec.state == ModuleState::Disabled {
            let err =
                ModuleRegistryError::with_module("E_MODULE_DISABLED", "module_disabled", module_id);
            rec.state = ModuleState::Disabled;
            rec.reason = "disabled".to_string();
            rec.updated_at_ms = now;
            rec.last_error = Some(ModuleErrorRecord::new(
                "module.lifecycle",
                err.code,
                err.message,
                &format!("id={}", module_id),
            ));
            self.last_error = rec.last_error.clone();
            let _ = rec;
            self.bump_event_seq();
            return Err(err);
        }

        rec.state = ModuleState::Ready;
        rec.reason = "-".to_string();
        rec.updated_at_ms = now;
        rec.last_error = None;
        self.last_error = None;
        let out = rec.clone();
        let _ = rec;
        self.bump_event_seq();
        Ok(out)
    }

    pub fn reload_all(&mut self) -> ModuleReloadAllResult {
        let mut out = ModuleReloadAllResult::default();
        let ids: Vec<String> = self.records.keys().cloned().collect();
        for id in &ids {
            let enabled = self
                .records
                .get(id)
                .map(|v| v.enabled && v.state != ModuleState::Disabled)
                .unwrap_or(false);
            if !enabled {
                continue;
            }
            out.total = out.total.saturating_add(1);
            if self.reload_by_id(id).is_err() {
                out.failed = out.failed.saturating_add(1);
                out.failed_ids.push(id.clone());
            }
        }
        out
    }

    pub fn set_last_error(&mut self, record: ModuleErrorRecord) {
        self.last_error = Some(record);
        self.bump_event_seq();
    }

    pub fn clear_last_error(&mut self) {
        self.last_error = None;
        self.bump_event_seq();
    }

    pub fn last_error(&self) -> Option<ModuleErrorRecord> {
        self.last_error.clone()
    }

    fn bump_event_seq(&mut self) {
        self.event_seq = self.event_seq.saturating_add(1);
    }
}

fn sanitize_module_id(raw: &str) -> Result<String, ModuleRegistryError> {
    let id = raw.trim();
    if id.is_empty() {
        return Err(ModuleRegistryError::with_module(
            "E_MODULE_ID_INVALID",
            "module_id_invalid",
            raw,
        ));
    }
    let ok = id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-');
    if !ok {
        return Err(ModuleRegistryError::with_module(
            "E_MODULE_ID_INVALID",
            "module_id_invalid",
            raw,
        ));
    }
    Ok(id.to_string())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_and_list_keep_stable_order() {
        let mut reg = ModuleRegistry::default();

        let mut b = ModuleRecord::new("test.b");
        b.state = ModuleState::Running;
        reg.upsert(b).expect("upsert b");

        let mut a = ModuleRecord::new("test.a");
        a.state = ModuleState::Stopped;
        reg.upsert(a).expect("upsert a");

        let ids: Vec<String> = reg.list().into_iter().map(|v| v.id).collect();
        assert_eq!(ids, vec!["test.a".to_string(), "test.b".to_string()]);
        assert!(reg.event_seq() >= 2);
    }

    #[test]
    fn reload_by_id_reports_disabled() {
        let mut reg = ModuleRegistry::default();
        let mut r = ModuleRecord::new("dsapi.demo.touch_ui");
        r.enabled = false;
        r.state = ModuleState::Disabled;
        reg.upsert(r).expect("upsert");

        let err = reg
            .reload_by_id("dsapi.demo.touch_ui")
            .expect_err("disabled should fail");
        assert_eq!(err.code, "E_MODULE_DISABLED");
        let last = reg.last_error().expect("last error");
        assert_eq!(last.code, "E_MODULE_DISABLED");
    }

    #[test]
    fn reload_all_skips_disabled_and_collects_failed_ids() {
        let mut reg = ModuleRegistry::default();

        let mut ok = ModuleRecord::new("m.ok");
        ok.enabled = true;
        ok.state = ModuleState::Stopped;
        reg.upsert(ok).expect("upsert ok");

        let mut disabled = ModuleRecord::new("m.disabled");
        disabled.enabled = false;
        disabled.state = ModuleState::Disabled;
        reg.upsert(disabled).expect("upsert disabled");

        let out = reg.reload_all();
        assert_eq!(out.total, 1);
        assert_eq!(out.failed, 0);
        assert!(out.failed_ids.is_empty());

        let changed = reg.get("m.ok").expect("m.ok exists");
        assert_eq!(changed.state, ModuleState::Ready);
    }
}
