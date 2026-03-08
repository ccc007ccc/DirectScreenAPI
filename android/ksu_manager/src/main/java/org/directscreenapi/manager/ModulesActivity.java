package org.directscreenapi.manager;

import android.app.Activity;
import android.app.AlertDialog;
import android.content.Intent;
import android.database.Cursor;
import android.net.Uri;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.os.SystemClock;
import android.provider.OpenableColumns;
import android.view.Gravity;
import android.view.View;
import android.widget.Button;
import android.widget.HorizontalScrollView;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;

import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.atomic.AtomicBoolean;

public final class ModulesActivity extends Activity {
    private static final long START_BACK_GUARD_MS = 1500L;
    private static final long MODULE_CACHE_MAX_AGE_MS = 3000L;
    private static final long MODULE_CACHE_RUNNING_MAX_AGE_MS = 1200L;
    private static final int REQ_PICK_MODULE_ZIP = 2001;
    private static final int ID_NAV_HOME = 2101;
    private static final int ID_NAV_SETTINGS = 2102;
    private static final int ID_NAV_LOGS = 2103;

    private final Handler handler = new Handler(Looper.getMainLooper());
    private final ExecutorService worker = Executors.newSingleThreadExecutor();
    private final AtomicBoolean refreshInFlight = new AtomicBoolean(false);

    private ManagerConfig config;
    private ManagerRepository repo;

    private TextView subtitleView;
    private LinearLayout moduleContainer;
    private TextView logView;
    private String fullLogText = "...";
    private String lastModuleEventSeq = "";
    private final List<CtlParsers.ModuleRow> cachedModules = new ArrayList<CtlParsers.ModuleRow>();
    private long lastModulesReloadUptimeMs;
    private boolean autoRefreshScheduled;
    private long createdUptimeMs;

    private volatile boolean destroyed;
    private final Runnable autoRefreshTask = new Runnable() {
        @Override
        public void run() {
            if (destroyed || !autoRefreshScheduled) {
                return;
            }
            refreshNow(false);
            long delayMs = Math.max(500L, (long) config.refreshMs);
            handler.postDelayed(this, delayMs);
        }
    };

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        createdUptimeMs = SystemClock.uptimeMillis();
        config = ManagerConfig.load(this, getIntent());
        repo = new ManagerRepository(config);

        setTitle("DSAPI Manager · 模块");
        setContentView(buildContentView());

