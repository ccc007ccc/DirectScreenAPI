package org.directscreenapi.manager;

import java.util.ArrayList;
import java.util.List;

final class CtlParsers {
    private CtlParsers() {
    }

    static List<ModuleRow> parseModuleRows(String output) {
        List<ModuleRow> out = new ArrayList<ModuleRow>();
        if (output == null || output.isEmpty()) {
            return out;
        }
        String[] lines = output.split("\\r?\\n");
        for (String line : lines) {
            if (!line.startsWith("module_row=")) {
                continue;
            }
            String[] parts = line.substring("module_row=".length()).split("\\|", -1);
            if (parts.length < 9) {
                continue;
            }
            ModuleRow row = new ModuleRow();
            row.id = parts[0];
            row.name = parts[1];
            row.kind = parts[2];
            row.version = parts[3];
            row.state = parts[4];
            row.enabled = parts[5];
            row.mainCap = parts[6];
            row.actionCount = parts[7];
            row.reason = parts[8];
            out.add(row);
        }
        return out;
    }

    static List<ModuleActionRow> parseModuleActionRows(String output) {
        List<ModuleActionRow> out = new ArrayList<ModuleActionRow>();
        if (output == null || output.isEmpty()) {
            return out;
        }
        String[] lines = output.split("\\r?\\n");
        for (String line : lines) {
            if (!line.startsWith("module_action_row=")) {
                continue;
            }
            String[] parts = line.substring("module_action_row=".length()).split("\\|", -1);
            if (parts.length < 3) {
                continue;
            }
            ModuleActionRow row = new ModuleActionRow();
            row.id = parts[0];
            row.name = parts[1];
            row.danger = parts[2];
            out.add(row);
        }
        return out;
    }

    static List<ModuleEnvRow> parseModuleEnvRows(String output) {
        List<ModuleEnvRow> out = new ArrayList<ModuleEnvRow>();
        if (output == null || output.isEmpty()) {
            return out;
        }
        String[] lines = output.split("\\r?\\n");
        for (String line : lines) {
            if (!line.startsWith("module_env_row=")) {
                continue;
            }
            String[] parts = line.substring("module_env_row=".length()).split("\\|", -1);
            if (parts.length < 6) {
                continue;
            }
            ModuleEnvRow row = new ModuleEnvRow();
            row.key = parts[0];
            row.value = parts[1];
            row.defaultValue = parts[2];
            row.type = parts[3];
            row.label = parts[4];
            row.description = parts[5];
            out.add(row);
        }
        return out;
    }

    static List<ModuleZipRow> parseModuleZipRows(String output) {
        List<ModuleZipRow> out = new ArrayList<ModuleZipRow>();
        if (output == null || output.isEmpty()) {
            return out;
        }
        String[] lines = output.split("\\r?\\n");
        for (String line : lines) {
            if (!line.startsWith("module_zip_row=")) {
                continue;
            }
            String[] parts = line.substring("module_zip_row=".length()).split("\\|", -1);
            if (parts.length < 2) {
                continue;
            }
            ModuleZipRow row = new ModuleZipRow();
            row.name = parts[0];
            row.path = parts[1];
            out.add(row);
        }
        return out;
    }

    static final class ModuleRow {
        String id = "";
        String name = "";
        String kind = "";
        String version = "";
        String state = "";
        String enabled = "";
        String mainCap = "";
        String actionCount = "";
        String reason = "";
        List<ModuleActionRow> actions = new ArrayList<ModuleActionRow>();
    }

    static final class ModuleActionRow {
        String id = "";
        String name = "";
        String danger = "0";

        boolean isDangerous() {
            return "1".equals(danger);
        }
    }

    static final class ModuleEnvRow {
        String key = "";
        String value = "";
        String defaultValue = "";
        String type = "";
        String label = "";
        String description = "";
    }

    static final class ModuleZipRow {
        String name = "";
        String path = "";
    }
}
