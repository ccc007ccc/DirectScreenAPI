package org.directscreenapi.adapter;

import java.util.Locale;

public final class AndroidAdapterMain {
    private static int parseInt(String s, int fallback) {
        try {
            return Integer.parseInt(s);
        } catch (Throwable ignored) {
            return fallback;
        }
    }

    private static void usage() {
        System.out.println("usage:");
        System.out.println("  AndroidAdapterMain display-kv");
        System.out.println("  AndroidAdapterMain display-line");
        System.out.println("  AndroidAdapterMain present-loop [socket_path] [poll_ms] [z_layer] [layer_name]");
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
        if ("present-loop".equals(cmd)) {
            String socketPath = args.length > 1 ? args[1] : "artifacts/run/dsapi.sock";
            int pollMs = args.length > 2 ? parseInt(args[2], 2) : 2;
            int zLayer = args.length > 3 ? parseInt(args[3], 1_000_000) : 1_000_000;
            String layerName = args.length > 4 ? args[4] : "DirectScreenAPI";
            try {
                RgbaFramePresenter presenter = new RgbaFramePresenter(socketPath, pollMs, zLayer, layerName);
                presenter.runLoop();
                return;
            } catch (Throwable t) {
                System.out.println("presenter_status=failed");
                System.out.println("presenter_error=" + t.getClass().getName() + ":" + t.getMessage());
                t.printStackTrace(System.out);
                System.exit(2);
            }
        }

        usage();
        System.exit(1);
    }
}