        refreshNow(true);
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
        refreshNow(true);
        startAutoRefresh();
    }

    @Override
    protected void onPause() {
        super.onPause();
        stopAutoRefresh();
    }

    @Override
    protected void onDestroy() {
        destroyed = true;
        try {
            handler.removeCallbacksAndMessages(null);
        } catch (Throwable ignored) {
        }
        stopAutoRefresh();
        try {
            worker.shutdownNow();
        } catch (Throwable ignored) {
        }
        super.onDestroy();
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (requestCode != REQ_PICK_MODULE_ZIP || resultCode != RESULT_OK || data == null) {
            return;
        }
        final Uri uri = data.getData();
        if (uri == null) {
            return;
        }
        worker.execute(new Runnable() {
            @Override
            public void run() {
                String copiedPath;
                try {
                    copiedPath = copyUriToImportZip(uri);
                } catch (Throwable t) {
                    final String message = "zip_import_error=" + t.getClass().getSimpleName() + ":" + t.getMessage();
                    runOnUiThread(new Runnable() {
                        @Override
                        public void run() {
                            setLogText(message);
                        }
                    });
                    return;
                }

                final DsapiCtlClient.CmdResult result = repo.run("module", "install-zip", copiedPath);
                try {
                    new File(copiedPath).delete();
                } catch (Throwable ignored) {
                }
                runOnUiThread(new Runnable() {
                    @Override
                    public void run() {
                        StringBuilder sb = new StringBuilder();
                        sb.append("$ module install-zip ").append(copiedPath);
                        sb.append("\nexit=").append(result.exitCode);
                        if (!result.output.isEmpty()) {
                            sb.append("\n").append(result.output);
                        }
                        setLogText(sb.toString());
                        refreshNow(true);
                    }
                });
            }
        });
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

        root.addView(buildToolbarCard());
        root.addView(buildModuleListCard(), UiStyles.cardLayout(this));
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
        UiStyles.HeaderBar bar = UiStyles.makeHeaderBar(this, "DSAPI Modules", "加载中...");
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
                Intent intent = new Intent(ModulesActivity.this, MainActivity.class);
                intent.addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP | Intent.FLAG_ACTIVITY_SINGLE_TOP);
                startActivity(config.applyToIntent(intent));
                finish();
                overridePendingTransition(0, 0);
            }
        });
        nav.addView(home, UiStyles.rowWeightLayout(this));

        Button modules = UiStyles.makeNavButton(this, "模块", true);
        modules.setEnabled(false);
        nav.addView(modules, UiStyles.rowWeightLayout(this));

        Button logs = UiStyles.makeNavButton(this, "日志", false);
        logs.setId(ID_NAV_LOGS);
        logs.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                Intent intent = new Intent(ModulesActivity.this, LogsActivity.class);
                intent.addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP | Intent.FLAG_ACTIVITY_SINGLE_TOP);
                startActivity(config.applyToIntent(intent));
                finish();
                overridePendingTransition(0, 0);
            }
        });
        nav.addView(logs, UiStyles.rowWeightLayout(this));

        Button settings = UiStyles.makeNavButton(this, "设置", false);
        settings.setId(ID_NAV_SETTINGS);
        settings.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                Intent intent = new Intent(ModulesActivity.this, SettingsActivity.class);
                intent.addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP | Intent.FLAG_ACTIVITY_SINGLE_TOP);
                startActivity(config.applyToIntent(intent));
                finish();
                overridePendingTransition(0, 0);
            }
        });
        nav.addView(settings, UiStyles.rowWeightLayout(this));
        return nav;
    }

    private View buildToolbarCard() {
        LinearLayout card = UiStyles.makeCard(this);
        card.addView(UiStyles.makeSectionTitle(this, "模块管理"));
        TextView desc = UiStyles.makeSectionDesc(this, "支持 ZIP 导入、内置模块安装、生命周期与 Action 管理");
        card.addView(desc, UiStyles.topMargin(this, 2));

        LinearLayout row = new LinearLayout(this);
        row.setOrientation(LinearLayout.HORIZONTAL);

        Button refresh = UiStyles.makeTonalButton(this, "刷新");
        refresh.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                refreshNow(true);
            }
        });
        row.addView(refresh, UiStyles.rowWeightLayout(this));

        Button importZip = UiStyles.makeFilledButton(this, "添加 ZIP 模块");
        importZip.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                launchZipPicker();
            }
        });
        row.addView(importZip, UiStyles.rowWeightLayout(this));

        Button installBuiltin = UiStyles.makeTonalButton(this, "安装内置");
        installBuiltin.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                showBuiltinZipPicker();
            }
        });
        row.addView(installBuiltin, UiStyles.rowWeightLayout(this));

        card.addView(row, UiStyles.topMargin(this, 10));

        LinearLayout row2 = new LinearLayout(this);
        row2.setOrientation(LinearLayout.HORIZONTAL);

        Button reloadAll = UiStyles.makeWarningButton(this, "重载全部");
        reloadAll.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                showDangerConfirm("重载全部模块", "将依次执行每个模块 reload，失败会写入最近错误。", new Runnable() {
                    @Override
                    public void run() {
                        runCtlAction(new String[]{"module", "reload-all"}, true, false, null);
                    }
                });
            }
        });
        row2.addView(reloadAll, UiStyles.rowWeightLayout(this));

        Button clearError = UiStyles.makeTonalButton(this, "清空错误");
        clearError.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                runCtlAction(new String[]{"errors", "clear"}, true, false, null);
            }
        });
        row2.addView(clearError, UiStyles.rowWeightLayout(this));

        card.addView(row2, UiStyles.topMargin(this, 8));
        return card;
    }

    private View buildModuleListCard() {
        LinearLayout card = UiStyles.makeCard(this);
        card.addView(UiStyles.makeSectionTitle(this, "模块列表"));
        TextView desc = UiStyles.makeSectionDesc(this, "每个模块提供独立生命周期、Action 按钮与详情");
        card.addView(desc, UiStyles.topMargin(this, 2));

        moduleContainer = new LinearLayout(this);
        moduleContainer.setOrientation(LinearLayout.VERTICAL);
        card.addView(moduleContainer, UiStyles.topMargin(this, 10));
        return card;
    }

    private View buildLogCard() {
        LinearLayout card = UiStyles.makeCard(this);
        card.addView(UiStyles.makeSectionTitle(this, "命令日志"));
        TextView desc = UiStyles.makeSectionDesc(this, "展示最近一次模块命令输出");
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
                UiStyles.dp(this, 110)
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

    private void launchZipPicker() {
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
        intent.addCategory(Intent.CATEGORY_OPENABLE);
        intent.setType("application/zip");
        intent.putExtra(Intent.EXTRA_MIME_TYPES, new String[]{"application/zip", "application/octet-stream", "*/*"});
        startActivityForResult(intent, REQ_PICK_MODULE_ZIP);
    }

    private void showBuiltinZipPicker() {
        worker.execute(new Runnable() {
            @Override
            public void run() {
                final DsapiCtlClient.CmdResult result = repo.run("module", "zip-list");
                final List<CtlParsers.ModuleZipRow> rows = CtlParsers.parseModuleZipRows(result.output);
                runOnUiThread(new Runnable() {
                    @Override
                    public void run() {
                        if (result.exitCode != 0) {
                            StringBuilder sb = new StringBuilder();
                            sb.append("$ module zip-list\nexit=").append(result.exitCode);
                            if (!result.output.isEmpty()) {
                                sb.append("\n").append(result.output);
                            }
                            setLogText(sb.toString());
                            return;
                        }
                        if (rows.isEmpty()) {
                            setLogText("$ module zip-list\nexit=0\nno_builtin_zip=1");
                            return;
                        }
                        String[] names = new String[rows.size()];
                        for (int i = 0; i < rows.size(); i++) {
                            names[i] = rows.get(i).name;
                        }
                        UiStyles.dialogBuilder(ModulesActivity.this)
                                .setTitle("安装内置模块")
                                .setItems(names, (dialog, which) -> runCtlAction(
                                        new String[]{"module", "install-builtin", rows.get(which).name},
                                        true,
                                        false,
                                        null
                                ))
                                .setNegativeButton("取消", null)
                                .show();
                    }
                });
            }
        });
    }

    private String copyUriToImportZip(Uri uri) throws Exception {
        String displayName = queryDisplayName(uri);
        if (displayName == null || displayName.trim().isEmpty()) {
            displayName = "module.zip";
        }
        displayName = displayName.replaceAll("[^A-Za-z0-9._-]", "_");
        if (!displayName.toLowerCase(Locale.US).endsWith(".zip")) {
            displayName = displayName + ".zip";
        }

        File baseDir = new File(getCacheDir(), "imports");
        if (!baseDir.exists() && !baseDir.mkdirs()) {
            throw new IllegalStateException("imports_dir_create_failed");
        }
        cleanupOldImports(baseDir, 5);

        File out = new File(baseDir, "install_" + displayName);
        if (out.exists()) {
            out.delete();
        }
        InputStream in = null;
        FileOutputStream fos = null;
        try {
            in = getContentResolver().openInputStream(uri);
            if (in == null) {
                throw new IllegalStateException("open_stream_failed");
            }
            fos = new FileOutputStream(out);
            byte[] buffer = new byte[8192];
            int n;
            while ((n = in.read(buffer)) > 0) {
                fos.write(buffer, 0, n);
            }
            fos.flush();
        } finally {
            if (in != null) {
                try {
                    in.close();
                } catch (Throwable ignored) {
                }
            }
            if (fos != null) {
                try {
                    fos.close();
                } catch (Throwable ignored) {
                }
            }
        }
        return out.getAbsolutePath();
    }

    private void cleanupOldImports(File baseDir, int keepCount) {
        File[] files = baseDir.listFiles();
        if (files == null || files.length <= keepCount) {
            return;
        }
        java.util.Arrays.sort(files, (a, b) -> Long.compare(b.lastModified(), a.lastModified()));
        for (int i = keepCount; i < files.length; i++) {
            try {
                files[i].delete();
            } catch (Throwable ignored) {
            }
        }
    }

    private String queryDisplayName(Uri uri) {
        Cursor cursor = null;
        try {
            cursor = getContentResolver().query(uri, new String[]{OpenableColumns.DISPLAY_NAME}, null, null, null);
            if (cursor != null && cursor.moveToFirst()) {
                int idx = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME);
                if (idx >= 0) {
                    return cursor.getString(idx);
                }
            }
        } catch (Throwable ignored) {
        } finally {
            if (cursor != null) {
                try {
                    cursor.close();
                } catch (Throwable ignored) {
                }
            }
        }
        return null;
    }

    private void refreshNow() {
        refreshNow(false);
    }

    private void refreshNow(final boolean forceFull) {
        if (!refreshInFlight.compareAndSet(false, true)) {
            return;
        }
        worker.execute(new Runnable() {
            @Override
            public void run() {
                final ManagerRepository.HomeSnapshot home = repo.loadHomeSnapshot();
                boolean shouldReloadModules = forceFull || cachedModules.isEmpty();
                long nowUptime = SystemClock.uptimeMillis();

                if (!shouldReloadModules) {
                    boolean hasRunningCache = hasRunningModules(cachedModules);
                    long maxCacheAge = hasRunningCache ? MODULE_CACHE_RUNNING_MAX_AGE_MS : MODULE_CACHE_MAX_AGE_MS;
                    if (nowUptime - lastModulesReloadUptimeMs >= maxCacheAge) {
                        shouldReloadModules = true;
                    }
                }

                if (!shouldReloadModules) {
                    String daemonSeq = home == null ? "" : home.daemonModuleEventSeq;
                    if (daemonSeq == null || daemonSeq.isEmpty()) {
                        shouldReloadModules = true;
                    } else if (!daemonSeq.equals(lastModuleEventSeq)) {
                        shouldReloadModules = true;
                    }
                }

                final ManagerRepository.ModulesSnapshot snapshot;
                final boolean fromCache;
                if (shouldReloadModules) {
                    snapshot = repo.loadModulesSnapshot(false);
                    fromCache = false;
                } else {
                    snapshot = new ManagerRepository.ModulesSnapshot();
                    snapshot.home = home;
                    snapshot.moduleList = new DsapiCtlClient.CmdResult(0, "module_cache_hit=1");
                    snapshot.modules = cloneModuleRows(cachedModules);
                    fromCache = true;
                }

                runOnUiThread(new Runnable() {
                    @Override
                    public void run() {
                        try {
                            renderSnapshot(snapshot, fromCache);
                            if (!fromCache && snapshot.moduleList.exitCode == 0) {
                                cachedModules.clear();
                                cachedModules.addAll(cloneModuleRows(snapshot.modules));
                                lastModuleEventSeq = snapshot.home == null ? "" : snapshot.home.daemonModuleEventSeq;
                                lastModulesReloadUptimeMs = SystemClock.uptimeMillis();
                            }
                        } finally {
                            refreshInFlight.set(false);
                        }
                    }
                });
            }
        });
    }

    private void renderSnapshot(ManagerRepository.ModulesSnapshot snapshot, boolean fromCache) {
        String runtime = snapshot.home == null ? "<none>" : snapshot.home.runtimeActive;
        String moduleCount = snapshot.home == null
                ? String.valueOf(snapshot.modules.size())
                : snapshot.home.moduleCount;
        String lastError = "none";
        if (snapshot.home != null && !"none".equalsIgnoreCase(snapshot.home.lastErrorState)) {
            lastError = snapshot.home.lastErrorCode + "/" + snapshot.home.lastErrorMessage;
        }

        subtitleView.setText(
                "runtime=" + runtime
                        + "  module_count=" + moduleCount
                        + "  last_error=" + lastError
                        + "  z_scope=" + (snapshot.home == null ? "0" : snapshot.home.zygoteScopeCount)
                        + "  " + (fromCache ? "cache=hit" : "cache=miss")
                        + "\nbridge_service=" + config.bridgeService
                        + "  zygote=" + (snapshot.home == null ? "unknown" : snapshot.home.zygoteState)
                        + "  refresh=" + config.refreshMs + "ms"
        );

        moduleContainer.removeAllViews();
        if (snapshot.moduleList.exitCode != 0) {
            TextView err = UiStyles.makeSectionDesc(this, "module list 加载失败");
            moduleContainer.addView(err);
            if (!cachedModules.isEmpty()) {
                TextView fallback = UiStyles.makeSectionDesc(this, "已展示最近缓存模块列表。");
                moduleContainer.addView(fallback, UiStyles.topMargin(this, 6));
                for (CtlParsers.ModuleRow module : cachedModules) {
                    moduleContainer.addView(buildModuleCard(module));
                }
            }
            return;
        }

        if (snapshot.modules.isEmpty()) {
            TextView empty = UiStyles.makeSectionDesc(this, "暂无模块。可点击“添加 ZIP 模块”或“安装内置”。");
            moduleContainer.addView(empty);
            return;
        }

        for (CtlParsers.ModuleRow module : snapshot.modules) {
            moduleContainer.addView(buildModuleCard(module));
        }
    }

    private List<CtlParsers.ModuleRow> cloneModuleRows(List<CtlParsers.ModuleRow> rows) {
        List<CtlParsers.ModuleRow> out = new ArrayList<CtlParsers.ModuleRow>();
        if (rows == null) {
            return out;
        }
        for (CtlParsers.ModuleRow src : rows) {
            CtlParsers.ModuleRow dst = new CtlParsers.ModuleRow();
            dst.id = src.id;
            dst.name = src.name;
            dst.kind = src.kind;
            dst.version = src.version;
            dst.state = src.state;
            dst.enabled = src.enabled;
            dst.mainCap = src.mainCap;
            dst.actionCount = src.actionCount;
            dst.reason = src.reason;
            dst.actions = cloneActionRows(src.actions);
            out.add(dst);
        }
        return out;
    }

    private boolean hasRunningModules(List<CtlParsers.ModuleRow> rows) {
        if (rows == null || rows.isEmpty()) {
            return false;
        }
        for (CtlParsers.ModuleRow row : rows) {
            if (row == null || row.state == null) {
                continue;
            }
            String s = row.state.toLowerCase(Locale.US);
            if (s.startsWith("running") || s.startsWith("ready") || s.startsWith("ok")) {
                return true;
            }
        }
        return false;
    }

    private void startAutoRefresh() {
        if (autoRefreshScheduled || destroyed) {
            return;
        }
        autoRefreshScheduled = true;
        handler.postDelayed(autoRefreshTask, Math.max(500L, (long) config.refreshMs));
    }

    private void stopAutoRefresh() {
        autoRefreshScheduled = false;
        try {
            handler.removeCallbacks(autoRefreshTask);
        } catch (Throwable ignored) {
        }
    }

    private List<CtlParsers.ModuleActionRow> cloneActionRows(List<CtlParsers.ModuleActionRow> rows) {
        List<CtlParsers.ModuleActionRow> out = new ArrayList<CtlParsers.ModuleActionRow>();
        if (rows == null) {
            return out;
        }
        for (CtlParsers.ModuleActionRow act : rows) {
            CtlParsers.ModuleActionRow item = new CtlParsers.ModuleActionRow();
            item.id = act.id;
            item.name = act.name;
            item.danger = act.danger;
            out.add(item);
        }
        return out;
    }

    private boolean hasActions(CtlParsers.ModuleRow module) {
        if (module == null) {
            return false;
        }
        if (module.actions != null && !module.actions.isEmpty()) {
            return true;
        }
        String count = module.actionCount == null ? "" : module.actionCount.trim();
        if (count.isEmpty()) {
            return false;
        }
        try {
            return Integer.parseInt(count) > 0;
        } catch (NumberFormatException ignored) {
            return !"0".equals(count);
        }
    }

    private void syncCachedModuleActions(String moduleId, List<CtlParsers.ModuleActionRow> actions) {
        if (moduleId == null || moduleId.isEmpty()) {
            return;
        }
        List<CtlParsers.ModuleActionRow> copied = cloneActionRows(actions);
        for (CtlParsers.ModuleRow cached : cachedModules) {
            if (moduleId.equals(cached.id)) {
                cached.actions = cloneActionRows(copied);
                cached.actionCount = String.valueOf(copied.size());
                return;
            }
        }
    }

    private View buildModuleCard(final CtlParsers.ModuleRow module) {
        LinearLayout card = new LinearLayout(this);
        card.setOrientation(LinearLayout.VERTICAL);
        card.setPadding(UiStyles.dp(this, 10), UiStyles.dp(this, 9), UiStyles.dp(this, 10), UiStyles.dp(this, 9));
        card.setBackground(UiStyles.makeRoundedDrawable(this, UiStyles.C_SURFACE_VARIANT, UiStyles.C_OUTLINE, 14, 1));

        LinearLayout.LayoutParams cardLp = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT
        );
        cardLp.bottomMargin = UiStyles.dp(this, 9);
        card.setLayoutParams(cardLp);

        TextView title = new TextView(this);
        title.setText(module.name + "  (" + module.id + ")");
        title.setTextColor(UiStyles.C_TEXT_PRIMARY);
        title.setTextSize(14f);
        title.setTypeface(android.graphics.Typeface.DEFAULT_BOLD);
        card.addView(title);

        LinearLayout chipRow = new LinearLayout(this);
        chipRow.setOrientation(LinearLayout.HORIZONTAL);
        chipRow.addView(UiStyles.makeChip(this, "state: " + module.state, stateColor(module.state)), chipLayout(0));
        chipRow.addView(UiStyles.makeChip(this, "enabled: " + module.enabled, "1".equals(module.enabled) ? UiStyles.C_OK_CONTAINER : UiStyles.C_WARNING_CONTAINER), chipLayout(8));
        chipRow.addView(UiStyles.makeChip(this, "actions: " + module.actionCount, UiStyles.C_SECONDARY_CONTAINER), chipLayout(8));
        card.addView(chipRow, UiStyles.topMargin(this, 6));

        TextView sub = new TextView(this);
        sub.setText(String.format(Locale.US,
                "kind=%s version=%s main_cap=%s reason=%s",
                module.kind,
                module.version,
                module.mainCap,
                module.reason));
        sub.setTextSize(11f);
        sub.setTextColor(UiStyles.C_TEXT_SECONDARY);
        card.addView(sub, UiStyles.topMargin(this, 4));

        final boolean running = isModuleRunning(module);
        final boolean enabled = "1".equals(module.enabled);
        LinearLayout row1 = new LinearLayout(this);
        row1.setOrientation(LinearLayout.HORIZONTAL);
        row1.addView(makeCardButton(running ? "停止模块" : "启动模块", running, new Runnable() {
            @Override
            public void run() {
                if (running) {
                    runCtlAction(new String[]{"module", "stop", module.id}, true, true, "停止模块将中断其正在运行的能力。");
                    return;
                }
                runCtlAction(new String[]{"module", "start", module.id}, true, false, null);
            }
        }), UiStyles.rowWeightLayout(this));
        row1.addView(makeCardButton("操作菜单", false, new Runnable() {
            @Override
            public void run() {
                showModuleOperationsDialog(module);
            }
        }), UiStyles.rowWeightLayout(this));
        card.addView(row1, UiStyles.topMargin(this, 8));

        TextView actionHint = UiStyles.makeSectionDesc(
                this,
                running
                        ? (enabled ? "模块运行中，可在操作菜单执行 Action/禁用/删除。" : "模块运行中但处于禁用状态，请先修复配置。")
                        : (!hasActions(module)
                        ? "模块已停止；可从操作菜单查看详情、启用/禁用或删除。"
                        : "模块已停止；可先启动，也可在操作菜单执行 Action。")
        );
        card.addView(actionHint, UiStyles.topMargin(this, 6));
        return card;
    }

    private void showModuleActionsDialog(final CtlParsers.ModuleRow module) {
        if (module == null) {
            setLogText("module_action_empty id=<unknown>");
            return;
        }
        if (module.actions != null && !module.actions.isEmpty()) {
            showModuleActionsDialogWithRows(module, module.actions);
            return;
        }
        if (!hasActions(module)) {
            setLogText("module_action_empty id=" + module.id);
            return;
        }
        worker.execute(new Runnable() {
            @Override
            public void run() {
                final DsapiCtlClient.CmdResult result = repo.run("module", "action-list", module.id);
                final List<CtlParsers.ModuleActionRow> rows = result.exitCode == 0
                        ? CtlParsers.parseModuleActionRows(result.output)
                        : new ArrayList<CtlParsers.ModuleActionRow>();
                runOnUiThread(new Runnable() {
                    @Override
                    public void run() {
                        if (result.exitCode != 0) {
                            StringBuilder sb = new StringBuilder();
                            sb.append("$ module action-list ").append(module.id);
                            sb.append("\nexit=").append(result.exitCode);
                            if (!result.output.isEmpty()) {
                                sb.append("\n").append(result.output);
                            }
                            setLogText(sb.toString());
                            return;
                        }
                        module.actions = cloneActionRows(rows);
                        module.actionCount = String.valueOf(module.actions.size());
                        syncCachedModuleActions(module.id, module.actions);
                        if (module.actions.isEmpty()) {
                            setLogText("module_action_empty id=" + module.id);
                            return;
                        }
                        showModuleActionsDialogWithRows(module, module.actions);
                    }
                });
            }
        });
    }

    private void showModuleActionsDialogWithRows(
            final CtlParsers.ModuleRow module,
            final List<CtlParsers.ModuleActionRow> actions
    ) {
        if (module == null || actions == null || actions.isEmpty()) {
            setLogText("module_action_empty id=" + (module == null ? "<unknown>" : module.id));
            return;
        }
        final String[] labels = new String[actions.size()];
        for (int i = 0; i < actions.size(); i++) {
            CtlParsers.ModuleActionRow action = actions.get(i);
            labels[i] = action.isDangerous() ? (action.name + "  [危险]") : action.name;
        }
        UiStyles.dialogBuilder(this)
                .setTitle(module.name + " · Action")
                .setItems(labels, (dialog, which) -> {
                    CtlParsers.ModuleActionRow action = actions.get(which);
                    runCtlAction(
                            new String[]{"module", "action-run", module.id, action.id},
                            true,
                            action.isDangerous(),
                            action.isDangerous() ? "该 Action 被标记为危险操作，确认后执行。" : null
                    );
                })
                .setNegativeButton("取消", null)
                .show();
    }

    private void showModuleOperationsDialog(final CtlParsers.ModuleRow module) {
        final List<String> labels = new ArrayList<String>();
        if (hasActions(module)) {
            labels.add("Action");
        }
        labels.add("详情");
        labels.add("重载");
        if ("1".equals(module.enabled)) {
            labels.add("禁用");
        } else {
            labels.add("启用");
        }
        labels.add("删除");
        final String[] items = labels.toArray(new String[0]);
        UiStyles.dialogBuilder(this)
                .setTitle(module.name + " · 操作菜单")
                .setItems(items, (dialog, which) -> {
                    String picked = items[which];
                    if ("Action".equals(picked)) {
                        showModuleActionsDialog(module);
                        return;
                    }
                    if ("详情".equals(picked)) {
                        runCtlAction(new String[]{"module", "detail", module.id}, false, false, null);
                        return;
                    }
                    if ("重载".equals(picked)) {
                        runCtlAction(new String[]{"module", "reload", module.id}, true, false, null);
                        return;
                    }
                    if ("启用".equals(picked)) {
                        runCtlAction(new String[]{"module", "enable", module.id}, true, false, null);
                        return;
                    }
                    if ("禁用".equals(picked)) {
                        runCtlAction(new String[]{"module", "disable", module.id}, true, true, "禁用后模块不会再参与 capability/action 调度。");
                        return;
                    }
                    runCtlAction(new String[]{"module", "remove", module.id}, true, true, "将永久移除模块目录与状态文件。");
                })
                .setNegativeButton("取消", null)
                .show();
    }

    private boolean isModuleRunning(CtlParsers.ModuleRow module) {
        if (module == null || module.state == null) {
            return false;
        }
        String s = module.state.toLowerCase(Locale.US);
        return s.startsWith("running") || s.startsWith("ready") || s.startsWith("ok");
    }

    private int stateColor(String state) {
        if (state == null) {
            return UiStyles.C_SECONDARY_CONTAINER;
        }
        String s = state.toLowerCase(Locale.US);
        if (s.startsWith("running") || s.startsWith("ready") || s.startsWith("ok")) {
            return UiStyles.C_OK_CONTAINER;
        }
        if (s.startsWith("disabled")) {
            return UiStyles.C_WARNING_CONTAINER;
        }
        if (s.startsWith("error") || s.startsWith("missing") || s.startsWith("fail") || s.startsWith("stopped")) {
            return UiStyles.C_ERROR_CONTAINER;
        }
        return UiStyles.C_SECONDARY_CONTAINER;
    }

    private LinearLayout.LayoutParams chipLayout(int leftMarginDp) {
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.WRAP_CONTENT,
                LinearLayout.LayoutParams.WRAP_CONTENT
        );
        lp.leftMargin = UiStyles.dp(this, leftMarginDp);
        return lp;
    }

    private Button makeCardButton(String text, boolean warning, Runnable action) {
        Button b = warning ? UiStyles.makeWarningButton(this, text) : UiStyles.makeTonalButton(this, text);
        b.setTextSize(11f);
        b.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                if (action != null) {
                    action.run();
                }
            }
        });
        return b;
    }

    private void runCtlAction(final String[] args, final boolean refreshAfter, boolean dangerous, String message) {
        if (dangerous) {
            showDangerConfirm("危险操作确认", message == null ? "该操作会改变模块状态。" : message, new Runnable() {
                @Override
                public void run() {
                    runCtlAction(args, refreshAfter, false, null);
                }
            });
            return;
        }

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
                            refreshNow(true);
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
