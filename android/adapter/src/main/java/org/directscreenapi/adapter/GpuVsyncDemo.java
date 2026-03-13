package org.directscreenapi.adapter;

/**
 * 实验入口门面：
 * 保持主命令行入口稳定，具体实现下沉到 experimental 目录。
 */
final class GpuVsyncDemo {
    private final GpuVsyncDemoExperiment delegate;

    GpuVsyncDemo(int requestedWidth, int requestedHeight, int zLayer, String layerName, float runSeconds) {
        this.delegate = new GpuVsyncDemoExperiment(
                requestedWidth,
                requestedHeight,
                zLayer,
                layerName,
                runSeconds
        );
    }

    void runLoop() throws Exception {
        delegate.runLoop();
    }
}
