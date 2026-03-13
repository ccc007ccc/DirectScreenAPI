package org.directscreenapi.manager;

import android.app.Activity;
import android.app.AlertDialog;
import android.content.DialogInterface;
import android.content.Intent;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.os.SystemClock;
import android.view.View;
import android.widget.Button;
import android.widget.HorizontalScrollView;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;

import java.util.List;
import java.util.Locale;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.atomic.AtomicBoolean;

public final class MainActivity extends Activity implements View.OnClickListener {
    private static final long START_BACK_GUARD_MS = 1500L;
    private static final long BOOTSTRAP_REFRESH_DELAY_MS = 900L;
    private static final int ID_NAV_MODULES = 1001;
    private static final int ID_NAV_SETTINGS = 1002;
    private static final int ID_NAV_LOGS = 1003;
    private static final int ID_START_CORE = 1101;
    private static final int ID_STOP_CORE = 1102;
    private static final int ID_REFRESH = 1103;
    private static final int ID_STATUS = 1104;
    private static final int ID_RUNTIME_LIST = 1105;
    private static final int ID_BRIDGE_RESTART = 1106;

    private final Handler handler = new Handler(Looper.getMainLooper());
    private final ExecutorService worker = Executors.newSingleThreadExecutor();
    private final AtomicBoolean refreshInFlight = new AtomicBoolean(false);

    private ManagerConfig config;
    private ManagerRepository repo;

    private TextView subtitleView;
    private TextView chipCore;
    private TextView chipBridge;
    private TextView chipRuntime;
    private TextView chipModules;
    private TextView chipLastError;
    private TextView logView;
    private String fullLogText = "...";
    private long createdUptimeMs;

    private volatile boolean destroyed;
    private final Runnable bootstrapRefreshTask = new Runnable() {
        @Override
        public void run() {
            if (destroyed) {
                return;
            }
            refreshNow();
        }
    };

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        createdUptimeMs = SystemClock.uptimeMillis();
        config = ManagerConfig.load(this, getIntent());
        repo = new ManagerRepository(config);

        setTitle("DSAPI Manager · 主页");
        setContentView(buildContentView());

