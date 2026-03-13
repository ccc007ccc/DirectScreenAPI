package org.directscreenapi.manager;

import android.app.Activity;
import android.app.AlertDialog;
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

import java.util.Locale;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.atomic.AtomicBoolean;

public final class LogsActivity extends Activity {
    private static final long START_BACK_GUARD_MS = 1500L;
    private static final int ID_NAV_HOME = 3001;
    private static final int ID_NAV_MODULES = 3002;
    private static final int ID_NAV_SETTINGS = 3003;

    private final Handler handler = new Handler(Looper.getMainLooper());
    private final ExecutorService worker = Executors.newSingleThreadExecutor();
    private final AtomicBoolean refreshInFlight = new AtomicBoolean(false);

    private ManagerConfig config;
    private ManagerRepository repo;

    private TextView subtitleView;
    private TextView chipCore;
    private TextView chipBridge;
    private TextView chipModules;
    private TextView chipError;
    private TextView logView;
    private String fullLogText = "...";
    private long createdUptimeMs;

    private volatile boolean destroyed;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        createdUptimeMs = SystemClock.uptimeMillis();
        config = ManagerConfig.load(this, getIntent());
        repo = new ManagerRepository(config);

        setTitle("DSAPI Manager · 日志");
        setContentView(buildContentView());

