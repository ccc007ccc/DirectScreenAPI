package org.directscreenapi.adapter;

import java.util.Locale;

public final class AndroidAdapterMain {
    private static void usage() {
        System.out.println("usage:");
        System.out.println("  AndroidAdapterMain display-kv");
        System.out.println("  AndroidAdapterMain display-line");
    }

    private static void printDisplayKv(DisplayAdapter.DisplaySnapshot s) {
        System.out.println("width=" + s.width);
        System.out.println("height=" + s.height);
        System.out.println(String.format(Locale.US, "refresh_hz=%.2f", s.refreshHz));
        System.out.println("density_dpi=" + s.densityDpi);
        System.out.println("rotation=" + s.rotation);
    }

    private static void printDisplayLine(DisplayAdapter.DisplaySnapshot s) {
        System.out.println(String.format(
                Locale.US,
                "display_snapshot width=%d height=%d refresh_hz=%.2f density_dpi=%d rotation=%d",
                s.width,
                s.height,
                s.refreshHz,
                s.densityDpi,
                s.rotation
        ));
    }

    public static void main(String[] args) {
        if (args.length < 1) {
            usage();
            System.exit(1);
        }

        String cmd = args[0];
        DisplayAdapter adapter = new AndroidDisplayAdapter();
        DisplayAdapter.DisplaySnapshot snapshot = adapter.queryDisplaySnapshot();

        if ("display-kv".equals(cmd)) {
            printDisplayKv(snapshot);
            return;
        }
        if ("display-line".equals(cmd)) {
            printDisplayLine(snapshot);
            return;
        }

        usage();
        System.exit(1);
    }
}