        refreshNow();
        scheduleBootstrapRefresh();
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
        refreshNow();
        scheduleBootstrapRefresh();
    }

    @Override
    protected void onPause() {
        super.onPause();
        try {
            handler.removeCallbacks(bootstrapRefreshTask);
        } catch (Throwable ignored) {
        }
    }

    @Override
    protected void onDestroy() {
        destroyed = true;
        try {
            handler.removeCallbacksAndMessages(null);
        } catch (Throwable ignored) {
        }
        try {
            worker.shutdownNow();
        } catch (Throwable ignored) {
        }
        super.onDestroy();
    }

    @Override
    public void onClick(View v) {
        int id = v.getId();
        if (id == ID_NAV_MODULES) {
            Intent intent = new Intent(this, ModulesActivity.class);
            intent.addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP | Intent.FLAG_ACTIVITY_SINGLE_TOP);
            startActivity(config.applyToIntent(intent));
            finish();
            overridePendingTransition(0, 0);
            return;
        }
        if (id == ID_NAV_SETTINGS) {
            Intent intent = new Intent(this, SettingsActivity.class);
            intent.addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP | Intent.FLAG_ACTIVITY_SINGLE_TOP);
            startActivity(config.applyToIntent(intent));
            finish();
            overridePendingTransition(0, 0);
            return;
        }
        if (id == ID_NAV_LOGS) {
            Intent intent = new Intent(this, LogsActivity.class);
            intent.addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP | Intent.FLAG_ACTIVITY_SINGLE_TOP);
            startActivity(config.applyToIntent(intent));
            finish();
            overridePendingTransition(0, 0);
            return;
        }
        if (id == ID_START_CORE) {
            runCtlAction(new String[]{"start"}, true);
            return;
        }
        if (id == ID_STOP_CORE) {
            showDangerConfirm(
                    "停止核心",
                    "将停止 DSAPI daemon，正在运行的能力会中断。",
                    new String[]{"capability", "stop", "core.daemon"},
                    true
            );
            return;
        }
        if (id == ID_REFRESH) {
            refreshNow();
            return;
        }
        if (id == ID_STATUS) {
            runCtlAction(new String[]{"status"}, false);
            return;
        }
        if (id == ID_RUNTIME_LIST) {
            runCtlAction(new String[]{"runtime", "list"}, false);
            return;
        }
        if (id == ID_BRIDGE_RESTART) {
            showDangerConfirm(
                    "重启桥接",
                    "会短暂中断 Manager 与核心通信。",
                    new String[]{"bridge", "restart"},
                    true
            );
        }
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
        LinearLayout content = UiStyles.buildPageRoot(this);
        content.setPadding(UiStyles.dp(this, 12), UiStyles.dp(this, 8), UiStyles.dp(this, 12), UiStyles.dp(this, 8));
        page.addView(content, new ScrollView.LayoutParams(
                ScrollView.LayoutParams.MATCH_PARENT,
                ScrollView.LayoutParams.WRAP_CONTENT
        ));
        content.addView(buildStatusCard());
        content.addView(buildLogCard(), UiStyles.cardLayout(this));

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
        UiStyles.HeaderBar bar = UiStyles.makeHeaderBar(this, "DSAPI Core Console", "加载中...");
        subtitleView = bar.subtitle;
        return bar.root;
    }

    private View buildBottomNav() {
        LinearLayout nav = new LinearLayout(this);
        nav.setOrientation(LinearLayout.HORIZONTAL);
        nav.setPadding(UiStyles.dp(this, 10), UiStyles.dp(this, 8), UiStyles.dp(this, 10), UiStyles.dp(this, 8));
        nav.setBackground(UiStyles.makeRoundedDrawable(this, UiStyles.C_SURFACE, UiStyles.C_OUTLINE, 0, 0));

        Button home = UiStyles.makeNavButton(this, "主页", true);
        home.setEnabled(false);
        nav.addView(home, UiStyles.rowWeightLayout(this));

        Button modules = UiStyles.makeNavButton(this, "模块", false);
        modules.setId(ID_NAV_MODULES);
        modules.setOnClickListener(this);
        nav.addView(modules, UiStyles.rowWeightLayout(this));

        Button logs = UiStyles.makeNavButton(this, "日志", false);
        logs.setId(ID_NAV_LOGS);
        logs.setOnClickListener(this);
        nav.addView(logs, UiStyles.rowWeightLayout(this));

        Button settings = UiStyles.makeNavButton(this, "设置", false);
        settings.setId(ID_NAV_SETTINGS);
        settings.setOnClickListener(this);
        nav.addView(settings, UiStyles.rowWeightLayout(this));
        return nav;
    }

    private View buildStatusCard() {
        LinearLayout card = UiStyles.makeCard(this);
        card.addView(UiStyles.makeSectionTitle(this, "核心状态"));
        TextView desc = UiStyles.makeSectionDesc(this, "查看 daemon/bridge/runtime，并执行核心操作");
        card.addView(desc, UiStyles.topMargin(this, 2));

        LinearLayout chipRow1 = new LinearLayout(this);
        chipRow1.setOrientation(LinearLayout.HORIZONTAL);
        chipCore = UiStyles.makeChip(this, "核心: ...", UiStyles.C_SECONDARY_CONTAINER);
        chipBridge = UiStyles.makeChip(this, "桥接: ...", UiStyles.C_SECONDARY_CONTAINER);
        chipRow1.addView(chipCore, chipLayout(0));
        chipRow1.addView(chipBridge, chipLayout(8));
        card.addView(chipRow1, UiStyles.topMargin(this, 10));

        LinearLayout chipRow2 = new LinearLayout(this);
        chipRow2.setOrientation(LinearLayout.HORIZONTAL);
        chipRuntime = UiStyles.makeChip(this, "运行时: ...", UiStyles.C_SECONDARY_CONTAINER);
        chipModules = UiStyles.makeChip(this, "模块: ...", UiStyles.C_SECONDARY_CONTAINER);
        chipRow2.addView(chipRuntime, chipLayout(0));
        chipRow2.addView(chipModules, chipLayout(8));
        card.addView(chipRow2, UiStyles.topMargin(this, 8));

        LinearLayout chipRow3 = new LinearLayout(this);
        chipRow3.setOrientation(LinearLayout.HORIZONTAL);
        chipLastError = UiStyles.makeChip(this, "最近错误: none", UiStyles.C_OK_CONTAINER);
        chipRow3.addView(chipLastError, chipLayout(0));
        card.addView(chipRow3, UiStyles.topMargin(this, 8));

        LinearLayout row1 = new LinearLayout(this);
        row1.setOrientation(LinearLayout.HORIZONTAL);

        Button startCore = UiStyles.makeFilledButton(this, "启动核心");
        startCore.setId(ID_START_CORE);
        startCore.setOnClickListener(this);
        row1.addView(startCore, UiStyles.rowWeightLayout(this));

        Button stopCore = UiStyles.makeWarningButton(this, "停止核心");
        stopCore.setId(ID_STOP_CORE);
        stopCore.setOnClickListener(this);
        row1.addView(stopCore, UiStyles.rowWeightLayout(this));

        Button more = UiStyles.makeTonalButton(this, "更多操作");
        more.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                showMoreActionsDialog();
            }
        });
        row1.addView(more, UiStyles.rowWeightLayout(this));

        card.addView(row1, UiStyles.topMargin(this, 10));

        return card;
    }

    private View buildLogCard() {
        LinearLayout card = UiStyles.makeCard(this);
        card.addView(UiStyles.makeSectionTitle(this, "命令日志"));
        TextView desc = UiStyles.makeSectionDesc(this, "显示最近一次输出（可展开完整日志）");
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

    private LinearLayout.LayoutParams chipLayout(int leftMarginDp) {
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.WRAP_CONTENT,
                LinearLayout.LayoutParams.WRAP_CONTENT
        );
        lp.leftMargin = UiStyles.dp(this, leftMarginDp);
        return lp;
    }

    private void refreshNow() {
        if (!refreshInFlight.compareAndSet(false, true)) {
            return;
        }
        worker.execute(new RefreshWorker());
    }

    private void renderStatus(ManagerRepository.HomeSnapshot snapshot) {
        String errorText = "none";
        int errorColor = UiStyles.C_OK_CONTAINER;
        if (!"none".equalsIgnoreCase(snapshot.lastErrorState)) {
            errorText = snapshot.lastErrorCode + "/" + snapshot.lastErrorMessage;
            errorColor = UiStyles.C_ERROR_CONTAINER;
        }

        subtitleView.setText(
                "ctl=" + config.ctlPath
                        + "\ntransport=" + config.transport
                        + "\nbridge_service=" + config.bridgeService
                        + " zygote=" + snapshot.zygoteState
                        + "\nrefresh=" + config.refreshMs + "ms status_exit=" + snapshot.status.exitCode
                        + " enabled=" + snapshot.enabled
                        + " daemon_registry=" + snapshot.daemonModuleCount
                        + "/" + snapshot.daemonModuleEventSeq
                        + " z_scope=" + snapshot.zygoteScopeCount
        );

        updateChip(chipCore, "核心: " + snapshot.coreState, stateColor(snapshot.coreState));
        updateChip(chipBridge, "桥接: " + snapshot.bridgeState, stateColor(snapshot.bridgeState));
        updateChip(chipRuntime, "运行时: " + snapshot.runtimeActive, UiStyles.C_PRIMARY_CONTAINER);
        updateChip(chipModules, "模块: " + snapshot.moduleCount, UiStyles.C_SECONDARY_CONTAINER);
        updateChip(chipLastError, "最近错误: " + errorText, errorColor);
    }

    private int stateColor(String state) {
        if (state == null) {
            return UiStyles.C_SECONDARY_CONTAINER;
        }
        String s = state.toLowerCase(Locale.US);
        if (s.startsWith("running") || s.startsWith("ok") || s.startsWith("ready") || s.startsWith("started")) {
            return UiStyles.C_OK_CONTAINER;
        }
        if (s.startsWith("stop")) {
            return UiStyles.C_WARNING_CONTAINER;
        }
        if (s.startsWith("error") || s.startsWith("fail") || s.startsWith("missing")) {
            return UiStyles.C_ERROR_CONTAINER;
        }
        if (s.startsWith("disabled")) {
            return UiStyles.C_WARNING_CONTAINER;
        }
        return UiStyles.C_SECONDARY_CONTAINER;
    }

    private void scheduleBootstrapRefresh() {
        try {
            handler.removeCallbacks(bootstrapRefreshTask);
            handler.postDelayed(bootstrapRefreshTask, BOOTSTRAP_REFRESH_DELAY_MS);
        } catch (Throwable ignored) {
        }
    }

    private void updateChip(TextView chip, String text, int bgColor) {
        chip.setText(text);
        chip.setBackground(UiStyles.makeRoundedDrawable(this, bgColor, 0, 999, 0));
    }

    private void runCtlAction(String[] args, boolean refreshAfter) {
        worker.execute(new ActionWorker(args, refreshAfter));
    }

    private void showMoreActionsDialog() {
        final String[] labels = new String[]{
                "刷新",
                "状态详情",
                "运行时列表",
                "重启桥接",
                "清空错误状态"
        };
        UiStyles.dialogBuilder(this)
                .setTitle("更多操作")
                .setItems(labels, new android.content.DialogInterface.OnClickListener() {
                    @Override
                    public void onClick(android.content.DialogInterface dialog, int which) {
                        if (which == 0) {
                            runCtlAction(new String[]{"status"}, true);
                            return;
                        }
                        if (which == 1) {
                            runCtlAction(new String[]{"status"}, false);
                            return;
                        }
                        if (which == 2) {
                            runCtlAction(new String[]{"runtime", "list"}, false);
                            return;
                        }
                        if (which == 3) {
                            showDangerConfirm(
                                    "重启桥接",
                                    "会短暂中断 Manager 与核心通信。",
                                    new String[]{"bridge", "restart"},
                                    true
                            );
                            return;
                        }
                        runCtlAction(new String[]{"errors", "clear"}, true);
                    }
                })
                .setNegativeButton("取消", null)
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

    private void showDangerConfirm(String title, String message, String[] args, boolean refreshAfter) {
        UiStyles.dialogBuilder(this)
                .setTitle(title)
                .setMessage(message)
                .setNegativeButton("取消", null)
                .setPositiveButton("确认", new ConfirmClickListener(args, refreshAfter))
                .show();
    }

    private final class RefreshWorker implements Runnable {
        @Override
        public void run() {
            ManagerRepository.HomeSnapshot snapshot = repo.loadHomeSnapshot();
            runOnUiThread(new RenderStatusWorker(snapshot));
        }
    }

    private final class RenderStatusWorker implements Runnable {
        private final ManagerRepository.HomeSnapshot snapshot;

        RenderStatusWorker(ManagerRepository.HomeSnapshot snapshot) {
            this.snapshot = snapshot;
        }

        @Override
        public void run() {
            try {
                renderStatus(snapshot);
            } finally {
                refreshInFlight.set(false);
            }
        }
    }

    private final class ActionWorker implements Runnable {
        private final String[] args;
        private final boolean refreshAfter;

        ActionWorker(String[] args, boolean refreshAfter) {
            this.args = args;
            this.refreshAfter = refreshAfter;
        }

        @Override
        public void run() {
            DsapiCtlClient.CmdResult result = repo.run(args);
            runOnUiThread(new RenderActionWorker(args, result, refreshAfter));
        }
    }

    private final class RenderActionWorker implements Runnable {
        private final String[] args;
        private final DsapiCtlClient.CmdResult result;
        private final boolean refreshAfter;

        RenderActionWorker(String[] args, DsapiCtlClient.CmdResult result, boolean refreshAfter) {
            this.args = args;
            this.result = result;
            this.refreshAfter = refreshAfter;
        }

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
                refreshNow();
            }
        }
    }

    private final class ConfirmClickListener implements DialogInterface.OnClickListener {
        private final String[] args;
        private final boolean refreshAfter;

        ConfirmClickListener(String[] args, boolean refreshAfter) {
            this.args = args;
            this.refreshAfter = refreshAfter;
        }

        @Override
        public void onClick(DialogInterface dialog, int which) {
            runCtlAction(args, refreshAfter);
        }
    }
}
