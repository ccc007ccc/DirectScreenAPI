use crate::api::Status;
use crate::engine::module_registry::{
    ModuleErrorRecord, ModuleRecord, ModuleRegistry, ModuleRegistryError, ModuleReloadAllResult,
    ModuleState,
};
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct ModuleRuntimeConfig {
    pub module_root_dir: PathBuf,
    pub module_state_root_dir: PathBuf,
    pub module_disabled_dir: PathBuf,
    pub registry_file: PathBuf,
    pub scope_file: PathBuf,
    pub action_timeout_sec: u64,
}

impl Default for ModuleRuntimeConfig {
    fn default() -> Self {
        static MODULE_RUNTIME_COUNTER: AtomicU64 = AtomicU64::new(0);
        let seq = MODULE_RUNTIME_COUNTER.fetch_add(1, Ordering::Relaxed);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros();
        let base = PathBuf::from(format!(
            "artifacts/module_runtime/default_{}_{}_{}",
            std::process::id(),
            now,
            seq
        ));
        let state = base.join("state");
        Self {
            module_root_dir: base.join("modules"),
            module_state_root_dir: state.join("modules"),
            module_disabled_dir: state.join("modules_disabled"),
            registry_file: state.join("module_registry.db"),
            scope_file: state.join("module_scope.db"),
            action_timeout_sec: 60,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleScopeRule {
    pub package_name: String,
    pub user_id: i32,
    pub allow: bool,
}

#[derive(Debug, Clone)]
struct ScopeContext {
    package_name: String,
    user_id: i32,
}

#[derive(Debug, Clone)]
struct ScopeDecision {
    allow: bool,
    matched: Option<ModuleScopeRule>,
}

#[derive(Debug, Clone)]
struct ExecOutput {
    code: i32,
    stdout: String,
    stderr: String,
    timed_out: bool,
}

#[derive(Debug, Clone)]
pub struct ModuleRuntimeRpcError {
    pub status: Status,
    pub body: String,
}

impl ModuleRuntimeRpcError {
    pub fn invalid(msg: impl Into<String>) -> Self {
        Self {
            status: Status::InvalidArgument,
            body: msg.into(),
        }
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self {
            status: Status::InternalError,
            body: msg.into(),
        }
    }
}

#[derive(Debug)]
pub struct ModuleRuntime {
    cfg: ModuleRuntimeConfig,
    registry: ModuleRegistry,
    scope_rules: BTreeMap<String, Vec<ModuleScopeRule>>,
}

impl ModuleRuntime {
    pub fn new(cfg: ModuleRuntimeConfig) -> Self {
        let mut out = Self {
            cfg,
            registry: ModuleRegistry::default(),
            scope_rules: BTreeMap::new(),
        };
        let _ = out.ensure_layout();
        let _ = out.load_registry_file();
        let _ = out.load_scope_file();
        let _ = out.sync_from_disk();
        out
    }

    pub fn upsert(&mut self, record: ModuleRecord) -> Result<ModuleRecord, ModuleRegistryError> {
        let out = self.registry.upsert(record)?;
        self.persist_registry_best_effort();
        Ok(out)
    }

    pub fn remove(&mut self, module_id: &str) -> bool {
        let removed = self.registry.remove(module_id);
        if removed {
            self.persist_registry_best_effort();
        }
        removed
    }

    pub fn get(&self, module_id: &str) -> Option<ModuleRecord> {
        self.registry.get(module_id)
    }

    pub fn list(&self) -> Vec<ModuleRecord> {
        self.registry.list()
    }

    pub fn reload_by_id(&mut self, module_id: &str) -> Result<ModuleRecord, ModuleRegistryError> {
        let rec = self.registry.reload_by_id(module_id)?;
        self.persist_registry_best_effort();
        Ok(rec)
    }

    pub fn reload_all(&mut self) -> ModuleReloadAllResult {
        let out = self.registry.reload_all();
        self.persist_registry_best_effort();
        out
    }

    pub fn set_last_error(&mut self, record: ModuleErrorRecord) {
        self.registry.set_last_error(record);
        self.persist_registry_best_effort();
    }

    pub fn clear_last_error(&mut self) {
        self.registry.clear_last_error();
        self.persist_registry_best_effort();
    }

    pub fn last_error(&self) -> Option<ModuleErrorRecord> {
        self.registry.last_error()
    }

    pub fn event_seq(&self) -> u64 {
        self.registry.event_seq()
    }

    pub fn count(&self) -> usize {
        self.registry.count()
    }

    pub fn sync_from_disk(&mut self) -> Result<(), ModuleRegistryError> {
        self.ensure_layout()
            .map_err(|_| ModuleRegistryError::internal())?;
        let entries = self.module_dirs_on_disk();
        let mut live_ids: Vec<String> = Vec::new();

        for (module_id, module_dir) in &entries {
            live_ids.push(module_id.clone());
            let meta_file = module_dir.join("dsapi.module");

            let mut rec = self
                .registry
                .get(module_id)
                .unwrap_or_else(|| ModuleRecord::new(module_id));
            rec.name =
                module_meta_with_alias(&meta_file, "MODULE_NAME", "DSAPI_MODULE_NAME", module_id);
            rec.kind =
                module_meta_with_alias(&meta_file, "MODULE_KIND", "DSAPI_MODULE_KIND", "module");
            rec.version =
                module_meta_with_alias(&meta_file, "MODULE_VERSION", "DSAPI_MODULE_VERSION", "0");
            rec.main_cap =
                module_meta_with_alias(&meta_file, "MAIN_CAP_ID", "DSAPI_MAIN_CAP_ID", "");
            rec.auto_start = parse_bool_like(&module_meta_with_alias(
                &meta_file,
                "MODULE_AUTO_START",
                "DSAPI_MODULE_AUTO_START",
                "0",
            ));
            rec.action_count = count_scripts(&module_dir.join("actions")).unwrap_or(0) as u32;
            rec.enabled = !self.is_module_disabled(module_id);
            if !rec.enabled {
                rec.state = ModuleState::Disabled;
                rec.reason = "disabled".to_string();
            } else if rec.state == ModuleState::Disabled {
                rec.state = ModuleState::Installed;
                rec.reason = "-".to_string();
            }

            self.registry.upsert(rec)?;
        }

        let existing_ids: Vec<String> = self.registry.list().into_iter().map(|v| v.id).collect();
        for id in existing_ids {
            if !live_ids.iter().any(|v| v == &id) {
                let _ = self.registry.remove(&id);
                self.scope_rules.remove(&id);
            }
        }

        self.persist_registry_best_effort();
        self.persist_scope_best_effort();
        Ok(())
    }

    pub fn handle_rpc(
        &mut self,
        request: &str,
        peer_uid: u32,
    ) -> Result<String, ModuleRuntimeRpcError> {
        let mut tokens: Vec<&str> = request.split_whitespace().collect();
        if tokens.is_empty() {
            return Err(ModuleRuntimeRpcError::invalid(
                "ksu_dsapi_error=module_rpc_empty",
            ));
        }
        let cmd = tokens.remove(0);
        let cmd_upper = cmd.to_ascii_uppercase();
        self.sync_from_disk()
            .map_err(|_| ModuleRuntimeRpcError::internal("ksu_dsapi_error=module_sync_failed"))?;

        match cmd_upper.as_str() {
            "MODULE_SYNC" => {
                let line = format!(
                    "module_action=synced total={} event_seq={}",
                    self.registry.count(),
                    self.registry.event_seq()
                );
                Ok(line)
            }
            "MODULE_LIST" => self.module_list_lines(),
            "MODULE_STATUS" => {
                if tokens.len() != 1 {
                    return Err(ModuleRuntimeRpcError::invalid(
                        "ksu_dsapi_error=module_id_missing",
                    ));
                }
                let module_id = tokens[0];
                let status = self.module_status_line(module_id)?;
                Ok(format!("module_status id={} {}", module_id, status))
            }
            "MODULE_DETAIL" => {
                if tokens.len() != 1 {
                    return Err(ModuleRuntimeRpcError::invalid(
                        "ksu_dsapi_error=module_id_missing",
                    ));
                }
                self.module_detail_lines(tokens[0])
            }
            "MODULE_START" => {
                if tokens.is_empty() {
                    return Err(ModuleRuntimeRpcError::invalid(
                        "ksu_dsapi_error=module_id_missing",
                    ));
                }
                let module_id = tokens[0];
                let scope = parse_scope_context(&tokens, 1, peer_uid as i32);
                self.ensure_scope_allowed(module_id, &scope)?;
                self.module_start(module_id)?;
                Ok(format!(
                    "module_action=started id={} package={} user={}",
                    module_id, scope.package_name, scope.user_id
                ))
            }
            "MODULE_STOP" => {
                if tokens.len() != 1 {
                    return Err(ModuleRuntimeRpcError::invalid(
                        "ksu_dsapi_error=module_id_missing",
                    ));
                }
                let module_id = tokens[0];
                self.module_stop(module_id)?;
                Ok(format!("module_action=stopped id={}", module_id))
            }
            "MODULE_RELOAD" => {
                if tokens.is_empty() {
                    return Err(ModuleRuntimeRpcError::invalid(
                        "ksu_dsapi_error=module_id_missing",
                    ));
                }
                let module_id = tokens[0];
                let scope = parse_scope_context(&tokens, 1, peer_uid as i32);
                self.ensure_scope_allowed(module_id, &scope)?;
                self.module_reload(module_id)?;
                Ok(format!(
                    "module_action=reloaded id={} package={} user={}",
                    module_id, scope.package_name, scope.user_id
                ))
            }
            "MODULE_RELOAD_ALL" => {
                let scope = parse_scope_context(&tokens, 0, peer_uid as i32);
                let all_ids: Vec<String> = self.registry.list().into_iter().map(|v| v.id).collect();
                let mut total = 0u32;
                let mut failed = 0u32;
                let mut failed_ids: Vec<String> = Vec::new();
                for id in all_ids {
                    let rec = self.registry.get(&id).ok_or_else(|| {
                        ModuleRuntimeRpcError::internal("ksu_dsapi_error=module_lookup_failed")
                    })?;
                    if !rec.enabled {
                        continue;
                    }
                    if !self.scope_eval(&id, &scope).allow {
                        continue;
                    }
                    total = total.saturating_add(1);
                    if self.module_reload(&id).is_err() {
                        failed = failed.saturating_add(1);
                        failed_ids.push(id);
                    }
                }
                let failed_text = if failed_ids.is_empty() {
                    "-".to_string()
                } else {
                    failed_ids.join(",")
                };
                let line = format!(
                    "module_action=reloaded_all total={} failed={} failed_ids={}",
                    total, failed, failed_text
                );
                if failed > 0 {
                    return Err(ModuleRuntimeRpcError::internal(line));
                }
                Ok(line)
            }
            "MODULE_DISABLE" => {
                if tokens.len() != 1 {
                    return Err(ModuleRuntimeRpcError::invalid(
                        "ksu_dsapi_error=module_id_missing",
                    ));
                }
                let module_id = tokens[0];
                self.module_disable(module_id)?;
                Ok(format!("module_action=disabled id={}", module_id))
            }
            "MODULE_ENABLE" => {
                if tokens.len() != 1 {
                    return Err(ModuleRuntimeRpcError::invalid(
                        "ksu_dsapi_error=module_id_missing",
                    ));
                }
                let module_id = tokens[0];
                self.module_enable(module_id)?;
                Ok(format!("module_action=enabled id={}", module_id))
            }
            "MODULE_REMOVE" => {
                if tokens.len() != 1 {
                    return Err(ModuleRuntimeRpcError::invalid(
                        "ksu_dsapi_error=module_id_missing",
                    ));
                }
                let module_id = tokens[0];
                self.module_remove(module_id)?;
                Ok(format!("module_action=removed id={}", module_id))
            }
            "MODULE_ACTION_LIST" => {
                if tokens.len() != 1 {
                    return Err(ModuleRuntimeRpcError::invalid(
                        "ksu_dsapi_error=module_id_missing",
                    ));
                }
                self.module_action_list_lines(tokens[0])
            }
            "MODULE_ACTION_RUN" => {
                if tokens.len() < 2 {
                    return Err(ModuleRuntimeRpcError::invalid(
                        "ksu_dsapi_error=module_action_missing",
                    ));
                }
                let module_id = tokens[0];
                let action_id = tokens[1];
                let scope = parse_scope_context(&tokens, 2, peer_uid as i32);
                self.ensure_scope_allowed(module_id, &scope)?;
                let out = self.module_action_run(module_id, action_id)?;
                let mut text = String::new();
                if !out.trim().is_empty() {
                    text.push_str(out.trim_end());
                    text.push('\n');
                }
                text.push_str(&format!(
                    "module_action=ran id={} action={} package={} user={}",
                    module_id, action_id, scope.package_name, scope.user_id
                ));
                Ok(text)
            }
            "MODULE_ENV_LIST" => {
                if tokens.len() != 1 {
                    return Err(ModuleRuntimeRpcError::invalid(
                        "ksu_dsapi_error=module_id_missing",
                    ));
                }
                self.module_env_list_lines(tokens[0])
            }
            "MODULE_ENV_SET" => {
                if tokens.len() < 3 {
                    return Err(ModuleRuntimeRpcError::invalid(
                        "ksu_dsapi_error=module_env_set_args_invalid",
                    ));
                }
                let module_id = tokens[0];
                let key = tokens[1];
                let value = tokens[2..].join(" ");
                self.module_env_set(module_id, key, &value)?;
                Ok(format!("module_env=updated id={} key={}", module_id, key))
            }
            "MODULE_ENV_UNSET" => {
                if tokens.len() != 2 {
                    return Err(ModuleRuntimeRpcError::invalid(
                        "ksu_dsapi_error=module_env_unset_args_invalid",
                    ));
                }
                let module_id = tokens[0];
                let key = tokens[1];
                self.module_env_unset(module_id, key)?;
                Ok(format!("module_env=unset id={} key={}", module_id, key))
            }
            "MODULE_SCOPE_LIST" => {
                let module_id = if tokens.is_empty() {
                    None
                } else {
                    Some(tokens[0])
                };
                self.scope_list_lines(module_id)
            }
            "MODULE_SCOPE_SET" => {
                if tokens.len() != 4 {
                    return Err(ModuleRuntimeRpcError::invalid(
                        "ksu_dsapi_error=module_scope_set_args_invalid",
                    ));
                }
                let module_id = tokens[0];
                let pkg = tokens[1];
                let user_id = tokens[2].parse::<i32>().map_err(|_| {
                    ModuleRuntimeRpcError::invalid("ksu_dsapi_error=scope_user_invalid")
                })?;
                let allow = parse_scope_allow(tokens[3]).ok_or_else(|| {
                    ModuleRuntimeRpcError::invalid("ksu_dsapi_error=scope_action_invalid")
                })?;
                self.scope_set(module_id, pkg, user_id, allow)?;
                Ok(format!(
                    "module_scope=updated id={} package={} user={} policy={}",
                    module_id,
                    pkg,
                    user_id,
                    if allow { "allow" } else { "deny" }
                ))
            }
            "MODULE_SCOPE_CLEAR" => {
                if tokens.len() != 1 {
                    return Err(ModuleRuntimeRpcError::invalid(
                        "ksu_dsapi_error=module_id_missing",
                    ));
                }
                self.scope_clear(tokens[0])?;
                Ok(format!("module_scope=cleared id={}", tokens[0]))
            }
            _ => Err(ModuleRuntimeRpcError::invalid(format!(
                "ksu_dsapi_error=module_rpc_unknown_command command={}",
                cmd
            ))),
        }
    }

    fn module_list_lines(&mut self) -> Result<String, ModuleRuntimeRpcError> {
        let ids: Vec<String> = self.registry.list().into_iter().map(|v| v.id).collect();
        let mut lines: Vec<String> = Vec::new();
        for id in ids {
            let line = self.module_row_line(&id)?;
            lines.push(line);
        }
        Ok(lines.join("\n"))
    }

    fn module_status_line(&mut self, module_id: &str) -> Result<String, ModuleRuntimeRpcError> {
        self.refresh_status(module_id)?;
        let rec = self
            .registry
            .get(module_id)
            .ok_or_else(|| ModuleRuntimeRpcError::invalid("ksu_dsapi_error=module_not_found"))?;
        let pid = module_last_pid(module_id, &self.cfg.module_state_root_dir);
        Ok(format!(
            "state={} pid={} reason={} enabled={} main_cap={}",
            rec.state.as_str(),
            pid,
            sanitize_token(&rec.reason),
            if rec.enabled { 1 } else { 0 },
            sanitize_token(&rec.main_cap)
        ))
    }

    fn module_row_line(&mut self, module_id: &str) -> Result<String, ModuleRuntimeRpcError> {
        self.refresh_status(module_id)?;
        let rec = self
            .registry
            .get(module_id)
            .ok_or_else(|| ModuleRuntimeRpcError::invalid("ksu_dsapi_error=module_not_found"))?;
        Ok(format!(
            "module_row={}|{}|{}|{}|{}|{}|{}|{}|{}",
            rec.id,
            sanitize_pipe_field(&rec.name),
            sanitize_pipe_field(&rec.kind),
            sanitize_pipe_field(&rec.version),
            rec.state.as_str(),
            if rec.enabled { 1 } else { 0 },
            sanitize_pipe_field(&rec.main_cap),
            rec.action_count,
            sanitize_pipe_field(&rec.reason)
        ))
    }

    fn module_detail_lines(&mut self, module_id: &str) -> Result<String, ModuleRuntimeRpcError> {
        self.refresh_status(module_id)?;
        let rec = self
            .registry
            .get(module_id)
            .ok_or_else(|| ModuleRuntimeRpcError::invalid("ksu_dsapi_error=module_not_found"))?;
        let meta_file = self.module_dir(module_id).join("dsapi.module");
        let desc = module_meta_with_alias(&meta_file, "MODULE_DESC", "DSAPI_MODULE_DESC", "-");

        let mut lines: Vec<String> = vec![
            format!("id={}", rec.id),
            format!("name={}", sanitize_pipe_field(&rec.name)),
            format!("kind={}", sanitize_pipe_field(&rec.kind)),
            format!("version={}", sanitize_pipe_field(&rec.version)),
            format!("desc={}", sanitize_pipe_field(&desc)),
            format!("status={}", self.module_status_line(module_id)?),
        ];

        let actions =
            action_entries(&self.module_dir(module_id).join("actions")).unwrap_or_default();
        for act in actions {
            lines.push(format!(
                "module_action_row={}|{}|{}",
                act.id,
                sanitize_pipe_field(&act.name),
                if act.danger { 1 } else { 0 }
            ));
        }

        let env_lines = module_env_lines(module_id, &self.module_dir(module_id));
        for item in env_lines {
            lines.push(format!(
                "module_env_row={}|{}|{}|{}|{}|{}",
                sanitize_pipe_field(&item.key),
                sanitize_pipe_field(&item.value),
                sanitize_pipe_field(&item.default_value),
                sanitize_pipe_field(&item.value_type),
                sanitize_pipe_field(&item.label),
                sanitize_pipe_field(&item.desc)
            ));
        }
        Ok(lines.join("\n"))
    }

    fn module_start(&mut self, module_id: &str) -> Result<(), ModuleRuntimeRpcError> {
        self.ensure_module_exists(module_id)?;
        let rec = self
            .registry
            .get(module_id)
            .ok_or_else(|| ModuleRuntimeRpcError::invalid("ksu_dsapi_error=module_not_found"))?;
        if !rec.enabled {
            self.record_error(
                module_id,
                "E_MODULE_DISABLED",
                "module_disabled",
                "op=start",
            );
            return Err(ModuleRuntimeRpcError::invalid(
                "ksu_dsapi_error=module_disabled",
            ));
        }

        if self.action_file(module_id, "start").exists() {
            let out = self.exec_action(module_id, "start")?;
            self.commit_exec_state(module_id, &out, ModuleState::Ready);
            if out.code != 0 {
                return Err(ModuleRuntimeRpcError::internal(
                    "ksu_dsapi_error=module_start_failed",
                ));
            }
            return Ok(());
        }

        let out = self.exec_main_cap(module_id, "cap_start")?;
        self.commit_exec_state(module_id, &out, ModuleState::Ready);
        if out.code != 0 {
            return Err(ModuleRuntimeRpcError::internal(
                "ksu_dsapi_error=module_start_failed",
            ));
        }
        Ok(())
    }

    fn module_stop(&mut self, module_id: &str) -> Result<(), ModuleRuntimeRpcError> {
        self.ensure_module_exists(module_id)?;

        if self.action_file(module_id, "stop").exists() {
            let out = self.exec_action(module_id, "stop")?;
            self.commit_exec_state(module_id, &out, ModuleState::Stopped);
            if out.code != 0 {
                return Err(ModuleRuntimeRpcError::internal(
                    "ksu_dsapi_error=module_stop_failed",
                ));
            }
            return Ok(());
        }

        let out = self.exec_main_cap(module_id, "cap_stop")?;
        self.commit_exec_state(module_id, &out, ModuleState::Stopped);
        if out.code != 0 {
            return Err(ModuleRuntimeRpcError::internal(
                "ksu_dsapi_error=module_stop_failed",
            ));
        }
        Ok(())
    }

    fn module_reload(&mut self, module_id: &str) -> Result<(), ModuleRuntimeRpcError> {
        self.ensure_module_exists(module_id)?;
        let _ = self.module_stop(module_id);
        self.module_start(module_id)
    }

    fn module_disable(&mut self, module_id: &str) -> Result<(), ModuleRuntimeRpcError> {
        self.ensure_module_exists(module_id)?;
        let _ = self.module_stop(module_id);
        let flag = self
            .cfg
            .module_disabled_dir
            .join(format!("{}.disabled", module_id));
        write_atomic_text(&flag, "1\n").map_err(|_| {
            ModuleRuntimeRpcError::internal("ksu_dsapi_error=module_disable_write_failed")
        })?;
        let mut rec = self
            .registry
            .get(module_id)
            .ok_or_else(|| ModuleRuntimeRpcError::invalid("ksu_dsapi_error=module_not_found"))?;
        rec.enabled = false;
        rec.state = ModuleState::Disabled;
        rec.reason = "disabled".to_string();
        let _ = self.registry.upsert(rec);
        self.persist_registry_best_effort();
        Ok(())
    }

    fn module_enable(&mut self, module_id: &str) -> Result<(), ModuleRuntimeRpcError> {
        self.ensure_module_exists(module_id)?;
        let flag = self
            .cfg
            .module_disabled_dir
            .join(format!("{}.disabled", module_id));
        let _ = fs::remove_file(flag);
        let mut rec = self
            .registry
            .get(module_id)
            .ok_or_else(|| ModuleRuntimeRpcError::invalid("ksu_dsapi_error=module_not_found"))?;
        rec.enabled = true;
        if rec.state == ModuleState::Disabled {
            rec.state = ModuleState::Installed;
            rec.reason = "-".to_string();
        }
        let _ = self.registry.upsert(rec);
        self.persist_registry_best_effort();
        Ok(())
    }

    fn module_remove(&mut self, module_id: &str) -> Result<(), ModuleRuntimeRpcError> {
        self.ensure_module_exists(module_id)?;
        let _ = self.module_stop(module_id);
        let _ = fs::remove_dir_all(self.module_dir(module_id));
        let _ = fs::remove_dir_all(self.module_state_dir(module_id));
        let _ = fs::remove_file(
            self.cfg
                .module_disabled_dir
                .join(format!("{}.disabled", module_id)),
        );
        let _ = self.registry.remove(module_id);
        self.scope_rules.remove(module_id);
        self.persist_registry_best_effort();
        self.persist_scope_best_effort();
        Ok(())
    }

    fn module_action_list_lines(&self, module_id: &str) -> Result<String, ModuleRuntimeRpcError> {
        self.ensure_module_exists_ro(module_id)?;
        let mut lines: Vec<String> = Vec::new();
        let actions =
            action_entries(&self.module_dir(module_id).join("actions")).unwrap_or_default();
        for act in actions {
            lines.push(format!(
                "module_action_row={}|{}|{}",
                act.id,
                sanitize_pipe_field(&act.name),
                if act.danger { 1 } else { 0 }
            ));
        }
        Ok(lines.join("\n"))
    }

    fn module_action_run(
        &mut self,
        module_id: &str,
        action_id: &str,
    ) -> Result<String, ModuleRuntimeRpcError> {
        self.ensure_module_exists(module_id)?;
        let out = self.exec_action(module_id, action_id)?;
        self.commit_exec_state(module_id, &out, ModuleState::Ready);
        if out.code != 0 {
            return Err(ModuleRuntimeRpcError::internal(format!(
                "ksu_dsapi_error=module_action_failed id={} action={} exit={}",
                module_id, action_id, out.code
            )));
        }
        Ok(out.stdout)
    }

    fn module_env_list_lines(&self, module_id: &str) -> Result<String, ModuleRuntimeRpcError> {
        self.ensure_module_exists_ro(module_id)?;
        let lines = module_env_lines(module_id, &self.module_dir(module_id));
        let mut out: Vec<String> = Vec::new();
        for item in lines {
            out.push(format!(
                "module_env_row={}|{}|{}|{}|{}|{}",
                sanitize_pipe_field(&item.key),
                sanitize_pipe_field(&item.value),
                sanitize_pipe_field(&item.default_value),
                sanitize_pipe_field(&item.value_type),
                sanitize_pipe_field(&item.label),
                sanitize_pipe_field(&item.desc)
            ));
        }
        Ok(out.join("\n"))
    }

    fn module_env_set(
        &mut self,
        module_id: &str,
        key: &str,
        value: &str,
    ) -> Result<(), ModuleRuntimeRpcError> {
        self.ensure_module_exists(module_id)?;
        if !env_key_valid(key) {
            return Err(ModuleRuntimeRpcError::invalid(
                "ksu_dsapi_error=module_env_key_invalid",
            ));
        }
        let env_file = self.module_dir(module_id).join("env.values");
        let mut map = read_env_values_map(&env_file).unwrap_or_default();
        map.insert(key.to_string(), value.to_string());
        write_env_values_map(&env_file, &map).map_err(|_| {
            ModuleRuntimeRpcError::internal("ksu_dsapi_error=module_env_set_failed")
        })?;
        Ok(())
    }

    fn module_env_unset(
        &mut self,
        module_id: &str,
        key: &str,
    ) -> Result<(), ModuleRuntimeRpcError> {
        self.ensure_module_exists(module_id)?;
        if !env_key_valid(key) {
            return Err(ModuleRuntimeRpcError::invalid(
                "ksu_dsapi_error=module_env_key_invalid",
            ));
        }
        let env_file = self.module_dir(module_id).join("env.values");
        let mut map = read_env_values_map(&env_file).unwrap_or_default();
        map.remove(key);
        write_env_values_map(&env_file, &map).map_err(|_| {
            ModuleRuntimeRpcError::internal("ksu_dsapi_error=module_env_unset_failed")
        })?;
        Ok(())
    }

    fn scope_list_lines(&self, module_id: Option<&str>) -> Result<String, ModuleRuntimeRpcError> {
        let mut lines: Vec<String> = Vec::new();
        match module_id {
            Some(id) => {
                let rules = self.scope_rules.get(id).cloned().unwrap_or_default();
                for rule in rules {
                    lines.push(scope_rule_line(id, &rule));
                }
            }
            None => {
                for (id, rules) in &self.scope_rules {
                    for rule in rules {
                        lines.push(scope_rule_line(id, rule));
                    }
                }
            }
        }
        Ok(lines.join("\n"))
    }

    fn scope_set(
        &mut self,
        module_id: &str,
        package_name: &str,
        user_id: i32,
        allow: bool,
    ) -> Result<(), ModuleRuntimeRpcError> {
        self.ensure_module_exists(module_id)?;
        if !scope_package_valid(package_name) {
            return Err(ModuleRuntimeRpcError::invalid(
                "ksu_dsapi_error=scope_package_invalid",
            ));
        }
        let rules = self.scope_rules.entry(module_id.to_string()).or_default();
        if let Some(existing) = rules
            .iter_mut()
            .find(|v| v.package_name == package_name && v.user_id == user_id)
        {
            existing.allow = allow;
        } else {
            rules.push(ModuleScopeRule {
                package_name: package_name.to_string(),
                user_id,
                allow,
            });
        }
        self.persist_scope_best_effort();
        Ok(())
    }

    fn scope_clear(&mut self, module_id: &str) -> Result<(), ModuleRuntimeRpcError> {
        self.ensure_module_exists(module_id)?;
        self.scope_rules.remove(module_id);
        self.persist_scope_best_effort();
        Ok(())
    }

    fn ensure_scope_allowed(
        &mut self,
        module_id: &str,
        scope: &ScopeContext,
    ) -> Result<(), ModuleRuntimeRpcError> {
        let decision = self.scope_eval(module_id, scope);
        if decision.allow {
            return Ok(());
        }

        let detail = if let Some(rule) = decision.matched {
            format!(
                "id={} package={} user={} rule_package={} rule_user={}",
                module_id, scope.package_name, scope.user_id, rule.package_name, rule.user_id
            )
        } else {
            format!(
                "id={} package={} user={} rule=none",
                module_id, scope.package_name, scope.user_id
            )
        };
        self.record_error(module_id, "E_SCOPE_DENIED", "scope_denied", &detail);
        Err(ModuleRuntimeRpcError::invalid(format!(
            "ksu_dsapi_error=scope_denied {}",
            sanitize_token(&detail)
        )))
    }

    fn scope_eval(&self, module_id: &str, scope: &ScopeContext) -> ScopeDecision {
        let Some(rules) = self.scope_rules.get(module_id) else {
            return ScopeDecision {
                allow: true,
                matched: None,
            };
        };
        let mut picked_score: i32 = -1;
        let mut picked: Option<ModuleScopeRule> = None;

        for rule in rules {
            if rule.package_name != "*" && rule.package_name != scope.package_name {
                continue;
            }
            if rule.user_id != -1 && rule.user_id != scope.user_id {
                continue;
            }
            let pkg_score = if rule.package_name == "*" { 0 } else { 2 };
            let user_score = if rule.user_id == -1 { 0 } else { 1 };
            let score = pkg_score + user_score;
            if score >= picked_score {
                picked_score = score;
                picked = Some(rule.clone());
            }
        }

        match picked {
            Some(rule) => ScopeDecision {
                allow: rule.allow,
                matched: Some(rule),
            },
            None => ScopeDecision {
                allow: true,
                matched: None,
            },
        }
    }

    fn refresh_status(&mut self, module_id: &str) -> Result<(), ModuleRuntimeRpcError> {
        let mut rec = self
            .registry
            .get(module_id)
            .ok_or_else(|| ModuleRuntimeRpcError::invalid("ksu_dsapi_error=module_not_found"))?;
        rec.enabled = !self.is_module_disabled(module_id);
        if !rec.enabled {
            rec.state = ModuleState::Disabled;
            rec.reason = "disabled".to_string();
            let _ = self.registry.upsert(rec);
            self.persist_registry_best_effort();
            return Ok(());
        }

        if self.action_file(module_id, "status").exists() {
            let out = self.exec_action(module_id, "status")?;
            if out.code != 0 {
                self.commit_exec_state(module_id, &out, ModuleState::Error);
                return Ok(());
            }
            let status = parse_status_kv(&out.stdout);
            if let Some(v) = status.state {
                rec.state = v;
            }
            rec.reason = status.reason.unwrap_or_else(|| "-".to_string());
            rec.last_error = None;
            let _ = self.registry.upsert(rec);
            self.persist_registry_best_effort();
            return Ok(());
        }

        if self.main_cap_file(module_id).is_some() {
            let out = self.exec_main_cap(module_id, "cap_status")?;
            if out.code != 0 {
                self.commit_exec_state(module_id, &out, ModuleState::Error);
                return Ok(());
            }
            let status = parse_status_kv(&out.stdout);
            if let Some(v) = status.state {
                rec.state = v;
            }
            rec.reason = status.reason.unwrap_or_else(|| "-".to_string());
            rec.last_error = None;
            let _ = self.registry.upsert(rec);
            self.persist_registry_best_effort();
        }
        Ok(())
    }

    fn commit_exec_state(&mut self, module_id: &str, out: &ExecOutput, ok_state: ModuleState) {
        if out.code == 0 {
            let status = parse_status_kv(&out.stdout);
            let reason = status.reason.unwrap_or_else(|| "-".to_string());
            self.commit_module_state(module_id, status.state.unwrap_or(ok_state), &reason);
            self.clear_last_error();
            return;
        }

        if out.timed_out {
            self.record_error(
                module_id,
                "E_ACTION_TIMEOUT",
                "module_action_timeout",
                "timeout",
            );
        } else {
            self.record_error(
                module_id,
                "E_ACTION_FAILED",
                "module_action_failed",
                &format!("exit={} stderr={}", out.code, sanitize_token(&out.stderr)),
            );
        }
        self.commit_module_state(module_id, ModuleState::Error, "action_failed");
    }

    fn commit_module_state(&mut self, module_id: &str, state: ModuleState, reason: &str) {
        if let Some(mut rec) = self.registry.get(module_id) {
            rec.state = state;
            rec.reason = reason.to_string();
            let _ = self.registry.upsert(rec);
            self.persist_registry_best_effort();
        }
    }

    fn record_error(&mut self, module_id: &str, code: &str, message: &str, detail: &str) {
        let mut rec = match self.registry.get(module_id) {
            Some(v) => v,
            None => return,
        };
        let err = ModuleErrorRecord::new("module.lifecycle", code, message, detail);
        rec.last_error = Some(err.clone());
        self.registry.set_last_error(err);
        let _ = self.registry.upsert(rec);
        self.persist_registry_best_effort();
    }

    fn exec_action(
        &self,
        module_id: &str,
        action_id: &str,
    ) -> Result<ExecOutput, ModuleRuntimeRpcError> {
        let action_file = self.action_file(module_id, action_id);
        if !action_file.exists() {
            return Err(ModuleRuntimeRpcError::invalid(format!(
                "ksu_dsapi_error=module_action_missing id={} action={}",
                module_id, action_id
            )));
        }
        let mut cmd = Command::new("/system/bin/sh");
        cmd.arg(action_file.as_os_str());
        cmd.current_dir(self.module_dir(module_id));
        self.apply_module_env(module_id, action_id, &mut cmd);
        run_command_capture(cmd, self.cfg.action_timeout_sec).map_err(|_| {
            ModuleRuntimeRpcError::internal("ksu_dsapi_error=module_action_exec_failed")
        })
    }

    fn exec_main_cap(
        &self,
        module_id: &str,
        func_name: &str,
    ) -> Result<ExecOutput, ModuleRuntimeRpcError> {
        let cap_file = self
            .main_cap_file(module_id)
            .ok_or_else(|| ModuleRuntimeRpcError::invalid("ksu_dsapi_error=main_cap_missing"))?;
        let script = format!(
            ". {}; if command -v {} >/dev/null 2>&1; then {} ; else exit 127; fi",
            shell_quote(cap_file.to_string_lossy().as_ref()),
            func_name,
            func_name
        );
        let mut cmd = Command::new("/system/bin/sh");
        cmd.arg("-c").arg(script);
        cmd.current_dir(self.module_dir(module_id));
        self.apply_module_env(module_id, func_name, &mut cmd);
        run_command_capture(cmd, self.cfg.action_timeout_sec)
            .map_err(|_| ModuleRuntimeRpcError::internal("ksu_dsapi_error=module_cap_exec_failed"))
    }

    fn apply_module_env(&self, module_id: &str, action_id: &str, cmd: &mut Command) {
        let module_dir = self.module_dir(module_id);
        let module_state_dir = self.module_state_dir(module_id);
        let _ = fs::create_dir_all(&module_state_dir);

        let base_dir = self
            .cfg
            .module_root_dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("/data/adb/dsapi"));
        let run_dir = base_dir.join("run");
        let log_dir = base_dir.join("log");
        let state_dir = base_dir.join("state");
        let runtime_dir = base_dir.join("runtime");
        let releases_dir = runtime_dir.join("releases");
        let active_release_dir = runtime_dir.join("current");
        let daemon_socket = run_dir.join("dsapi.sock");
        let daemon_pid_file = run_dir.join("dsapid.pid");
        let agent_run_dir = run_dir.join("agents");
        let active_dsapid = active_release_dir.join("bin").join("dsapid");
        let active_dsapictl = active_release_dir.join("bin").join("dsapictl");
        let active_adapter_dex = active_release_dir
            .join("android")
            .join("directscreen-adapter-dex.jar");
        let modroot = std::env::var("DSAPI_MODROOT")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/data/adb/modules/directscreenapi"));

        cmd.env("DSAPI_MODROOT", modroot.as_os_str());
        cmd.env("DSAPI_BASE_DIR", base_dir.as_os_str());
        cmd.env("DSAPI_RUN_DIR", run_dir.as_os_str());
        cmd.env("DSAPI_LOG_DIR", log_dir.as_os_str());
        cmd.env("DSAPI_STATE_DIR", state_dir.as_os_str());
        cmd.env("DSAPI_RUNTIME_DIR", runtime_dir.as_os_str());
        cmd.env("DSAPI_RELEASES_DIR", releases_dir.as_os_str());
        cmd.env("DSAPI_ACTIVE_RELEASE", active_release_dir.as_os_str());
        cmd.env("DSAPI_ACTIVE_DSAPID", active_dsapid.as_os_str());
        cmd.env("DSAPI_ACTIVE_DSAPICTL", active_dsapictl.as_os_str());
        cmd.env("DSAPI_DAEMON_SOCKET", daemon_socket.as_os_str());
        cmd.env("DSAPI_DAEMON_PID_FILE", daemon_pid_file.as_os_str());
        cmd.env("DSAPI_AGENT_RUN_DIR", agent_run_dir.as_os_str());
        cmd.env("DSAPI_ACTIVE_ADAPTER_DEX", active_adapter_dex.as_os_str());
        cmd.env("DSAPI_MODULES_DIR", self.cfg.module_root_dir.as_os_str());
        cmd.env(
            "DSAPI_MODULE_DISABLED_DIR",
            self.cfg.module_disabled_dir.as_os_str(),
        );
        cmd.env(
            "DSAPI_MODULE_STATE_ROOT",
            self.cfg.module_state_root_dir.as_os_str(),
        );
        cmd.env("DSAPI_MODULE_ID", module_id);
        cmd.env("DSAPI_MODULE_DIR", module_dir.as_os_str());
        cmd.env("DSAPI_MODULE_STATE_DIR", module_state_dir.as_os_str());
        cmd.env("DSAPI_MODULE_ACTION_ID", action_id);
        cmd.env(
            "DSAPI_MODULE_ACTION_TIMEOUT_SEC",
            self.cfg.action_timeout_sec.to_string(),
        );
        if let Some(app_process_bin) = find_app_process_bin() {
            cmd.env("DSAPI_APP_PROCESS_BIN", app_process_bin);
        }

        let env_values = read_env_values_map(&module_dir.join("env.values")).unwrap_or_default();
        for (k, v) in env_values {
            if env_key_valid(&k) {
                cmd.env(k, v);
            }
        }
    }

    fn main_cap_file(&self, module_id: &str) -> Option<PathBuf> {
        let module_dir = self.module_dir(module_id);
        let meta_file = module_dir.join("dsapi.module");
        let configured = module_meta_with_alias(&meta_file, "MAIN_CAP_ID", "DSAPI_MAIN_CAP_ID", "");
        let cap_dir = module_dir.join("capabilities");
        let entries = script_files(&cap_dir).unwrap_or_default();
        if entries.is_empty() {
            return None;
        }
        if configured.is_empty() {
            return entries.first().cloned();
        }
        for file in entries {
            if cap_id_of_file(&file) == configured {
                return Some(file);
            }
        }
        None
    }

    fn ensure_layout(&self) -> std::io::Result<()> {
        fs::create_dir_all(&self.cfg.module_root_dir)?;
        fs::create_dir_all(&self.cfg.module_state_root_dir)?;
        fs::create_dir_all(&self.cfg.module_disabled_dir)?;
        if let Some(parent) = self.cfg.registry_file.parent() {
            fs::create_dir_all(parent)?;
        }
        if let Some(parent) = self.cfg.scope_file.parent() {
            fs::create_dir_all(parent)?;
        }
        Ok(())
    }

    fn ensure_module_exists(&self, module_id: &str) -> Result<(), ModuleRuntimeRpcError> {
        self.ensure_module_exists_ro(module_id)
    }

    fn ensure_module_exists_ro(&self, module_id: &str) -> Result<(), ModuleRuntimeRpcError> {
        if !module_id_valid(module_id) {
            return Err(ModuleRuntimeRpcError::invalid(
                "ksu_dsapi_error=module_id_invalid",
            ));
        }
        if !self.module_dir(module_id).is_dir() {
            return Err(ModuleRuntimeRpcError::invalid(
                "ksu_dsapi_error=module_not_found",
            ));
        }
        Ok(())
    }

    fn module_dirs_on_disk(&self) -> Vec<(String, PathBuf)> {
        let mut out: Vec<(String, PathBuf)> = Vec::new();
        let rd = match fs::read_dir(&self.cfg.module_root_dir) {
            Ok(v) => v,
            Err(_) => return out,
        };
        for ent in rd.flatten() {
            let path = ent.path();
            if !path.is_dir() {
                continue;
            }
            let id = ent.file_name().to_string_lossy().to_string();
            if !module_id_valid(&id) {
                continue;
            }
            out.push((id, path));
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    fn module_dir(&self, module_id: &str) -> PathBuf {
        self.cfg.module_root_dir.join(module_id)
    }

    fn module_state_dir(&self, module_id: &str) -> PathBuf {
        self.cfg.module_state_root_dir.join(module_id)
    }

    fn action_file(&self, module_id: &str, action_id: &str) -> PathBuf {
        self.module_dir(module_id)
            .join("actions")
            .join(format!("{}.sh", action_id))
    }

    fn is_module_disabled(&self, module_id: &str) -> bool {
        self.cfg
            .module_disabled_dir
            .join(format!("{}.disabled", module_id))
            .exists()
    }

    fn persist_registry_best_effort(&self) {
        let _ = self.persist_registry_file();
    }

    fn persist_scope_best_effort(&self) {
        let _ = self.persist_scope_file();
    }

    fn persist_registry_file(&self) -> std::io::Result<()> {
        let mut lines: Vec<String> = Vec::new();
        for rec in self.registry.list() {
            let (err_scope, err_code, err_message, err_detail, err_ts) = match rec.last_error {
                Some(v) => (
                    escape_field(&v.scope),
                    escape_field(&v.code),
                    escape_field(&v.message),
                    escape_field(&v.detail),
                    v.ts_ms.to_string(),
                ),
                None => (
                    "-".to_string(),
                    "-".to_string(),
                    "-".to_string(),
                    "-".to_string(),
                    "0".to_string(),
                ),
            };
            lines.push(format!(
                "module\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                escape_field(&rec.id),
                escape_field(&rec.name),
                escape_field(&rec.kind),
                escape_field(&rec.version),
                if rec.enabled { 1 } else { 0 },
                rec.state.as_str(),
                escape_field(&rec.reason),
                escape_field(&rec.main_cap),
                rec.action_count,
                if rec.auto_start { 1 } else { 0 },
                rec.installed_at_ms,
                rec.updated_at_ms,
                err_scope,
                err_code,
                err_message,
                err_detail,
                err_ts
            ));
        }

        if let Some(last) = self.registry.last_error() {
            lines.push(format!(
                "last_error\t{}\t{}\t{}\t{}\t{}",
                escape_field(&last.scope),
                escape_field(&last.code),
                escape_field(&last.message),
                escape_field(&last.detail),
                last.ts_ms
            ));
        }

        write_atomic_text(&self.cfg.registry_file, &(lines.join("\n") + "\n"))
    }

    fn load_registry_file(&mut self) -> std::io::Result<()> {
        let text = match fs::read_to_string(&self.cfg.registry_file) {
            Ok(v) => v,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e),
        };

        for line in text.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.is_empty() {
                continue;
            }
            if parts[0] == "module" && parts.len() >= 18 {
                let mut rec = ModuleRecord::new(unescape_field(parts[1]));
                rec.name = unescape_field(parts[2]);
                rec.kind = unescape_field(parts[3]);
                rec.version = unescape_field(parts[4]);
                rec.enabled = parts[5] == "1";
                rec.state = parse_module_state(parts[6]).unwrap_or(ModuleState::Installed);
                rec.reason = unescape_field(parts[7]);
                rec.main_cap = unescape_field(parts[8]);
                rec.action_count = parts[9].parse::<u32>().unwrap_or(0);
                rec.auto_start = parts[10] == "1";
                rec.installed_at_ms = parts[11].parse::<u64>().unwrap_or(0);
                rec.updated_at_ms = parts[12].parse::<u64>().unwrap_or(0);
                if parts[13] != "-" {
                    rec.last_error = Some(ModuleErrorRecord {
                        scope: unescape_field(parts[13]),
                        code: unescape_field(parts[14]),
                        message: unescape_field(parts[15]),
                        detail: unescape_field(parts[16]),
                        ts_ms: parts[17].parse::<u64>().unwrap_or(0),
                    });
                }
                let _ = self.registry.upsert(rec);
            } else if parts[0] == "last_error" && parts.len() >= 6 {
                self.registry.set_last_error(ModuleErrorRecord {
                    scope: unescape_field(parts[1]),
                    code: unescape_field(parts[2]),
                    message: unescape_field(parts[3]),
                    detail: unescape_field(parts[4]),
                    ts_ms: parts[5].parse::<u64>().unwrap_or(0),
                });
            }
        }
        Ok(())
    }

    fn persist_scope_file(&self) -> std::io::Result<()> {
        let mut lines: Vec<String> = Vec::new();
        for (module_id, rules) in &self.scope_rules {
            for rule in rules {
                lines.push(format!(
                    "scope\t{}\t{}\t{}\t{}",
                    escape_field(module_id),
                    escape_field(&rule.package_name),
                    rule.user_id,
                    if rule.allow { 1 } else { 0 }
                ));
            }
        }
        write_atomic_text(&self.cfg.scope_file, &(lines.join("\n") + "\n"))
    }

    fn load_scope_file(&mut self) -> std::io::Result<()> {
        let text = match fs::read_to_string(&self.cfg.scope_file) {
            Ok(v) => v,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e),
        };
        self.scope_rules.clear();
        for line in text.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() != 5 || parts[0] != "scope" {
                continue;
            }
            let module_id = unescape_field(parts[1]);
            if !module_id_valid(&module_id) {
                continue;
            }
            let package_name = unescape_field(parts[2]);
            let user_id = parts[3].parse::<i32>().unwrap_or(-1);
            let allow = parts[4] == "1";
            self.scope_rules
                .entry(module_id)
                .or_default()
                .push(ModuleScopeRule {
                    package_name,
                    user_id,
                    allow,
                });
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct ModuleStatusKv {
    state: Option<ModuleState>,
    reason: Option<String>,
}

fn parse_status_kv(raw: &str) -> ModuleStatusKv {
    let mut state: Option<ModuleState> = None;
    let mut reason: Option<String> = None;
    for line in raw.lines() {
        for token in line.split_whitespace() {
            if let Some(rest) = token.strip_prefix("state=") {
                state = parse_module_state(rest);
            } else if let Some(rest) = token.strip_prefix("reason=") {
                reason = Some(rest.to_string());
            }
        }
    }
    ModuleStatusKv { state, reason }
}

fn parse_scope_context(tokens: &[&str], start: usize, fallback_user: i32) -> ScopeContext {
    let package_name = tokens
        .get(start)
        .map(|v| v.to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "*".to_string());
    let user_id = tokens
        .get(start + 1)
        .and_then(|v| v.parse::<i32>().ok())
        .unwrap_or(fallback_user);
    ScopeContext {
        package_name,
        user_id,
    }
}

fn parse_scope_allow(raw: &str) -> Option<bool> {
    if raw.eq_ignore_ascii_case("allow") || raw == "1" || raw.eq_ignore_ascii_case("true") {
        return Some(true);
    }
    if raw.eq_ignore_ascii_case("deny") || raw == "0" || raw.eq_ignore_ascii_case("false") {
        return Some(false);
    }
    None
}

fn scope_rule_line(module_id: &str, rule: &ModuleScopeRule) -> String {
    format!(
        "module_scope_row={}|{}|{}|{}",
        module_id,
        sanitize_token(&rule.package_name),
        rule.user_id,
        if rule.allow { "allow" } else { "deny" }
    )
}

fn module_last_pid(module_id: &str, module_state_root_dir: &Path) -> String {
    let pid_file = module_state_root_dir.join(module_id).join("pid");
    match fs::read_to_string(pid_file) {
        Ok(v) => {
            let trimmed = v.trim();
            if trimmed.is_empty() {
                "-".to_string()
            } else {
                sanitize_token(trimmed)
            }
        }
        Err(_) => "-".to_string(),
    }
}

fn shell_quote(raw: &str) -> String {
    let escaped = raw.replace('"', "\\\"");
    format!("\"{}\"", escaped)
}

fn run_command_capture(mut cmd: Command, timeout_sec: u64) -> std::io::Result<ExecOutput> {
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    let mut child = cmd.spawn()?;

    let stdout_handle = child.stdout.take().map(|mut s| {
        thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = s.read_to_end(&mut buf);
            buf
        })
    });
    let stderr_handle = child.stderr.take().map(|mut s| {
        thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = s.read_to_end(&mut buf);
            buf
        })
    });

    let deadline = Instant::now() + Duration::from_secs(timeout_sec.max(1));
    let mut timed_out = false;

    let code = loop {
        match child.try_wait()? {
            Some(status) => {
                break status.code().unwrap_or(0);
            }
            None => {
                if Instant::now() >= deadline {
                    timed_out = true;
                    let _ = child.kill();
                    let _ = child.wait();
                    break 124;
                }
                thread::sleep(Duration::from_millis(20));
            }
        }
    };

    let stdout = stdout_handle
        .and_then(|h| h.join().ok())
        .map(|v| String::from_utf8_lossy(&v).to_string())
        .unwrap_or_default();
    let stderr = stderr_handle
        .and_then(|h| h.join().ok())
        .map(|v| String::from_utf8_lossy(&v).to_string())
        .unwrap_or_default();

    Ok(ExecOutput {
        code,
        stdout,
        stderr,
        timed_out,
    })
}