        refreshNow();
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
    }

    @Override
    protected void onPause() {
        super.onPause();
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

        root.addView(buildStatusCard());
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
        UiStyles.HeaderBar bar = UiStyles.makeHeaderBar(this, "DSAPI Logs", "加载中...");
        subtitleView = bar.subtitle;
        return bar.root;
    }

    private View buildBottomNav() {
        LinearLayout nav = new LinearLayout(this);
        nav.setOrientation(LinearLayout.HORIZONTAL);
        nav.setPadding(UiStyles.dp(this, 10), UiStyles.dp(this, 8), UiStyles.dp(this, 10), UiStyles.dp(this, 8));
        nav.setBackground(UiStyles.makeRoundedDrawable(this, UiStyles.C_SURFACE, UiStyles.C_OUTLINE, 0, 0));

        Button home = UiStyles.makeNavButton(this, "主页", false);
        home.setId(ID_NAV_HOME);
        home.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                Intent intent = new Intent(LogsActivity.this, MainActivity.class);
                intent.addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP | Intent.FLAG_ACTIVITY_SINGLE_TOP);
                startActivity(config.applyToIntent(intent));
                finish();
                overridePendingTransition(0, 0);
            }
        });
        nav.addView(home, UiStyles.rowWeightLayout(this));

        Button modules = UiStyles.makeNavButton(this, "模块", false);
        modules.setId(ID_NAV_MODULES);
        modules.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                Intent intent = new Intent(LogsActivity.this, ModulesActivity.class);
                intent.addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP | Intent.FLAG_ACTIVITY_SINGLE_TOP);
                startActivity(config.applyToIntent(intent));
                finish();
                overridePendingTransition(0, 0);
            }
        });
        nav.addView(modules, UiStyles.rowWeightLayout(this));

        Button logs = UiStyles.makeNavButton(this, "日志", true);
        logs.setEnabled(false);
        nav.addView(logs, UiStyles.rowWeightLayout(this));

        Button settings = UiStyles.makeNavButton(this, "设置", false);
        settings.setId(ID_NAV_SETTINGS);
        settings.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                Intent intent = new Intent(LogsActivity.this, SettingsActivity.class);
                intent.addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP | Intent.FLAG_ACTIVITY_SINGLE_TOP);
                startActivity(config.applyToIntent(intent));
                finish();
                overridePendingTransition(0, 0);
            }
        });
        nav.addView(settings, UiStyles.rowWeightLayout(this));
        return nav;
    }

    private View buildStatusCard() {
        LinearLayout card = UiStyles.makeCard(this);
        card.addView(UiStyles.makeSectionTitle(this, "日志状态"));
        TextView desc = UiStyles.makeSectionDesc(this, "聚合最近错误与核心状态，便于快速定位故障");
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
        chipModules = UiStyles.makeChip(this, "模块: ...", UiStyles.C_SECONDARY_CONTAINER);
        chipError = UiStyles.makeChip(this, "最近错误: ...", UiStyles.C_WARNING_CONTAINER);
        chipRow2.addView(chipModules, chipLayout(0));
        chipRow2.addView(chipError, chipLayout(8));
        card.addView(chipRow2, UiStyles.topMargin(this, 8));

        LinearLayout actionRow = new LinearLayout(this);
        actionRow.setOrientation(LinearLayout.HORIZONTAL);

        Button refresh = UiStyles.makeTonalButton(this, "刷新");
        refresh.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                refreshNow();
            }
        });
        actionRow.addView(refresh, UiStyles.rowWeightLayout(this));

        Button clearError = UiStyles.makeWarningButton(this, "清空错误");
        clearError.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                runCtlAction(new String[]{"errors", "clear"}, true);
            }
        });
        actionRow.addView(clearError, UiStyles.rowWeightLayout(this));

        Button status = UiStyles.makeTonalButton(this, "查看状态");
        status.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                runCtlAction(new String[]{"status"}, false);
            }
        });
        actionRow.addView(status, UiStyles.rowWeightLayout(this));

        card.addView(actionRow, UiStyles.topMargin(this, 10));
        return card;
    }

    private View buildLogCard() {
        LinearLayout card = UiStyles.makeCard(this);
        card.addView(UiStyles.makeSectionTitle(this, "错误与链路日志"));
        TextView desc = UiStyles.makeSectionDesc(this, "默认展示 `errors last` + `status` + `module list` 结果");
        card.addView(desc, UiStyles.topMargin(this, 2));

        HorizontalScrollView scroll = new HorizontalScrollView(this);
        logView = UiStyles.makeLogText(this);
        logView.setText("...");
        scroll.addView(logView, new HorizontalScrollView.LayoutParams(
                HorizontalScrollView.LayoutParams.WRAP_CONTENT,
                HorizontalScrollView.LayoutParams.WRAP_CONTENT
        ));
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                UiStyles.dp(this, 220)
        );
        lp.topMargin = UiStyles.dp(this, 10);
        card.addView(scroll, lp);

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
        worker.execute(new Runnable() {
            @Override
            public void run() {
                final ManagerRepository.LogsSnapshot snapshot = repo.loadLogsSnapshot();
                runOnUiThread(new Runnable() {
                    @Override
                    public void run() {
                        try {
                            renderSnapshot(snapshot);
                        } finally {
                            refreshInFlight.set(false);
                        }
                    }
                });
            }
        });
    }

    private void renderSnapshot(ManagerRepository.LogsSnapshot snapshot) {
        ManagerRepository.HomeSnapshot home = snapshot.home;
        String lastErrorState = home == null ? "none" : home.lastErrorState;
        String lastErrorCode = home == null ? "-" : home.lastErrorCode;
        String lastErrorMessage = home == null ? "-" : home.lastErrorMessage;

        subtitleView.setText(
                "runtime=" + (home == null ? "<none>" : home.runtimeActive)
                        + "  module_count=" + (home == null ? "0" : home.moduleCount)
                        + "  daemon_registry=" + (home == null ? "0" : home.daemonModuleCount)
                        + "  z_scope=" + (home == null ? "0" : home.zygoteScopeCount)
                        + "\ntransport=" + config.transport
                        + "\nbridge_service=" + config.bridgeService
                        + "  zygote=" + (home == null ? "unknown" : home.zygoteState)
                        + "  refresh=" + config.refreshMs + "ms"
        );

        updateChip(chipCore, "核心: " + (home == null ? "unknown" : home.coreState), stateColor(home == null ? "unknown" : home.coreState));
        updateChip(chipBridge, "桥接: " + (home == null ? "unknown" : home.bridgeState), stateColor(home == null ? "unknown" : home.bridgeState));
        updateChip(chipModules, "模块: " + (home == null ? "0" : home.moduleCount), UiStyles.C_SECONDARY_CONTAINER);
        if ("none".equalsIgnoreCase(lastErrorState)) {
            updateChip(chipError, "最近错误: none", UiStyles.C_OK_CONTAINER);
        } else {
            updateChip(chipError, "最近错误: " + lastErrorCode, UiStyles.C_ERROR_CONTAINER);
        }

        StringBuilder sb = new StringBuilder();
        sb.append("$ errors last\nexit=").append(snapshot.lastError.exitCode);
        if (!snapshot.lastError.output.isEmpty()) {
            sb.append("\n").append(snapshot.lastError.output);
        }
        sb.append("\n\n$ status\nexit=").append(home == null ? -1 : home.status.exitCode);
        if (home != null && !home.status.output.isEmpty()) {
            sb.append("\n").append(home.status.output);
        }
        sb.append("\n\n$ module list\nexit=").append(snapshot.moduleList.exitCode);
        if (!snapshot.moduleList.output.isEmpty()) {
            sb.append("\n").append(snapshot.moduleList.output);
        }
        if (!"none".equalsIgnoreCase(lastErrorState)) {
            sb.append("\n\nlast_error_hint=").append(lastErrorCode).append("/").append(lastErrorMessage);
        }
        setLogText(sb.toString());
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
                            refreshNow();
                        }
                    }
                });
            }
        });
    }

    private int stateColor(String state) {
        if (state == null) {
            return UiStyles.C_SECONDARY_CONTAINER;
        }
        String s = state.toLowerCase(Locale.US);
        if (s.startsWith("running") || s.startsWith("ok") || s.startsWith("ready") || s.startsWith("started")) {
            return UiStyles.C_OK_CONTAINER;
        }
        if (s.startsWith("stop") || s.startsWith("error") || s.startsWith("fail") || s.startsWith("missing")) {
            return UiStyles.C_ERROR_CONTAINER;
        }
        if (s.startsWith("disabled")) {
            return UiStyles.C_WARNING_CONTAINER;
        }
        return UiStyles.C_SECONDARY_CONTAINER;
    }

    private void updateChip(TextView chip, String text, int bgColor) {
        chip.setText(text);
        chip.setBackground(UiStyles.makeRoundedDrawable(this, bgColor, 0, 999, 0));
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
        int max = Math.min(lines.length, 14);
        for (int i = 0; i < max; i++) {
            if (i > 0) {
                preview.append('\n');
            }
            preview.append(lines[i]);
        }
        if (lines.length > max) {
            preview.append("\n... (").append(lines.length).append(" lines)");
        }
        logView.setText(preview.toString());
    }

}
