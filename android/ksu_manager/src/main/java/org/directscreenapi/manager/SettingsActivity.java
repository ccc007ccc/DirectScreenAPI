package org.directscreenapi.manager;

import android.app.Activity;
import android.app.AlertDialog;
import android.content.Intent;
import android.os.Bundle;
import android.os.SystemClock;
import android.text.InputType;
import android.view.View;
import android.widget.ArrayAdapter;
import android.widget.Button;
import android.widget.EditText;
import android.widget.HorizontalScrollView;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.Spinner;
import android.widget.TextView;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

public final class SettingsActivity extends Activity {
    private static final long START_BACK_GUARD_MS = 1500L;
    private final ExecutorService worker = Executors.newSingleThreadExecutor();

    private ManagerConfig config;
    private ManagerRepository repo;

    private TextView subtitleView;
    private EditText ctlPathEdit;
    private EditText bridgeServiceEdit;
    private EditText refreshMsEdit;

    private Spinner moduleSpinner;
    private LinearLayout envContainer;
    private TextView envHintView;
    private TextView logView;
    private String fullLogText = "...";

    private final List<CtlParsers.ModuleRow> modules = new ArrayList<CtlParsers.ModuleRow>();
    private final List<CtlParsers.ModuleEnvRow> envRows = new ArrayList<CtlParsers.ModuleEnvRow>();
    private final Map<String, EditText> envInputs = new HashMap<String, EditText>();

    private String selectedModuleId = "";
    private boolean spinnerUpdating;
    private long createdUptimeMs;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        createdUptimeMs = SystemClock.uptimeMillis();
        config = ManagerConfig.load(this, getIntent());
        repo = new ManagerRepository(config);

        setTitle("DSAPI Manager · 设置");
        setContentView(buildContentView());

