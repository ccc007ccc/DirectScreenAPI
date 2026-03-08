package org.directscreenapi.adapter;

import android.content.Context;
import android.graphics.Color;
import android.graphics.PixelFormat;
import android.os.Handler;
import android.os.Looper;
import android.util.DisplayMetrics;
import android.view.Gravity;
import android.view.View;
import android.view.WindowManager;
import android.widget.Button;
import android.widget.HorizontalScrollView;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;

import java.io.BufferedReader;
import java.io.IOException;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import java.util.Locale;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.atomic.AtomicBoolean;

final class CapabilityManagerUi {
    private static final String TAG_PREFIX = "capability_row=";

    private final String ctlPath;
    private final int refreshMs;
    private final ExecutorService worker;
    private final AtomicBoolean refreshInFlight;

    private Handler mainHandler;
    private Context context;
    private WindowManager windowManager;

    private View rootView;
    private TextView headerText;
    private TextView detailText;
    private LinearLayout listContainer;

    private volatile boolean closed;

    CapabilityManagerUi(String ctlPath, int refreshMs) {
        this.ctlPath = ctlPath;
        this.refreshMs = Math.max(300, refreshMs);
        this.worker = Executors.newSingleThreadExecutor();
        this.refreshInFlight = new AtomicBoolean(false);
    }

    void runLoop() throws Exception {
        if (Looper.getMainLooper() == null) {
            Looper.prepareMainLooper();
        } else if (Looper.myLooper() == null) {
            Looper.prepare();
        }
        Looper looper = Looper.myLooper();
        if (looper == null) {
            throw new IllegalStateException("looper_unavailable");
        }

        context = resolveContext();
        mainHandler = new Handler(looper);

        Object wmObj = context.getSystemService(Context.WINDOW_SERVICE);
        if (!(wmObj instanceof WindowManager)) {
            throw new IllegalStateException("window_manager_unavailable");
        }
        windowManager = (WindowManager) wmObj;

        buildUi();
        addWindowWithFallback();

        mainHandler.post(this::onRefreshTick);
        Looper.loop();
    }

    private void onRefreshTick() {
        if (closed) {
            return;
        }
        scheduleRefreshNow();
        mainHandler.postDelayed(this::onRefreshTick, refreshMs);
    }

    private Context resolveContext() throws Exception {
        try {
            Class<?> activityThreadClass = Class.forName("android.app.ActivityThread");
            Object app = ReflectBridge.invokeStatic(activityThreadClass, "currentApplication");
            if (app instanceof Context) {
                return (Context) app;
            }
        } catch (Throwable ignored) {
        }

        Class<?> activityThreadClass = Class.forName("android.app.ActivityThread");
        Object thread = ReflectBridge.invokeStatic(activityThreadClass, "systemMain");
        Object systemContext = ReflectBridge.invoke(thread, "getSystemContext");
        if (systemContext instanceof Context) {
            return (Context) systemContext;
        }
        throw new IllegalStateException("context_unavailable");
    }

    private void buildUi() {
        int pad = dp(12);

        LinearLayout panel = new LinearLayout(context);
        panel.setOrientation(LinearLayout.VERTICAL);
        panel.setPadding(pad, pad, pad, pad);
        panel.setBackgroundColor(Color.argb(235, 18, 24, 32));

        headerText = new TextView(context);
        headerText.setTextColor(Color.WHITE);
        headerText.setTextSize(16f);
        headerText.setText("DSAPI Capability Manager");
        panel.addView(headerText, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT
        ));

