#include "dsapi_hwbuffer_bridge.h"

#include <android/hardware_buffer.h>
#include <android/hardware_buffer_jni.h>
#include <android/native_handle.h>
#include <fcntl.h>
#include <stddef.h>
#include <string.h>
#include <unistd.h>

#ifndef AHARDWAREBUFFER_FORMAT_R8G8B8A8_UNORM
#define AHARDWAREBUFFER_FORMAT_R8G8B8A8_UNORM 1
#endif

#define DSAPI_HWBUF_ERR_NULL (-1)
#define DSAPI_HWBUF_ERR_FORMAT (-2)
#define DSAPI_HWBUF_ERR_HANDLE (-3)
#define DSAPI_HWBUF_ERR_FD_DUP (-4)
#define DSAPI_HWBUF_ERR_SIZE (-5)

int dsapi_jni_export_hwbuffer_frame(
    JNIEnv* env,
    jobject hardware_buffer_obj,
    uint64_t frame_seq,
    dsapi_jni_hwbuffer_frame_t* out_frame
) {
    if (env == NULL || hardware_buffer_obj == NULL || out_frame == NULL) {
        return DSAPI_HWBUF_ERR_NULL;
    }

    memset(out_frame, 0, sizeof(*out_frame));
    out_frame->fd = -1;

    AHardwareBuffer* buffer = AHardwareBuffer_fromHardwareBuffer(env, hardware_buffer_obj);
    if (buffer == NULL) {
        return DSAPI_HWBUF_ERR_HANDLE;
    }

    AHardwareBuffer_acquire(buffer);

    AHardwareBuffer_Desc desc;
    memset(&desc, 0, sizeof(desc));
    AHardwareBuffer_describe(buffer, &desc);

    if (desc.width == 0 || desc.height == 0 || desc.stride == 0) {
        AHardwareBuffer_release(buffer);
        return DSAPI_HWBUF_ERR_SIZE;
    }
    if (desc.format != AHARDWAREBUFFER_FORMAT_R8G8B8A8_UNORM) {
        AHardwareBuffer_release(buffer);
        return DSAPI_HWBUF_ERR_FORMAT;
    }

    const native_handle_t* handle = AHardwareBuffer_getNativeHandle(buffer);
    if (handle == NULL || handle->numFds < 1) {
        AHardwareBuffer_release(buffer);
        return DSAPI_HWBUF_ERR_HANDLE;
    }

    int dup_fd = fcntl(handle->data[0], F_DUPFD_CLOEXEC, 0);
    if (dup_fd < 0) {
        AHardwareBuffer_release(buffer);
        return DSAPI_HWBUF_ERR_FD_DUP;
    }

    uint64_t row_bytes = (uint64_t)desc.stride * 4ULL;
    uint64_t total_bytes = row_bytes * (uint64_t)desc.height;
    if (total_bytes == 0 || total_bytes > UINT32_MAX) {
        close(dup_fd);
        AHardwareBuffer_release(buffer);
        return DSAPI_HWBUF_ERR_SIZE;
    }

    out_frame->fd = dup_fd;
    out_frame->frame_seq = frame_seq;
    out_frame->width = desc.width;
    out_frame->height = desc.height;
    out_frame->stride = desc.stride;
    out_frame->format = desc.format;
    out_frame->usage = desc.usage;
    out_frame->byte_offset = 0;
    out_frame->byte_len = (uint32_t)total_bytes;

    AHardwareBuffer_release(buffer);
    return 0;
}

void dsapi_jni_release_hwbuffer_frame(dsapi_jni_hwbuffer_frame_t* frame) {
    if (frame == NULL) {
        return;
    }
    if (frame->fd >= 0) {
        close(frame->fd);
    }
    memset(frame, 0, sizeof(*frame));
    frame->fd = -1;
}
