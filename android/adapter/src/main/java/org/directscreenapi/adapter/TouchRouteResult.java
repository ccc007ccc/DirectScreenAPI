package org.directscreenapi.adapter;

public final class TouchRouteResult {
    public final int decision;
    public final int regionId;

    public TouchRouteResult(int decision, int regionId) {
        this.decision = decision;
        this.regionId = regionId;
    }
}