#[derive(Debug, Clone)]
struct ActionEntry {
    id: String,
    name: String,
    danger: bool,
}

#[derive(Debug, Clone)]
struct EnvSpecItem {
    key: String,
    value: String,
    default_value: String,
    value_type: String,
    label: String,
    desc: String,
}

fn module_env_lines(module_id: &str, module_dir: &Path) -> Vec<EnvSpecItem> {
    let spec = module_dir.join("env.spec");
    let values = read_env_values_map(&module_dir.join("env.values")).unwrap_or_default();
    let text = match fs::read_to_string(spec) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = trimmed.split('|').collect();
        if parts.is_empty() {
            continue;
        }
        let key = parts.first().copied().unwrap_or("").trim().to_string();
        if !env_key_valid(&key) {
            continue;
        }
        let default_value = parts.get(1).copied().unwrap_or("").trim().to_string();
        let value_type = parts.get(2).copied().unwrap_or("text").trim().to_string();
        let label = parts.get(3).copied().unwrap_or(&key).trim().to_string();
        let desc = parts.get(4).copied().unwrap_or("-").trim().to_string();
        let value = values
            .get(&key)
            .cloned()
            .unwrap_or_else(|| default_value.clone());
        out.push(EnvSpecItem {
            key,
            value,
            default_value,
            value_type,
            label,
            desc,
        });
    }

    if out.is_empty() {
        let _ = module_id;
    }
    out
}

