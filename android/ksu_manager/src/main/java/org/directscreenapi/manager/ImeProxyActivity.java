package org.directscreenapi.manager;

import android.app.Activity;
import android.content.Intent;
import android.graphics.Color;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.text.Editable;
import android.text.InputType;
import android.view.Gravity;
import android.view.KeyEvent;
import android.view.View;
import android.view.WindowManager;
import android.view.inputmethod.BaseInputConnection;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;
import android.view.inputmethod.InputMethodManager;
import android.widget.FrameLayout;

import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

public final class ImeProxyActivity extends Activity {
    private static final String EXTRA_ACTION = "ime_proxy_action";
    private static final String EXTRA_MODE = "ime_proxy_mode";

    private static final String ACTION_OPEN = "open";
    private static final String ACTION_CLOSE = "close";

    private static final String MODE_TEXT = "text";
    private static final String MODE_NUMBER = "number";
    private static final String MODE_PASSWORD = "password";
    private static final String MODE_NUMBER_PASSWORD = "number_password";

    // 与 core/rust/src/api/types.rs 保持一致。
    private static final int KEYBOARD_EVENT_KIND_CHAR = 1;
    private static final int KEYBOARD_EVENT_KIND_BACKSPACE = 2;
    private static final int KEYBOARD_EVENT_KIND_DONE = 3;
    private static final int KEYBOARD_EVENT_KIND_FOCUS_ON = 4;
    private static final int KEYBOARD_EVENT_KIND_FOCUS_OFF = 5;

    private final Handler handler = new Handler(Looper.getMainLooper());
    private final ExecutorService worker = Executors.newSingleThreadExecutor();

    private ManagerConfig config;
    private DsapiCtlClient ctl;
    private ProxyInputView proxyInput;

    private String imeMode = MODE_TEXT;
    private boolean destroyed;
    private boolean focusActive;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        config = ManagerConfig.load(this, getIntent());
        ctl = new DsapiCtlClient(config);

        String action = normalizeAction(getIntent());
        if (ACTION_CLOSE.equals(action)) {
            finish();
            return;
        }

        imeMode = normalizeMode(getIntentMode(getIntent()));
        setContentView(buildContentView());
        applyTinyWindowBounds();
        ensureFocusedAndShowIme();
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        config = ManagerConfig.load(this, intent);
        ctl = new DsapiCtlClient(config);

        String action = normalizeAction(intent);
        if (ACTION_CLOSE.equals(action)) {
            requestClose();
            return;
        }