        bindConfigToInputs();
        refreshModulesAndEnv();
    }

    @Override
    public void onBackPressed() {
        if (SystemClock.uptimeMillis() - createdUptimeMs < START_BACK_GUARD_MS) {
            return;
        }
        super.onBackPressed();
    }

    @Override
    protected void onResume() {
        super.onResume();
        config = ManagerConfig.load(this, getIntent());
        repo = new ManagerRepository(config);
        bindConfigToInputs();
        refreshModulesAndEnv();
    }

    @Override
    protected void onDestroy() {
        try {
            worker.shutdownNow();
        } catch (Throwable ignored) {
        }
        super.onDestroy();
    }

    private View buildContentView() {
        LinearLayout shell = new LinearLayout(this);
        shell.setOrientation(LinearLayout.VERTICAL);
        shell.setBackground(UiStyles.makeAppBackground());

        shell.addView(buildHeaderCard(), new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT
        ));

        ScrollView page = new ScrollView(this);
        page.setFillViewport(true);
        LinearLayout root = UiStyles.buildPageRoot(this);
        root.setPadding(UiStyles.dp(this, 12), UiStyles.dp(this, 8), UiStyles.dp(this, 12), UiStyles.dp(this, 8));
        page.addView(root, new ScrollView.LayoutParams(
                ScrollView.LayoutParams.MATCH_PARENT,
                ScrollView.LayoutParams.WRAP_CONTENT
        ));

        root.addView(buildConfigCard());
        root.addView(buildModuleEnvCard(), UiStyles.cardLayout(this));
        root.addView(buildLogCard(), UiStyles.cardLayout(this));

        LinearLayout.LayoutParams pageLp = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                0,
                1f
        );
        shell.addView(page, pageLp);
        shell.addView(buildBottomNav());
        return shell;
    }

    private View buildHeaderCard() {
        UiStyles.HeaderBar bar = UiStyles.makeHeaderBar(this, "Manager Settings", "管理器配置与模块环境变量");
        subtitleView = bar.subtitle;
        return bar.root;
    }

    private View buildBottomNav() {
        LinearLayout nav = new LinearLayout(this);
        nav.setOrientation(LinearLayout.HORIZONTAL);
        nav.setPadding(UiStyles.dp(this, 10), UiStyles.dp(this, 8), UiStyles.dp(this, 10), UiStyles.dp(this, 8));
        nav.setBackground(UiStyles.makeRoundedDrawable(this, UiStyles.C_SURFACE, UiStyles.C_OUTLINE, 0, 0));

        Button home = UiStyles.makeNavButton(this, "主页", false);
        home.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                Intent intent = new Intent(SettingsActivity.this, MainActivity.class);
                intent.addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP | Intent.FLAG_ACTIVITY_SINGLE_TOP);
                startActivity(config.applyToIntent(intent));
                finish();
                overridePendingTransition(0, 0);
            }
        });
        nav.addView(home, UiStyles.rowWeightLayout(this));

        Button modules = UiStyles.makeNavButton(this, "模块", false);
        modules.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                Intent intent = new Intent(SettingsActivity.this, ModulesActivity.class);
                intent.addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP | Intent.FLAG_ACTIVITY_SINGLE_TOP);
                startActivity(config.applyToIntent(intent));
                finish();
                overridePendingTransition(0, 0);
            }
        });
        nav.addView(modules, UiStyles.rowWeightLayout(this));

        Button logs = UiStyles.makeNavButton(this, "日志", false);
        logs.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                Intent intent = new Intent(SettingsActivity.this, LogsActivity.class);
                intent.addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP | Intent.FLAG_ACTIVITY_SINGLE_TOP);
                startActivity(config.applyToIntent(intent));
                finish();
                overridePendingTransition(0, 0);
            }
        });
        nav.addView(logs, UiStyles.rowWeightLayout(this));

        Button settings = UiStyles.makeNavButton(this, "设置", true);
        settings.setEnabled(false);
        nav.addView(settings, UiStyles.rowWeightLayout(this));
        return nav;
    }

    private View buildConfigCard() {
        LinearLayout card = UiStyles.makeCard(this);
        card.addView(UiStyles.makeSectionTitle(this, "管理器配置"));
        TextView desc = UiStyles.makeSectionDesc(this, "保存 ctl/binder bridge/刷新参数，供主页与模块页复用");
        card.addView(desc, UiStyles.topMargin(this, 2));

        ctlPathEdit = makeLabeledInput(card, "ctl 路径", "例如 /data/adb/modules/directscreenapi/bin/dsapi_service_ctl.sh", false);
        bridgeServiceEdit = makeLabeledInput(card, "binder service", ManagerConfig.DEFAULT_BRIDGE_SERVICE, false);
        refreshMsEdit = makeLabeledInput(card, "刷新间隔(ms)", "1000", true);

        LinearLayout row = new LinearLayout(this);
        row.setOrientation(LinearLayout.HORIZONTAL);

        Button save = UiStyles.makeFilledButton(this, "保存配置");
        save.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                saveConfigFromInputs();
            }
        });
        row.addView(save, UiStyles.rowWeightLayout(this));

        Button restartBridge = UiStyles.makeWarningButton(this, "重启桥接");
        restartBridge.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                showDangerConfirm("重启桥接", "会短暂中断管理器通信，是否继续？", new Runnable() {
                    @Override
                    public void run() {
                        runCtlAction(new String[]{"bridge", "restart"}, false);
                    }
                });
            }
        });
        row.addView(restartBridge, UiStyles.rowWeightLayout(this));

        Button refresh = UiStyles.makeTonalButton(this, "刷新模块配置");
        refresh.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                refreshModulesAndEnv();
            }
        });
        row.addView(refresh, UiStyles.rowWeightLayout(this));

        card.addView(row, UiStyles.topMargin(this, 10));
        return card;
    }

    private View buildModuleEnvCard() {
        LinearLayout card = UiStyles.makeCard(this);
        card.addView(UiStyles.makeSectionTitle(this, "模块环境变量"));
        TextView desc = UiStyles.makeSectionDesc(this, "读取 env.spec 并持久化到 env.values（测试模块可直接调参）");
        card.addView(desc, UiStyles.topMargin(this, 2));

        TextView moduleLabel = UiStyles.makeSectionDesc(this, "目标模块");
        card.addView(moduleLabel, UiStyles.topMargin(this, 10));

        moduleSpinner = new Spinner(this);
        moduleSpinner.setBackground(UiStyles.makeRoundedDrawable(this, UiStyles.C_SURFACE, UiStyles.C_OUTLINE, 10, 1));
        moduleSpinner.setOnItemSelectedListener(new android.widget.AdapterView.OnItemSelectedListener() {
            @Override
            public void onItemSelected(android.widget.AdapterView<?> parent, View view, int position, long id) {
                if (spinnerUpdating || position < 0 || position >= modules.size()) {
                    return;
                }
                selectedModuleId = modules.get(position).id;
                refreshEnvOnly();
            }

            @Override
            public void onNothingSelected(android.widget.AdapterView<?> parent) {
            }
        });
        card.addView(moduleSpinner, UiStyles.topMargin(this, 6));

        envHintView = UiStyles.makeSectionDesc(this, "加载中...");
        card.addView(envHintView, UiStyles.topMargin(this, 8));

        envContainer = new LinearLayout(this);
        envContainer.setOrientation(LinearLayout.VERTICAL);
        card.addView(envContainer, UiStyles.topMargin(this, 8));

        LinearLayout row = new LinearLayout(this);
        row.setOrientation(LinearLayout.HORIZONTAL);

        Button saveEnv = UiStyles.makeFilledButton(this, "保存环境变量");
        saveEnv.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                saveModuleEnv();
            }
        });
        row.addView(saveEnv, UiStyles.rowWeightLayout(this));

        Button resetEnv = UiStyles.makeWarningButton(this, "重置环境变量");
        resetEnv.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                showDangerConfirm("重置环境变量", "将清空当前模块所有自定义 env.values，是否继续？", new Runnable() {
                    @Override
                    public void run() {
                        resetModuleEnv();
                    }
                });
            }
        });
        row.addView(resetEnv, UiStyles.rowWeightLayout(this));

        Button reloadEnv = UiStyles.makeTonalButton(this, "重新读取");
        reloadEnv.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                refreshEnvOnly();
            }
        });
        row.addView(reloadEnv, UiStyles.rowWeightLayout(this));

        card.addView(row, UiStyles.topMargin(this, 10));
        return card;
    }

    private View buildLogCard() {
        LinearLayout card = UiStyles.makeCard(this);
        card.addView(UiStyles.makeSectionTitle(this, "命令日志"));
        TextView desc = UiStyles.makeSectionDesc(this, "展示最近一次设置写入/读取输出（可查看完整日志）");
        card.addView(desc, UiStyles.topMargin(this, 2));

        logView = UiStyles.makeLogText(this);
        logView.setText("...");

        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                UiStyles.dp(this, 110)
        );
        lp.topMargin = UiStyles.dp(this, 10);
        card.addView(logView, lp);

        Button full = UiStyles.makeTonalButton(this, "查看完整日志");
        full.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                showFullLogDialog();
            }
        });
        card.addView(full, UiStyles.gravityEndWrap(this, 8));

        return card;
    }

    private EditText makeLabeledInput(LinearLayout card, String label, String hint, boolean numberOnly) {
        TextView tv = UiStyles.makeSectionDesc(this, label);
        card.addView(tv, UiStyles.topMargin(this, 10));

        EditText et = new EditText(this);
        UiStyles.styleEditText(this, et);
        et.setHint(hint);
        if (numberOnly) {
            et.setInputType(InputType.TYPE_CLASS_NUMBER);
        }
        card.addView(et, UiStyles.topMargin(this, 6));
        return et;
    }

    private void bindConfigToInputs() {
        subtitleView.setText(
                "ctl=" + config.ctlPath
                        + "\nbridge_service=" + config.bridgeService
                        + "  refresh=" + config.refreshMs + "ms"
        );
        ctlPathEdit.setText(config.ctlPath);
        bridgeServiceEdit.setText(config.bridgeService);
        refreshMsEdit.setText(String.valueOf(config.refreshMs));
    }

    private void saveConfigFromInputs() {
        String ctlPath = textOf(ctlPathEdit, ManagerConfig.DEFAULT_CTL_PATH);
        String service = textOf(bridgeServiceEdit, ManagerConfig.DEFAULT_BRIDGE_SERVICE);

        int refreshMs = parseIntSafe(textOf(refreshMsEdit, String.valueOf(ManagerConfig.DEFAULT_REFRESH_MS)),
                ManagerConfig.DEFAULT_REFRESH_MS);
        if (refreshMs < 250) {
            refreshMs = 250;
        }

        config.ctlPath = ctlPath;
        config.bridgeService = service;
        config.refreshMs = refreshMs;
        config.save(this);
        repo = new ManagerRepository(config);
        bindConfigToInputs();

        setLogText("config_saved=1\nctl=" + config.ctlPath + "\nbridge_service=" + config.bridgeService + "\nrefresh_ms=" + config.refreshMs);
    }

    private String textOf(EditText et, String fallback) {
        if (et == null || et.getText() == null) {
            return fallback;
        }
        String value = et.getText().toString().trim();
        if (value.isEmpty()) {
            return fallback;
        }
        return value;
    }

    private int parseIntSafe(String raw, int fallback) {
        try {
            return Integer.parseInt(raw);
        } catch (Throwable ignored) {
            return fallback;
        }
    }

    private void refreshModulesAndEnv() {
        worker.execute(new Runnable() {
            @Override
            public void run() {
                final ManagerRepository.SettingsSnapshot snapshot = repo.loadSettingsSnapshot(selectedModuleId);

                runOnUiThread(new Runnable() {
                    @Override
                    public void run() {
                        modules.clear();
                        modules.addAll(snapshot.modules);
                        selectedModuleId = snapshot.selectedModuleId;

                        envRows.clear();
                        envRows.addAll(snapshot.envRows);

                        renderModuleSpinner();
                        renderEnvRows();

                        StringBuilder sb = new StringBuilder();
                        sb.append("$ module list\nexit=").append(snapshot.moduleList.exitCode);
                        if (!snapshot.moduleList.output.isEmpty()) {
                            sb.append("\n").append(snapshot.moduleList.output);
                        }
                        if (!selectedModuleId.isEmpty()) {
                            sb.append("\n\n$ module env-list ").append(selectedModuleId);
                            sb.append("\nexit=").append(snapshot.envList.exitCode);
                            if (!snapshot.envList.output.isEmpty()) {
                                sb.append("\n").append(snapshot.envList.output);
                            }
                        }
                        setLogText(sb.toString());
                    }
                });
            }
        });
    }

    private void refreshEnvOnly() {
        if (selectedModuleId == null || selectedModuleId.isEmpty()) {
            envRows.clear();
            renderEnvRows();
            return;
        }
        worker.execute(new Runnable() {
            @Override
            public void run() {
                final ManagerRepository.SettingsSnapshot snapshot = repo.loadSettingsSnapshot(selectedModuleId);
                runOnUiThread(new Runnable() {
                    @Override
                    public void run() {
                        selectedModuleId = snapshot.selectedModuleId;
                        envRows.clear();
                        envRows.addAll(snapshot.envRows);
                        renderEnvRows();

                        StringBuilder sb = new StringBuilder();
                        sb.append("$ module env-list ").append(snapshot.selectedModuleId)
                                .append("\nexit=").append(snapshot.envList.exitCode);
                        if (!snapshot.envList.output.isEmpty()) {
                            sb.append("\n").append(snapshot.envList.output);
                        }
                        setLogText(sb.toString());
                    }
                });
            }
        });
    }

    private void renderModuleSpinner() {
        List<String> labels = new ArrayList<String>();
        int selectedIndex = -1;

        for (int i = 0; i < modules.size(); i++) {
            CtlParsers.ModuleRow module = modules.get(i);
            labels.add(module.name + " (" + module.id + ")");
            if (module.id.equals(selectedModuleId)) {
                selectedIndex = i;
            }
        }

        spinnerUpdating = true;
        ArrayAdapter<String> adapter = new ArrayAdapter<String>(this, android.R.layout.simple_spinner_item, labels);
        adapter.setDropDownViewResource(android.R.layout.simple_spinner_dropdown_item);
        moduleSpinner.setAdapter(adapter);
        if (selectedIndex >= 0) {
            moduleSpinner.setSelection(selectedIndex);
        }
        spinnerUpdating = false;

        if (modules.isEmpty()) {
            envHintView.setText("暂无模块，可去“模块页”导入 ZIP 或安装内置模块。");
        } else {
            envHintView.setText("当前模块: " + selectedModuleId);
        }
    }

    private void renderEnvRows() {
        envContainer.removeAllViews();
        envInputs.clear();

        if (selectedModuleId == null || selectedModuleId.isEmpty()) {
            TextView empty = UiStyles.makeSectionDesc(this, "未选择模块。");
            envContainer.addView(empty);
            return;
        }

        if (envRows.isEmpty()) {
            TextView empty = UiStyles.makeSectionDesc(this, "该模块未定义 env.spec。\n如果是测试模块，请先安装最新内置 ZIP。");
            envContainer.addView(empty);
            return;
        }

        for (CtlParsers.ModuleEnvRow env : envRows) {
            LinearLayout rowCard = new LinearLayout(this);
            rowCard.setOrientation(LinearLayout.VERTICAL);
            rowCard.setPadding(UiStyles.dp(this, 10), UiStyles.dp(this, 8), UiStyles.dp(this, 10), UiStyles.dp(this, 8));
            rowCard.setBackground(UiStyles.makeRoundedDrawable(this, UiStyles.C_SURFACE_VARIANT, UiStyles.C_OUTLINE, 10, 1));

            LinearLayout.LayoutParams cardLp = new LinearLayout.LayoutParams(
                    LinearLayout.LayoutParams.MATCH_PARENT,
                    LinearLayout.LayoutParams.WRAP_CONTENT
            );
            cardLp.bottomMargin = UiStyles.dp(this, 8);
            rowCard.setLayoutParams(cardLp);

            TextView key = UiStyles.makeSectionTitle(this, env.label + "  (" + env.key + ")");
            key.setTextSize(13f);
            rowCard.addView(key);

            TextView desc = UiStyles.makeSectionDesc(this,
                    "type=" + env.type + " default=" + env.defaultValue + "\n" + env.description);
            rowCard.addView(desc, UiStyles.topMargin(this, 4));

            EditText valueInput = new EditText(this);
            UiStyles.styleEditText(this, valueInput);
            valueInput.setText(env.value);
            rowCard.addView(valueInput, UiStyles.topMargin(this, 6));

            envInputs.put(env.key, valueInput);
            envContainer.addView(rowCard);
        }
    }

    private void saveModuleEnv() {
        if (selectedModuleId == null || selectedModuleId.isEmpty() || envRows.isEmpty()) {
            setLogText("module_env_save_skipped=1 reason=no_module_or_env");
            return;
        }
        final String moduleId = selectedModuleId;
        final Map<String, String> valuesSnapshot = new HashMap<String, String>();
        for (CtlParsers.ModuleEnvRow env : envRows) {
            EditText input = envInputs.get(env.key);
            String value = input == null || input.getText() == null ? "" : input.getText().toString();
            valuesSnapshot.put(env.key, value);
        }

        worker.execute(new Runnable() {
            @Override
            public void run() {
                StringBuilder sb = new StringBuilder();
                int finalExit = 0;
                for (CtlParsers.ModuleEnvRow env : envRows) {
                    String value = valuesSnapshot.get(env.key);
                    if (value == null) {
                        value = "";
                    }
                    DsapiCtlClient.CmdResult result = repo.run("module", "env-set", moduleId, env.key, value);
                    sb.append("$ module env-set ")
                            .append(moduleId)
                            .append(' ')
                            .append(env.key)
                            .append(' ')
                            .append(value)
                            .append("\nexit=")
                            .append(result.exitCode);
                    if (!result.output.isEmpty()) {
                        sb.append("\n").append(result.output);
                    }
                    sb.append("\n\n");
                    if (result.exitCode != 0) {
                        finalExit = result.exitCode;
                    }
                }
                final int exitCode = finalExit;
                final String output = sb.toString();
                runOnUiThread(new Runnable() {
                    @Override
                    public void run() {
                        setLogText("$ batch env-save\nexit=" + exitCode + "\n" + output);
                        refreshEnvOnly();
                    }
                });
            }
        });
    }

    private void resetModuleEnv() {
        if (selectedModuleId == null || selectedModuleId.isEmpty() || envRows.isEmpty()) {
            setLogText("module_env_reset_skipped=1 reason=no_module_or_env");
            return;
        }
        final String moduleId = selectedModuleId;

        worker.execute(new Runnable() {
            @Override
            public void run() {
                StringBuilder sb = new StringBuilder();
                int finalExit = 0;
                for (CtlParsers.ModuleEnvRow env : envRows) {
                    DsapiCtlClient.CmdResult result = repo.run("module", "env-unset", moduleId, env.key);
                    sb.append("$ module env-unset ")
                            .append(moduleId)
                            .append(' ')
                            .append(env.key)
                            .append("\nexit=")
                            .append(result.exitCode);
                    if (!result.output.isEmpty()) {
                        sb.append("\n").append(result.output);
                    }
                    sb.append("\n\n");
                    if (result.exitCode != 0) {
                        finalExit = result.exitCode;
                    }
                }
                final int exitCode = finalExit;
                final String output = sb.toString();
                runOnUiThread(new Runnable() {
                    @Override
                    public void run() {
                        setLogText("$ batch env-reset\nexit=" + exitCode + "\n" + output);
                        refreshEnvOnly();
                    }
                });
            }
        });
    }

    private void runCtlAction(final String[] args, final boolean refreshAfter) {
        worker.execute(new Runnable() {
            @Override
            public void run() {
                final DsapiCtlClient.CmdResult result = repo.run(args);
                runOnUiThread(new Runnable() {
                    @Override
                    public void run() {
                        StringBuilder sb = new StringBuilder();
                        sb.append("$ ").append(DsapiCtlClient.joinSpace(args));
                        sb.append("\nexit=").append(result.exitCode);
                        if (!result.output.isEmpty()) {
                            sb.append("\n").append(result.output);
                        }
                        setLogText(sb.toString());
                        if (refreshAfter) {
                            refreshModulesAndEnv();
                        }
                    }
                });
            }
        });
    }

    private void showDangerConfirm(String title, String message, final Runnable confirm) {
        UiStyles.dialogBuilder(this)
                .setTitle(title)
                .setMessage(message)
                .setNegativeButton("取消", null)
                .setPositiveButton("确认", (dialog, which) -> {
                    if (confirm != null) {
                        confirm.run();
                    }
                })
                .show();
    }

    private void showFullLogDialog() {
        HorizontalScrollView scroll = new HorizontalScrollView(this);
        TextView tv = UiStyles.makeLogText(this);
        tv.setText(fullLogText == null ? "..." : fullLogText);
        scroll.addView(tv, new HorizontalScrollView.LayoutParams(
                HorizontalScrollView.LayoutParams.WRAP_CONTENT,
                HorizontalScrollView.LayoutParams.WRAP_CONTENT
        ));
        UiStyles.dialogBuilder(this)
                .setTitle("完整日志")
                .setView(scroll)
                .setPositiveButton("关闭", null)
                .show();
    }

    private void setLogText(String text) {
        fullLogText = text == null ? "" : text;
        String[] lines = fullLogText.split("\\r?\\n");
        StringBuilder preview = new StringBuilder();
        int max = Math.min(lines.length, 6);
        for (int i = 0; i < max; i++) {
            if (i > 0) {
                preview.append('\n');
            }
            preview.append(lines[i]);
        }
        if (lines.length > 6) {
            preview.append("\n... (").append(lines.length).append(" lines)");
        }
        logView.setText(preview.toString());
    }
}