fn read_env_values_map(path: &Path) -> std::io::Result<BTreeMap<String, String>> {
    let text = match fs::read_to_string(path) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(e) => return Err(e),
    };
    let mut out = BTreeMap::new();
    for line in text.lines() {
        if let Some((k, v)) = line.split_once('=') {
            if env_key_valid(k.trim()) {
                out.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
    }
    Ok(out)
}

fn write_env_values_map(path: &Path, map: &BTreeMap<String, String>) -> std::io::Result<()> {
    let mut text = String::new();
    for (k, v) in map {
        text.push_str(k);
        text.push('=');
        text.push_str(v);
        text.push('\n');
    }
    write_atomic_text(path, &text)
}

fn action_entries(dir: &Path) -> std::io::Result<Vec<ActionEntry>> {
    let files = script_files(dir)?;
    let mut out = Vec::new();
    for file in files {
        let id = file
            .file_stem()
            .and_then(|v| v.to_str())
            .unwrap_or("")
            .to_string();
        if !module_id_valid(&id) {
            continue;
        }
        let text = fs::read_to_string(&file).unwrap_or_default();
        let mut name = id.clone();
        let mut danger = false;
        for line in text.lines() {
            let trimmed = line.trim();
            if let Some(v) = trimmed.strip_prefix("ACTION_NAME=") {
                name = trim_shell_value(v);
            } else if let Some(v) = trimmed.strip_prefix("ACTION_DANGER=") {
                danger = parse_bool_like(v);
            }
        }
        out.push(ActionEntry { id, name, danger });
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

fn script_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut out: Vec<PathBuf> = Vec::new();
    let rd = match fs::read_dir(dir) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(e),
    };
    for ent in rd.flatten() {
        let path = ent.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|v| v.to_str()) != Some("sh") {
            continue;
        }
        out.push(path);
    }
    out.sort();
    Ok(out)
}

