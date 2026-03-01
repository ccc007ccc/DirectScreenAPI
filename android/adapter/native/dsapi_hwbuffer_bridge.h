#ifndef DSAPI_HWBUFFER_BRIDGE_H
#define DSAPI_HWBUFFER_BRIDGE_H

#include <jni.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct dsapi_jni_hwbuffer_frame {
    int32_t fd;
    uint64_t frame_seq;
    uint32_t width;
    uint32_t height;
    uint32_t stride;
    uint32_t format;
    uint64_t usage;
    uint32_t byte_offset;
    uint32_t byte_len;
} dsapi_jni_hwbuffer_frame_t;

/*
 * Exports a dma-buf fd from a java.nio.HardwareBuffer object.
 * Returns 0 on success, negative value on failure.
 */
int dsapi_jni_export_hwbuffer_frame(
    JNIEnv* env,
    jobject hardware_buffer_obj,
    uint64_t frame_seq,
    dsapi_jni_hwbuffer_frame_t* out_frame
);

void dsapi_jni_release_hwbuffer_frame(dsapi_jni_hwbuffer_frame_t* frame);

#ifdef __cplusplus
}
#endif

#endif
