package org.directscreenapi.manager;

import java.util.ArrayList;
import java.util.List;

final class ManagerRepository {
    private final DsapiCtlClient ctl;

    ManagerRepository(ManagerConfig config) {
        this.ctl = new DsapiCtlClient(config);
    }

    DsapiCtlClient.CmdResult run(String... args) {
        return ctl.run(args);
    }

    HomeSnapshot loadHomeSnapshot() {
        DsapiCtlClient.CmdResult status = ctl.run("status");
        List<String> lines = DsapiCtlClient.splitLines(status.output);
        HomeSnapshot snapshot = new HomeSnapshot();
        snapshot.status = status;
        snapshot.coreState = tokenAfterPrefix(lines, "ksu_dsapi_status=", "unknown");
        snapshot.bridgeState = tokenByKey(lines, "ksu_dsapi_bridge ", "state", "unknown");
        snapshot.zygoteState = tokenByKey(lines, "ksu_dsapi_zygote ", "state", "unknown");
        snapshot.runtimeActive = tokenAfterPrefix(lines, "runtime_active=", "<none>");
        snapshot.moduleCount = tokenAfterPrefix(lines, "module_count=", "0");
        snapshot.enabled = tokenAfterPrefix(lines, "enabled=", "?");
        snapshot.daemonModuleCount = tokenAfterPrefix(lines, "daemon_module_registry_count=", "0");
        snapshot.daemonModuleEventSeq = tokenAfterPrefix(lines, "daemon_module_registry_event_seq=", "0");
        snapshot.daemonModuleError = tokenAfterPrefix(lines, "daemon_module_registry_error=", "0");
        snapshot.zygoteScopeCount = tokenAfterPrefix(lines, "zygote_scope_count=", "0");
        snapshot.lastErrorState = tokenByKey(lines, "ksu_dsapi_last_error ", "state", "none");
        snapshot.lastErrorCode = tokenByKey(lines, "ksu_dsapi_last_error ", "code", "-");
        snapshot.lastErrorMessage = tokenByKey(lines, "ksu_dsapi_last_error ", "message", "-");
        return snapshot;
    }

    ModulesSnapshot loadModulesSnapshot(boolean includeActions) {
        ModulesSnapshot snapshot = new ModulesSnapshot();
        snapshot.home = loadHomeSnapshot();
        snapshot.moduleList = ctl.run("module", "list");
        snapshot.modules = CtlParsers.parseModuleRows(snapshot.moduleList.output);
        if (includeActions && snapshot.moduleList.exitCode == 0) {
            for (CtlParsers.ModuleRow module : snapshot.modules) {
                DsapiCtlClient.CmdResult actionRes = ctl.run("module", "action-list", module.id);
                if (actionRes.exitCode == 0) {
                    module.actions = CtlParsers.parseModuleActionRows(actionRes.output);
                } else {
                    module.actions = new ArrayList<CtlParsers.ModuleActionRow>();
                }
            }
        }
        return snapshot;
    }

    SettingsSnapshot loadSettingsSnapshot(String selectedModuleId) {
        SettingsSnapshot snapshot = new SettingsSnapshot();
        snapshot.moduleList = ctl.run("module", "list");
        snapshot.modules = CtlParsers.parseModuleRows(snapshot.moduleList.output);

        String targetModule = selectedModuleId == null ? "" : selectedModuleId;
        if (targetModule.isEmpty()) {
            if (!snapshot.modules.isEmpty()) {
                targetModule = snapshot.modules.get(0).id;
            }
        } else if (!containsModule(snapshot.modules, targetModule)) {
            targetModule = snapshot.modules.isEmpty() ? "" : snapshot.modules.get(0).id;
        }
        snapshot.selectedModuleId = targetModule;

        if (!targetModule.isEmpty()) {
            snapshot.envList = ctl.run("module", "env-list", targetModule);
            snapshot.envRows = CtlParsers.parseModuleEnvRows(snapshot.envList.output);
        } else {
            snapshot.envList = new DsapiCtlClient.CmdResult(0, "");
            snapshot.envRows = new ArrayList<CtlParsers.ModuleEnvRow>();
        }
        return snapshot;
    }

    LogsSnapshot loadLogsSnapshot() {
        LogsSnapshot snapshot = new LogsSnapshot();
        snapshot.home = loadHomeSnapshot();
        snapshot.lastError = ctl.run("errors", "last");
        snapshot.moduleList = ctl.run("module", "list");
        return snapshot;
    }

    private static boolean containsModule(List<CtlParsers.ModuleRow> rows, String id) {
        for (CtlParsers.ModuleRow row : rows) {
            if (row.id.equals(id)) {
                return true;
            }
        }
        return false;
    }

    private static String tokenAfterPrefix(List<String> lines, String prefix, String fallback) {
        String line = DsapiCtlClient.findLineByPrefix(lines, prefix);
        if (line == null) {
            return fallback;
        }
        String value = DsapiCtlClient.parseAfterPrefixToken(line, prefix);
        return value == null || value.isEmpty() ? fallback : value;
    }

    private static String tokenByKey(List<String> lines, String prefix, String key, String fallback) {
        String line = DsapiCtlClient.findLineByPrefix(lines, prefix);
        if (line == null) {
            return fallback;
        }
        String value = DsapiCtlClient.kvGet(line, key);
        return value == null || value.isEmpty() ? fallback : value;
    }

    static final class HomeSnapshot {
        DsapiCtlClient.CmdResult status;
        String coreState = "unknown";
        String bridgeState = "unknown";
        String zygoteState = "unknown";
        String runtimeActive = "<none>";
        String moduleCount = "0";
        String enabled = "?";
        String daemonModuleCount = "0";
        String daemonModuleEventSeq = "0";
        String daemonModuleError = "0";
        String zygoteScopeCount = "0";
        String lastErrorState = "none";
        String lastErrorCode = "-";
        String lastErrorMessage = "-";
    }

    static final class ModulesSnapshot {
        HomeSnapshot home;
        DsapiCtlClient.CmdResult moduleList;
        List<CtlParsers.ModuleRow> modules = new ArrayList<CtlParsers.ModuleRow>();
    }

    static final class SettingsSnapshot {
        DsapiCtlClient.CmdResult moduleList;
        DsapiCtlClient.CmdResult envList;
        List<CtlParsers.ModuleRow> modules = new ArrayList<CtlParsers.ModuleRow>();
        List<CtlParsers.ModuleEnvRow> envRows = new ArrayList<CtlParsers.ModuleEnvRow>();
        String selectedModuleId = "";
    }

    static final class LogsSnapshot {
        HomeSnapshot home;
        DsapiCtlClient.CmdResult lastError;
        DsapiCtlClient.CmdResult moduleList;
    }
}