        LinearLayout topRow = new LinearLayout(context);
        topRow.setOrientation(LinearLayout.HORIZONTAL);
        topRow.setGravity(Gravity.END);
        topRow.addView(makeTopButton("刷新", this::scheduleRefreshNow));
        topRow.addView(makeTopButton("关闭", this::closeUi));
        panel.addView(topRow, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT
        ));

        ScrollView scrollView = new ScrollView(context);
        listContainer = new LinearLayout(context);
        listContainer.setOrientation(LinearLayout.VERTICAL);
        scrollView.addView(listContainer, new ScrollView.LayoutParams(
                ScrollView.LayoutParams.MATCH_PARENT,
                ScrollView.LayoutParams.WRAP_CONTENT
        ));

        LinearLayout.LayoutParams listLp = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                0,
                1f
        );
        listLp.topMargin = dp(8);
        panel.addView(scrollView, listLp);

        TextView detailTitle = new TextView(context);
        detailTitle.setText("详情输出");
        detailTitle.setTextColor(Color.rgb(180, 210, 255));
        detailTitle.setTextSize(13f);
        panel.addView(detailTitle, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT
        ));

        HorizontalScrollView detailScroll = new HorizontalScrollView(context);
        detailText = new TextView(context);
        detailText.setTextColor(Color.rgb(220, 230, 240));
        detailText.setTextSize(12f);
        detailText.setText("等待数据...");
        detailScroll.addView(detailText, new HorizontalScrollView.LayoutParams(
                HorizontalScrollView.LayoutParams.WRAP_CONTENT,
                HorizontalScrollView.LayoutParams.WRAP_CONTENT
        ));

        LinearLayout.LayoutParams detailLp = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                dp(120)
        );
        detailLp.topMargin = dp(6);
        panel.addView(detailScroll, detailLp);

        rootView = panel;
    }

    private View makeTopButton(String label, Runnable action) {
        Button button = new Button(context);
        button.setText(label);
        button.setAllCaps(false);
        button.setOnClickListener(v -> action.run());
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.WRAP_CONTENT,
                LinearLayout.LayoutParams.WRAP_CONTENT
        );
        lp.leftMargin = dp(8);
        button.setLayoutParams(lp);
        return button;
    }

    private void addWindowWithFallback() {
        DisplayMetrics dm = context.getResources().getDisplayMetrics();
        int width = Math.max(dp(420), Math.round(dm.widthPixels * 0.92f));
        int height = Math.max(dp(560), Math.round(dm.heightPixels * 0.86f));

        int[] types = new int[]{
                WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY,
                WindowManager.LayoutParams.TYPE_SYSTEM_ALERT,
                WindowManager.LayoutParams.TYPE_PHONE,
                WindowManager.LayoutParams.TYPE_SYSTEM_ERROR
        };

        Throwable last = null;
        for (int type : types) {
            WindowManager.LayoutParams lp = new WindowManager.LayoutParams(
                    width,
                    height,
                    type,
                    WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN,
                    PixelFormat.TRANSLUCENT
            );
            lp.gravity = Gravity.CENTER;
            lp.setTitle("DSAPI Capability Manager");
            try {
                windowManager.addView(rootView, lp);
                return;
            } catch (Throwable t) {
                last = t;
            }
        }

        if (last instanceof RuntimeException) {
            throw (RuntimeException) last;
        }
        throw new RuntimeException("window_add_failed", last);
    }

    private int dp(int value) {
        float density = context.getResources().getDisplayMetrics().density;
        return Math.round(value * density);
    }

    private void scheduleRefreshNow() {
        if (!refreshInFlight.compareAndSet(false, true)) {
            return;
        }
        worker.execute(() -> {
            final Snapshot snapshot = loadSnapshot();
            mainHandler.post(() -> {
                try {
                    renderSnapshot(snapshot);
                } finally {
                    refreshInFlight.set(false);
                }
            });
        });
    }

    private Snapshot loadSnapshot() {
        Snapshot snapshot = new Snapshot();

        CommandResult statusRes = runCtl("status");
        snapshot.statusLines = splitLines(statusRes.output);

        CommandResult listRes = runCtl("capability", "list");
        snapshot.entries = parseEntries(listRes.output);

        snapshot.errorHint = "";
        if (statusRes.exitCode != 0) {
            snapshot.errorHint = "status_exit=" + statusRes.exitCode;
        }
        if (listRes.exitCode != 0) {
            if (!snapshot.errorHint.isEmpty()) {
                snapshot.errorHint = snapshot.errorHint + " ";
            }
            snapshot.errorHint = snapshot.errorHint + "list_exit=" + listRes.exitCode;
        }
        return snapshot;
    }

    private void renderSnapshot(Snapshot snapshot) {
        StringBuilder sb = new StringBuilder();
        sb.append("DSAPI Capability Manager");
        if (!snapshot.errorHint.isEmpty()) {
            sb.append(" [").append(snapshot.errorHint).append("]");
        }
        for (String line : snapshot.statusLines) {
            if (line.startsWith("ksu_dsapi_status=") || line.startsWith("runtime_active=") || line.startsWith("enabled=")) {
                sb.append('\n').append(line);
            }
        }
        headerText.setText(sb.toString());

        listContainer.removeAllViews();
        if (snapshot.entries.isEmpty()) {
            TextView empty = new TextView(context);
            empty.setText("暂无 capability（请检查 runtime/capability 定义）");
            empty.setTextColor(Color.LTGRAY);
            listContainer.addView(empty);
            return;
        }

        for (CapabilityEntry entry : snapshot.entries) {
            listContainer.addView(buildCapabilityCard(entry));
        }
    }

    private View buildCapabilityCard(CapabilityEntry entry) {
        LinearLayout card = new LinearLayout(context);
        card.setOrientation(LinearLayout.VERTICAL);
        card.setPadding(dp(10), dp(8), dp(10), dp(8));
        card.setBackgroundColor(Color.argb(210, 32, 42, 56));

        LinearLayout.LayoutParams cardLp = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT
        );
        cardLp.bottomMargin = dp(8);
        card.setLayoutParams(cardLp);

        TextView title = new TextView(context);
        title.setText(String.format(Locale.US, "%s (%s)", entry.name, entry.id));
        title.setTextColor(Color.WHITE);
        title.setTextSize(14f);
        card.addView(title);

        TextView state = new TextView(context);
        state.setText(String.format(
                Locale.US,
                "state=%s pid=%s reason=%s kind=%s source=%s",
                entry.state,
                entry.pid,
                entry.reason,
                entry.kind,
                entry.source
        ));
        state.setTextColor(Color.rgb(210, 220, 230));
        state.setTextSize(12f);
        card.addView(state);

        LinearLayout row = new LinearLayout(context);
        row.setOrientation(LinearLayout.HORIZONTAL);
        row.setGravity(Gravity.START);
        row.addView(makeActionButton("启用", () -> runCapabilityAction("start", entry.id)));
        row.addView(makeActionButton("停用", () -> runCapabilityAction("stop", entry.id)));
        row.addView(makeActionButton("删除", () -> runCapabilityAction("remove", entry.id)));
        row.addView(makeActionButton("恢复", () -> runCapabilityAction("enable", entry.id)));
        row.addView(makeActionButton("详情", () -> showCapabilityDetail(entry.id)));
        card.addView(row);

        return card;
    }

    private View makeActionButton(String label, Runnable action) {
        Button b = new Button(context);
        b.setText(label);
        b.setAllCaps(false);
        b.setTextSize(11f);
        b.setOnClickListener(v -> action.run());
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(
                0,
                LinearLayout.LayoutParams.WRAP_CONTENT,
                1f
        );
        lp.rightMargin = dp(6);
        b.setLayoutParams(lp);
        return b;
    }

    private void runCapabilityAction(String action, String capId) {
        worker.execute(() -> {
            CommandResult res = runCtl("capability", action, capId);
            final String detail = "action=" + action + " id=" + capId + " exit=" + res.exitCode + "\n" + res.output;
            mainHandler.post(() -> {
                detailText.setText(detail);
                scheduleRefreshNow();
            });
        });
    }

    private void showCapabilityDetail(String capId) {
        worker.execute(() -> {
            CommandResult res = runCtl("capability", "detail", capId);
            final String text = "detail id=" + capId + " exit=" + res.exitCode + "\n" + res.output;
            mainHandler.post(() -> detailText.setText(text));
        });
    }

    private void closeUi() {
        if (closed) {
            return;
        }
        closed = true;
        try {
            mainHandler.removeCallbacksAndMessages(null);
        } catch (Throwable ignored) {
        }
        try {
            worker.shutdownNow();
        } catch (Throwable ignored) {
        }
        try {
            windowManager.removeViewImmediate(rootView);
        } catch (Throwable ignored) {
        }
        System.exit(0);
    }

    private CommandResult runCtl(String... ctlArgs) {
        List<String> cmd = new ArrayList<>();
        cmd.add("/system/bin/sh");
        cmd.add(ctlPath);
        cmd.addAll(Arrays.asList(ctlArgs));

        Process process = null;
        try {
            process = new ProcessBuilder(cmd).redirectErrorStream(true).start();
            String output = readAll(process.getInputStream());
            int code = process.waitFor();
            return new CommandResult(code, output);
        } catch (Throwable t) {
            return new CommandResult(255, "command_error=" + t.getClass().getName() + ":" + t.getMessage());
        } finally {
            if (process != null) {
                try {
                    process.destroy();
                } catch (Throwable ignored) {
                }
            }
        }
    }

    private static String readAll(InputStream in) throws IOException {
        StringBuilder sb = new StringBuilder();
        BufferedReader br = new BufferedReader(new InputStreamReader(in, StandardCharsets.UTF_8));
        String line;
        while ((line = br.readLine()) != null) {
            if (sb.length() > 0) {
                sb.append('\n');
            }
            sb.append(line);
        }
        return sb.toString();
    }

    private static List<String> splitLines(String text) {
        List<String> lines = new ArrayList<>();
        if (text == null || text.isEmpty()) {
            return lines;
        }
        for (String line : text.split("\\r?\\n")) {
            if (!line.isEmpty()) {
                lines.add(line);
            }
        }
        return lines;
    }

    private static List<CapabilityEntry> parseEntries(String output) {
        List<CapabilityEntry> list = new ArrayList<>();
        if (output == null || output.isEmpty()) {
            return list;
        }

        String[] lines = output.split("\\r?\\n");
        for (String line : lines) {
            if (!line.startsWith(TAG_PREFIX)) {
                continue;
            }
            String raw = line.substring(TAG_PREFIX.length());
            String[] parts = raw.split("\\|", -1);
            if (parts.length < 7) {
                continue;
            }
            CapabilityEntry e = new CapabilityEntry();
            e.id = parts[0];
            e.name = parts[1];
            e.kind = parts[2];
            e.source = parts[3];
            e.state = parts[4];
            e.pid = parts[5];
            e.reason = parts[6];
            list.add(e);
        }
        return list;
    }

    private static final class Snapshot {
        String errorHint;
        List<String> statusLines = new ArrayList<>();
        List<CapabilityEntry> entries = new ArrayList<>();
    }

    private static final class CapabilityEntry {
        String id;
        String name;
        String kind;
        String source;
        String state;
        String pid;
        String reason;
    }

    private static final class CommandResult {
        final int exitCode;
        final String output;

        CommandResult(int exitCode, String output) {
            this.exitCode = exitCode;
            this.output = output == null ? "" : output;
        }
    }
}
