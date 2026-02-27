package org.directscreenapi.adapter;

public final class TouchEvent {
    public final int pointerId;
    public final float x;
    public final float y;

    public TouchEvent(int pointerId, float x, float y) {
        this.pointerId = pointerId;
        this.x = x;
        this.y = y;
    }
}
