package org.directscreenapi.adapter;

public interface DisplayAdapter {
    DisplaySnapshot queryDisplaySnapshot();

    final class DisplaySnapshot {
        public final int width;
        public final int height;
        public final float refreshHz;
        public final int densityDpi;
        public final int rotation;

        public DisplaySnapshot(int width, int height, float refreshHz, int densityDpi, int rotation) {
            this.width = width;
            this.height = height;
            this.refreshHz = refreshHz;
            this.densityDpi = densityDpi;
            this.rotation = rotation;
        }
    }
}