fn module_meta_with_alias(meta_file: &Path, key: &str, alias: &str, default_val: &str) -> String {
    if let Some(v) = module_meta_get(meta_file, key) {
        return v;
    }
    if let Some(v) = module_meta_get(meta_file, alias) {
        return v;
    }
    default_val.to_string()
}

fn module_meta_get(meta_file: &Path, key: &str) -> Option<String> {
    let text = fs::read_to_string(meta_file).ok()?;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(&(key.to_string() + "=")) {
            return Some(trim_shell_value(rest));
        }
    }
    None
}

fn cap_id_of_file(path: &Path) -> String {
    let text = fs::read_to_string(path).unwrap_or_default();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(v) = trimmed.strip_prefix("CAP_ID=") {
            let id = trim_shell_value(v);
            if !id.is_empty() {
                return id;
            }
        }
    }
    path.file_stem()
        .and_then(|v| v.to_str())
        .unwrap_or("")
        .to_string()
}

fn parse_bool_like(raw: &str) -> bool {
    let v = raw.trim().trim_matches('"').trim_matches('\'');
    matches!(
        v,
        "1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON" | "allow"
    )
}

fn parse_module_state(raw: &str) -> Option<ModuleState> {
    let norm = raw.trim().to_ascii_lowercase();
    match norm.as_str() {
        "installed" => Some(ModuleState::Installed),
        "disabled" => Some(ModuleState::Disabled),
        "running" => Some(ModuleState::Running),
        "ready" => Some(ModuleState::Ready),
        "degraded" => Some(ModuleState::Degraded),
        "stopped" => Some(ModuleState::Stopped),
        "error" => Some(ModuleState::Error),
        "removed" => Some(ModuleState::Removed),
        _ => None,
    }
}

