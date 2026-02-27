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

    public InputAdapter.RouteDecision routePoint(float x, float y) {
        return inputAdapter.routePoint(x, y);
    }
}
