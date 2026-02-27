package org.directscreenapi.adapter;

public interface InputAdapter {
    RouteDecision routePoint(float x, float y);

    enum RouteDecision {
        PASS,
        BLOCK
    }
}