        String newMode = normalizeMode(getIntentMode(intent));
        if (!newMode.equals(imeMode)) {
            imeMode = newMode;
            if (proxyInput != null) {
                proxyInput.setImeMode(imeMode);
            }
        }
        ensureFocusedAndShowIme();
    }

    @Override
    protected void onResume() {
        super.onResume();
        ensureFocusedAndShowIme();
    }

    @Override
    protected void onDestroy() {
        destroyed = true;
        try {
            handler.removeCallbacksAndMessages(null);
        } catch (Throwable ignored) {
        }
        hideIme();
        if (focusActive) {
            focusActive = false;
            dispatchKeyboardEvent(KEYBOARD_EVENT_KIND_FOCUS_OFF, 0);
        }
        try {
            worker.shutdownNow();
        } catch (Throwable ignored) {
        }
        super.onDestroy();
    }

    @Override
    public void onBackPressed() {
        requestClose();
    }

    private View buildContentView() {
        FrameLayout root = new FrameLayout(this);
        root.setBackgroundColor(Color.TRANSPARENT);

        proxyInput = new ProxyInputView(this);
        proxyInput.setImeMode(imeMode);
        FrameLayout.LayoutParams lp = new FrameLayout.LayoutParams(dp(1), dp(1));
        lp.gravity = Gravity.TOP | Gravity.START;
        root.addView(proxyInput, lp);
        return root;
    }

    private void applyTinyWindowBounds() {
        try {
            WindowManager.LayoutParams lp = getWindow().getAttributes();
            lp.width = dp(1);
            lp.height = dp(1);
            lp.gravity = Gravity.TOP | Gravity.START;
            lp.flags |= WindowManager.LayoutParams.FLAG_NOT_TOUCH_MODAL;
            getWindow().setAttributes(lp);
            getWindow().setLayout(dp(1), dp(1));
        } catch (Throwable ignored) {
        }
    }

    private void ensureFocusedAndShowIme() {
        handler.post(new Runnable() {
            @Override
            public void run() {
                if (destroyed || proxyInput == null) {
                    return;
                }
                proxyInput.requestFocus();
                showIme();
                if (!focusActive) {
                    focusActive = true;
                    dispatchKeyboardEvent(KEYBOARD_EVENT_KIND_FOCUS_ON, 0);
                }
            }
        });
    }

    private void requestClose() {
        if (destroyed) {
            return;
        }
        hideIme();
        finish();
    }

    private void showIme() {
        if (proxyInput == null) {
            return;
        }
        InputMethodManager imm = (InputMethodManager) getSystemService(INPUT_METHOD_SERVICE);
        if (imm == null) {
            return;
        }
        try {
            imm.restartInput(proxyInput);
            boolean shown = imm.showSoftInput(proxyInput, InputMethodManager.SHOW_IMPLICIT);
            if (!shown) {
                imm.showSoftInput(proxyInput, InputMethodManager.SHOW_FORCED);
            }
        } catch (Throwable ignored) {
        }
    }

    private void hideIme() {
        if (proxyInput == null) {
            return;
        }
        InputMethodManager imm = (InputMethodManager) getSystemService(INPUT_METHOD_SERVICE);
        if (imm == null) {
            return;
        }
        try {
            imm.hideSoftInputFromWindow(proxyInput.getWindowToken(), 0);
        } catch (Throwable ignored) {
        }
    }

    private void dispatchCommittedText(CharSequence text) {
        if (text == null || text.length() == 0) {
            return;
        }
        int idx = 0;
        while (idx < text.length()) {
            int cp = Character.codePointAt(text, idx);
            idx += Character.charCount(cp);
            dispatchKeyboardEvent(KEYBOARD_EVENT_KIND_CHAR, cp);
        }
    }

    private void dispatchBackspace(int count) {
        int n = count <= 0 ? 1 : count;
        for (int i = 0; i < n; i++) {
            dispatchKeyboardEvent(KEYBOARD_EVENT_KIND_BACKSPACE, 0);
        }
    }

    private void dispatchDone() {
        dispatchKeyboardEvent(KEYBOARD_EVENT_KIND_DONE, 0);
        requestClose();
    }

    private void dispatchKeyboardEvent(final int kind, final int codepoint) {
        final DsapiCtlClient localCtl = ctl;
        if (localCtl == null) {
            return;
        }
        worker.execute(new Runnable() {
            @Override
            public void run() {
                DsapiCtlClient.CmdResult res = localCtl.run(
                        "cmd",
                        "KEYBOARD_INJECT",
                        String.valueOf(kind),
                        String.valueOf(codepoint)
                );
                if (res.exitCode != 0) {
                    System.out.println("ime_proxy_warn=keyboard_inject_failed kind=" + kind
                            + " codepoint=" + codepoint
                            + " exit=" + res.exitCode
                            + " out=" + sanitizeToken(res.output));
                }
            }
        });
    }

    private static String sanitizeToken(String raw) {
        if (raw == null) {
            return "-";
        }
        String s = raw.trim();
        if (s.isEmpty()) {
            return "-";
        }
        return s.replace('\n', '_').replace('\r', '_').replace('\t', '_').replace(' ', '_');
    }

    private static String getIntentMode(Intent intent) {
        if (intent == null) {
            return MODE_TEXT;
        }
        String mode = intent.getStringExtra(EXTRA_MODE);
        if (mode == null || mode.trim().isEmpty()) {
            mode = intent.getStringExtra("mode");
        }
        if (mode == null || mode.trim().isEmpty()) {
            return MODE_TEXT;
        }
        return mode.trim();
    }

    private static String normalizeAction(Intent intent) {
        if (intent == null) {
            return ACTION_OPEN;
        }
        String action = intent.getStringExtra(EXTRA_ACTION);
        if (action == null || action.trim().isEmpty()) {
            action = intent.getStringExtra("action");
        }
        if (action == null || action.trim().isEmpty()) {
            return ACTION_OPEN;
        }
        action = action.trim().toLowerCase();
        if (ACTION_CLOSE.equals(action)) {
            return ACTION_CLOSE;
        }
        return ACTION_OPEN;
    }

    private static String normalizeMode(String raw) {
        if (raw == null) {
            return MODE_TEXT;
        }
        String mode = raw.trim().toLowerCase();
        if (MODE_NUMBER.equals(mode)
                || MODE_PASSWORD.equals(mode)
                || MODE_NUMBER_PASSWORD.equals(mode)) {
            return mode;
        }
        return MODE_TEXT;
    }

    private static int inputTypeForMode(String mode) {
        if (MODE_NUMBER.equals(mode)) {
            return InputType.TYPE_CLASS_NUMBER;
        }
        if (MODE_PASSWORD.equals(mode)) {
            return InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_PASSWORD;
        }
        if (MODE_NUMBER_PASSWORD.equals(mode)) {
            return InputType.TYPE_CLASS_NUMBER | InputType.TYPE_NUMBER_VARIATION_PASSWORD;
        }
        return InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS;
    }

    private int dp(int value) {
        float density = getResources().getDisplayMetrics().density;
        return Math.round(value * density);
    }

    private final class ProxyInputView extends View {
        private final Editable editable = Editable.Factory.getInstance().newEditable("");
        private int inputType = inputTypeForMode(MODE_TEXT);

        ProxyInputView(Activity activity) {
            super(activity);
            setFocusable(true);
            setFocusableInTouchMode(true);
            setClickable(true);
        }

        void setImeMode(String mode) {
            inputType = inputTypeForMode(mode);
        }

        @Override
        public boolean onCheckIsTextEditor() {
            return true;
        }

        @Override
        public InputConnection onCreateInputConnection(EditorInfo outAttrs) {
            outAttrs.inputType = inputType;
            outAttrs.imeOptions = EditorInfo.IME_ACTION_DONE
                    | EditorInfo.IME_FLAG_NO_EXTRACT_UI
                    | EditorInfo.IME_FLAG_NO_FULLSCREEN;
            outAttrs.initialSelStart = editable.length();
            outAttrs.initialSelEnd = editable.length();
            return new ProxyInputConnection(this, true);
        }

        private final class ProxyInputConnection extends BaseInputConnection {
            ProxyInputConnection(View targetView, boolean fullEditor) {
                super(targetView, fullEditor);
            }

            @Override
            public Editable getEditable() {
                return editable;
            }

            @Override
            public boolean commitText(CharSequence text, int newCursorPosition) {
                dispatchCommittedText(text);
                editable.clear();
                return true;
            }

            @Override
            public boolean setComposingText(CharSequence text, int newCursorPosition) {
                if (text == null) {
                    editable.clear();
                } else {
                    editable.replace(0, editable.length(), text);
                }
                return true;
            }

            @Override
            public boolean finishComposingText() {
                editable.clear();
                return true;
            }

            @Override
            public boolean deleteSurroundingText(int beforeLength, int afterLength) {
                int n = beforeLength <= 0 ? 1 : beforeLength;
                dispatchBackspace(n);
                editable.clear();
                return true;
            }

            @Override
            public boolean sendKeyEvent(KeyEvent event) {
                if (event != null && event.getAction() == KeyEvent.ACTION_DOWN) {
                    if (event.getKeyCode() == KeyEvent.KEYCODE_DEL) {
                        dispatchBackspace(1);
                        return true;
                    }
                    if (event.getKeyCode() == KeyEvent.KEYCODE_ENTER) {
                        dispatchDone();
                        return true;
                    }
                }
                return true;
            }

            @Override
            public boolean performEditorAction(int actionCode) {
                if (actionCode == EditorInfo.IME_ACTION_DONE
                        || actionCode == EditorInfo.IME_ACTION_GO
                        || actionCode == EditorInfo.IME_ACTION_SEND
                        || actionCode == EditorInfo.IME_ACTION_NEXT
                        || actionCode == EditorInfo.IME_ACTION_UNSPECIFIED) {
                    dispatchDone();
                    return true;
                }
                return true;
            }
        }
    }
}
