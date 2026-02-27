package org.directscreenapi.adapter;

public final class AdapterBootstrap {
    private final DisplayAdapter displayAdapter;
    private final InputAdapter inputAdapter;
    private final RenderAdapter renderAdapter;

    public AdapterBootstrap(DisplayAdapter displayAdapter, InputAdapter inputAdapter, RenderAdapter renderAdapter) {
        this.displayAdapter = displayAdapter;
        this.inputAdapter = inputAdapter;
        this.renderAdapter = renderAdapter;
    }

    public void initialize() {
        DisplayAdapter.DisplaySnapshot snapshot = displayAdapter.queryDisplaySnapshot();
        renderAdapter.onDisplayChanged(snapshot);
    }

    public TouchRouteResult onTouchDown(TouchEvent event) {
        return inputAdapter.onTouchDown(event);
    }

    public TouchRouteResult onTouchMove(TouchEvent event) {
        return inputAdapter.onTouchMove(event);
    }

    public TouchRouteResult onTouchUp(TouchEvent event) {
        return inputAdapter.onTouchUp(event);
    }

    public TouchRouteResult onTouchCancel(int pointerId) {
        return inputAdapter.onTouchCancel(pointerId);
    }

    public int activeTouchCount() {
        return inputAdapter.activeTouchCount();
    }
}
