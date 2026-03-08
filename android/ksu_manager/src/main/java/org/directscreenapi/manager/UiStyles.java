package org.directscreenapi.manager;

import android.app.AlertDialog;
import android.content.Context;
import android.graphics.Color;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
import android.view.Gravity;
import android.widget.Button;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.TextView;

final class UiStyles {
    static final int C_BG = Color.rgb(243, 247, 252);
    static final int C_BG_GRADIENT_END = Color.rgb(232, 240, 249);
    static final int C_SURFACE = Color.WHITE;
    static final int C_SURFACE_VARIANT = Color.rgb(232, 241, 249);
    static final int C_PRIMARY = Color.rgb(0, 122, 138);
    static final int C_PRIMARY_DARK = Color.rgb(0, 97, 112);
    static final int C_PRIMARY_CONTAINER = Color.rgb(205, 240, 245);
    static final int C_SECONDARY = Color.rgb(63, 86, 104);
    static final int C_SECONDARY_CONTAINER = Color.rgb(223, 235, 244);
    static final int C_ERROR_CONTAINER = Color.rgb(255, 226, 226);
    static final int C_OK_CONTAINER = Color.rgb(210, 244, 221);
    static final int C_WARNING_CONTAINER = Color.rgb(255, 236, 202);
    static final int C_OUTLINE = Color.rgb(199, 214, 227);
    static final int C_TEXT_PRIMARY = Color.rgb(20, 35, 49);
    static final int C_TEXT_SECONDARY = Color.rgb(78, 98, 116);
    static final int C_ON_PRIMARY = Color.WHITE;

    private UiStyles() {
    }

    static AlertDialog.Builder dialogBuilder(Context context) {
        return new AlertDialog.Builder(context, android.R.style.Theme_DeviceDefault_Light_Dialog_Alert);
    }

    static int dp(Context context, int value) {
        float density = context.getResources().getDisplayMetrics().density;
        return Math.round(value * density);
    }

    static LinearLayout makeCard(Context context) {
        LinearLayout card = new LinearLayout(context);
        card.setOrientation(LinearLayout.VERTICAL);
        card.setPadding(dp(context, 14), dp(context, 12), dp(context, 14), dp(context, 12));
        card.setBackground(makeRoundedDrawable(context, C_SURFACE, C_OUTLINE, 16, 1));
        card.setElevation(dp(context, 1));
        return card;
    }