fn module_id_valid(raw: &str) -> bool {
    if raw.is_empty() {
        return false;
    }
    raw.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
}

fn scope_package_valid(raw: &str) -> bool {
    if raw == "*" {
        return true;
    }
    if raw.is_empty() {
        return false;
    }
    raw.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
}

fn env_key_valid(raw: &str) -> bool {
    let mut chars = raw.chars();
    match chars.next() {
        Some(c) if c.is_ascii_uppercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

fn count_scripts(dir: &Path) -> std::io::Result<usize> {
    Ok(script_files(dir)?.len())
}

fn find_app_process_bin() -> Option<String> {
    for path in [
        "/system/bin/app_process64",
        "/system/bin/app_process",
        "/system/bin/app_process32",
    ] {
        if Path::new(path).is_file() {
            return Some(path.to_string());
        }
    }
    std::env::var("DSAPI_APP_PROCESS_BIN")
        .ok()
        .filter(|v| !v.trim().is_empty())
}

fn trim_shell_value(raw: &str) -> String {
    raw.trim().trim_matches('"').trim_matches('\'').to_string()
}

fn sanitize_token(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn sanitize_pipe_field(raw: &str) -> String {
    raw.chars()
        .map(|c| match c {
            '\r' | '\n' | '\t' => ' ',
            '|' => '¦',
            _ => c,
        })
        .collect::<String>()
        .trim()
        .to_string()
}

fn escape_field(raw: &str) -> String {
    raw.replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
}

fn unescape_field(raw: &str) -> String {
    let mut out = String::new();
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn write_atomic_text(path: &Path, content: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(content.as_bytes())?;
        f.sync_all()?;
    }
    fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_scope_allow_values() {
        assert_eq!(parse_scope_allow("allow"), Some(true));
        assert_eq!(parse_scope_allow("deny"), Some(false));
        assert_eq!(parse_scope_allow("1"), Some(true));
        assert_eq!(parse_scope_allow("0"), Some(false));
        assert_eq!(parse_scope_allow("x"), None);
    }

    #[test]
    fn module_state_parse_roundtrip() {
        assert_eq!(parse_module_state("ready"), Some(ModuleState::Ready));
        assert_eq!(parse_module_state("running"), Some(ModuleState::Running));
        assert_eq!(parse_module_state("none"), None);
    }

    #[test]
    fn env_key_validation() {
        assert!(env_key_valid("ABC"));
        assert!(env_key_valid("A1_B"));
        assert!(!env_key_valid("a1"));
        assert!(!env_key_valid("1A"));
    }
}
