package org.directscreenapi.adapter;

public interface InputAdapter {
    TouchRouteResult onTouchDown(TouchEvent event);
    TouchRouteResult onTouchMove(TouchEvent event);
    TouchRouteResult onTouchUp(TouchEvent event);
    TouchRouteResult onTouchCancel(int pointerId);
    void clearTouches();
    int activeTouchCount();
}