    static LinearLayout.LayoutParams cardLayout(Context context) {
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT
        );
        lp.topMargin = dp(context, 10);
        return lp;
    }

    static GradientDrawable makeRoundedDrawable(Context context, int fill, int stroke, int radiusDp, int strokeDp) {
        GradientDrawable d = new GradientDrawable();
        d.setShape(GradientDrawable.RECTANGLE);
        d.setCornerRadius(dp(context, radiusDp));
        d.setColor(fill);
        if (strokeDp > 0) {
            d.setStroke(dp(context, strokeDp), stroke);
        }
        return d;
    }

    static GradientDrawable makeRoundedGradientDrawable(
            Context context,
            int startColor,
            int endColor,
            int stroke,
            int radiusDp,
            int strokeDp
    ) {
        GradientDrawable d = new GradientDrawable(
                GradientDrawable.Orientation.TL_BR,
                new int[]{startColor, endColor}
        );
        d.setShape(GradientDrawable.RECTANGLE);
        d.setCornerRadius(dp(context, radiusDp));
        if (strokeDp > 0) {
            d.setStroke(dp(context, strokeDp), stroke);
        }
        return d;
    }

    static GradientDrawable makeAppBackground() {
        return new GradientDrawable(
                GradientDrawable.Orientation.TOP_BOTTOM,
                new int[]{C_BG, C_BG_GRADIENT_END}
        );
    }

    static TextView makeTitle(Context context, String text) {
        TextView t = new TextView(context);
        t.setText(text);
        t.setTextSize(23f);
        t.setTextColor(C_TEXT_PRIMARY);
        t.setTypeface(Typeface.create("sans-serif-medium", Typeface.NORMAL));
        return t;
    }

    static TextView makeSubTitle(Context context, String text) {
        TextView t = new TextView(context);
        t.setText(text);
        t.setTextSize(12f);
        t.setTextColor(C_TEXT_SECONDARY);
        return t;
    }

    static TextView makeSectionTitle(Context context, String text) {
        TextView t = new TextView(context);
        t.setText(text);
        t.setTextSize(16f);
        t.setTextColor(C_TEXT_PRIMARY);
        t.setTypeface(Typeface.create("sans-serif-medium", Typeface.NORMAL));
        return t;
    }

    static TextView makeSectionDesc(Context context, String text) {
        TextView t = new TextView(context);
        t.setText(text);
        t.setTextSize(12f);
        t.setTextColor(C_TEXT_SECONDARY);
        return t;
    }

    static TextView makeChip(Context context, String text, int bgColor) {
        TextView chip = new TextView(context);
        chip.setText(text);
        chip.setTextSize(11f);
        chip.setTextColor(C_TEXT_PRIMARY);
        chip.setTypeface(Typeface.DEFAULT_BOLD);
        chip.setPadding(dp(context, 10), dp(context, 6), dp(context, 10), dp(context, 6));
        chip.setBackground(makeRoundedDrawable(context, bgColor, Color.TRANSPARENT, 999, 0));
        return chip;
    }

    static Button makeFilledButton(Context context, String text) {
        Button b = new Button(context);
        b.setAllCaps(false);
        b.setText(text);
        b.setTextColor(C_ON_PRIMARY);
        b.setTextSize(12.5f);
        b.setTypeface(Typeface.create("sans-serif-medium", Typeface.NORMAL));
        b.setMinHeight(dp(context, 42));
        b.setBackground(makeRoundedGradientDrawable(context, C_PRIMARY, C_PRIMARY_DARK, C_PRIMARY_DARK, 12, 1));
        return b;
    }

    static Button makeTonalButton(Context context, String text) {
        Button b = new Button(context);
        b.setAllCaps(false);
        b.setText(text);
        b.setTextColor(C_TEXT_PRIMARY);
        b.setTextSize(12.5f);
        b.setMinHeight(dp(context, 42));
        b.setBackground(makeRoundedDrawable(context, C_PRIMARY_CONTAINER, C_OUTLINE, 12, 1));
        return b;
    }

    static Button makeWarningButton(Context context, String text) {
        Button b = new Button(context);
        b.setAllCaps(false);
        b.setText(text);
        b.setTextColor(C_TEXT_PRIMARY);
        b.setTextSize(12.5f);
        b.setMinHeight(dp(context, 42));
        b.setBackground(makeRoundedDrawable(context, C_WARNING_CONTAINER, C_OUTLINE, 12, 1));
        return b;
    }

    static EditText styleEditText(Context context, EditText editText) {
        editText.setTextColor(C_TEXT_PRIMARY);
        editText.setTextSize(13f);
        editText.setSingleLine(true);
        editText.setPadding(dp(context, 10), dp(context, 8), dp(context, 10), dp(context, 8));
        editText.setBackground(makeRoundedDrawable(context, Color.WHITE, C_OUTLINE, 10, 1));
        return editText;
    }

    static TextView makeLogText(Context context) {
        TextView tv = new TextView(context);
        tv.setTextSize(12f);
        tv.setTextColor(C_TEXT_PRIMARY);
        tv.setTypeface(Typeface.MONOSPACE);
        tv.setTextIsSelectable(true);
        tv.setPadding(dp(context, 10), dp(context, 8), dp(context, 10), dp(context, 8));
        tv.setBackground(makeRoundedDrawable(context, Color.rgb(248, 251, 255), C_OUTLINE, 12, 1));
        return tv;
    }

    static LinearLayout.LayoutParams rowWeightLayout(Context context) {
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(
                0,
                LinearLayout.LayoutParams.WRAP_CONTENT,
                1f
        );
        lp.rightMargin = dp(context, 6);
        return lp;
    }

    static Button makeNavButton(Context context, String text, boolean selected) {
        Button b = selected ? makeFilledButton(context, text) : makeTonalButton(context, text);
        if (selected) {
            b.setTextSize(12f);
        }
        return b;
    }

    static LinearLayout buildPageRoot(Context context) {
        LinearLayout root = new LinearLayout(context);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setPadding(dp(context, 12), dp(context, 12), dp(context, 12), dp(context, 12));
        root.setBackgroundColor(Color.TRANSPARENT);
        return root;
    }

    static void setCardHeaderSpacing(Context context, TextView sub) {
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT
        );
        lp.topMargin = dp(context, 4);
        sub.setLayoutParams(lp);
    }

    static LinearLayout.LayoutParams topMargin(Context context, int dpValue) {
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT
        );
        lp.topMargin = dp(context, dpValue);
        return lp;
    }

    static LinearLayout.LayoutParams gravityEndWrap(Context context, int topMarginDp) {
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.WRAP_CONTENT,
                LinearLayout.LayoutParams.WRAP_CONTENT
        );
        lp.topMargin = dp(context, topMarginDp);
        lp.gravity = Gravity.END;
        return lp;
    }

    static final class HeaderBar {
        final LinearLayout root;
        final TextView subtitle;

        HeaderBar(LinearLayout root, TextView subtitle) {
            this.root = root;
            this.subtitle = subtitle;
        }
    }

    static HeaderBar makeHeaderBar(Context context, String title, String subtitleText) {
        LinearLayout card = makeCard(context);
        card.setPadding(dp(context, 14), dp(context, 12), dp(context, 14), dp(context, 10));
        card.setBackground(makeRoundedGradientDrawable(
                context,
                Color.rgb(247, 252, 255),
                Color.rgb(235, 245, 252),
                C_OUTLINE,
                16,
                1
        ));

        TextView badge = makeSectionDesc(context, "KernelSU · DirectScreenAPI");
        badge.setTextSize(11f);
        badge.setTypeface(Typeface.create("sans-serif-medium", Typeface.NORMAL));
        badge.setTextColor(C_SECONDARY);
        badge.setPadding(dp(context, 8), dp(context, 4), dp(context, 8), dp(context, 4));
        badge.setBackground(makeRoundedDrawable(context, C_PRIMARY_CONTAINER, Color.TRANSPARENT, 999, 0));
        card.addView(badge);

        TextView titleView = makeTitle(context, title);
        card.addView(titleView, topMargin(context, 8));

        TextView subtitle = makeSubTitle(context, subtitleText);
        setCardHeaderSpacing(context, subtitle);
        card.addView(subtitle);
        return new HeaderBar(card, subtitle);
    }
}
