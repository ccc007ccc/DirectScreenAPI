#include "dsapi_hwbuffer_bridge.h"

#include <android/hardware_buffer.h>
#include <android/hardware_buffer_jni.h>
#include <fcntl.h>
#include <stddef.h>
#include <sys/socket.h>
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
#define DSAPI_HWBUF_ERR_SOCKET (-6)

static int recv_first_fd_from_socket(int socket_fd, int* out_fd) {
    if (out_fd == NULL) {
        return DSAPI_HWBUF_ERR_NULL;
    }
    *out_fd = -1;

    char marker = 0;
    struct iovec iov;
    iov.iov_base = &marker;
    iov.iov_len = sizeof(marker);

    char control[CMSG_SPACE(sizeof(int) * 4)];
    memset(control, 0, sizeof(control));

    struct msghdr msg;
    memset(&msg, 0, sizeof(msg));
    msg.msg_iov = &iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control;
    msg.msg_controllen = sizeof(control);

    ssize_t recv_rc = recvmsg(socket_fd, &msg, 0);
    if (recv_rc <= 0) {
        return DSAPI_HWBUF_ERR_SOCKET;
    }

    for (struct cmsghdr* cmsg = CMSG_FIRSTHDR(&msg);
         cmsg != NULL;
         cmsg = CMSG_NXTHDR(&msg, cmsg)) {
        if (cmsg->cmsg_level != SOL_SOCKET || cmsg->cmsg_type != SCM_RIGHTS) {
            continue;
        }
        int* fds = (int*)CMSG_DATA(cmsg);
        size_t fd_count = (size_t)(cmsg->cmsg_len - CMSG_LEN(0)) / sizeof(int);
        if (fd_count < 1) {
            continue;
        }
        int raw_fd = fds[0];
        int dup_fd = fcntl(raw_fd, F_DUPFD_CLOEXEC, 0);
        if (dup_fd < 0) {
            return DSAPI_HWBUF_ERR_FD_DUP;
        }
        *out_fd = dup_fd;
        return 0;
    }

    return DSAPI_HWBUF_ERR_HANDLE;
}

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

    int socket_pair[2] = {-1, -1};
    if (socketpair(AF_UNIX, SOCK_STREAM, 0, socket_pair) != 0) {
        AHardwareBuffer_release(buffer);
        return DSAPI_HWBUF_ERR_SOCKET;
    }
    if (AHardwareBuffer_sendHandleToUnixSocket(buffer, socket_pair[0]) != 0) {
        close(socket_pair[0]);
        close(socket_pair[1]);
        AHardwareBuffer_release(buffer);
        return DSAPI_HWBUF_ERR_SOCKET;
    }
    int dup_fd = -1;
    int recv_rc = recv_first_fd_from_socket(socket_pair[1], &dup_fd);
    close(socket_pair[0]);
    close(socket_pair[1]);
    if (recv_rc != 0 || dup_fd < 0) {
        AHardwareBuffer_release(buffer);
        return recv_rc != 0 ? recv_rc : DSAPI_HWBUF_ERR_HANDLE;
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

static jfieldID resolve_file_descriptor_field(JNIEnv* env, jclass fd_class) {
    jfieldID field = (*env)->GetFieldID(env, fd_class, "descriptor", "I");
    if (field != NULL) {
        return field;
    }
    (*env)->ExceptionClear(env);
    field = (*env)->GetFieldID(env, fd_class, "fd", "I");
    if (field != NULL) {
        return field;
    }
    (*env)->ExceptionClear(env);
    return NULL;
}

JNIEXPORT jobject JNICALL Java_org_directscreenapi_adapter_HardwareBufferBridge_nativeExportFrame(
    JNIEnv* env,
    jclass clazz,
    jobject hardware_buffer_obj,
    jlong frame_seq
) {
    (void)clazz;
    dsapi_jni_hwbuffer_frame_t frame;
    int rc = dsapi_jni_export_hwbuffer_frame(
        env,
        hardware_buffer_obj,
        (uint64_t)frame_seq,
        &frame
    );
    if (rc != 0 || frame.fd < 0) {
        return NULL;
    }

    jclass fd_class = (*env)->FindClass(env, "java/io/FileDescriptor");
    if (fd_class == NULL) {
        close(frame.fd);
        return NULL;
    }
    jmethodID fd_ctor = (*env)->GetMethodID(env, fd_class, "<init>", "()V");
    if (fd_ctor == NULL) {
        close(frame.fd);
        return NULL;
    }
    jobject fd_obj = (*env)->NewObject(env, fd_class, fd_ctor);
    if (fd_obj == NULL) {
        close(frame.fd);
        return NULL;
    }
    jfieldID fd_field = resolve_file_descriptor_field(env, fd_class);
    if (fd_field == NULL) {
        close(frame.fd);
        return NULL;
    }
    (*env)->SetIntField(env, fd_obj, fd_field, frame.fd);

    jclass native_frame_class = (*env)->FindClass(
        env,
        "org/directscreenapi/adapter/HardwareBufferBridge$NativeFrame"
    );
    if (native_frame_class == NULL) {
        close(frame.fd);
        return NULL;
    }
    jmethodID frame_ctor = (*env)->GetMethodID(
        env,
        native_frame_class,
        "<init>",
        "(Ljava/io/FileDescriptor;JIIIIJII)V"
    );
    if (frame_ctor == NULL) {
        close(frame.fd);
        return NULL;
    }

    jobject out = (*env)->NewObject(
        env,
        native_frame_class,
        frame_ctor,
        fd_obj,
        (jlong)frame.frame_seq,
        (jint)frame.width,
        (jint)frame.height,
        (jint)frame.stride,
        (jint)frame.format,
        (jlong)frame.usage,
        (jint)frame.byte_offset,
        (jint)frame.byte_len
    );
    if (out == NULL) {
        close(frame.fd);
        return NULL;
    }
    return out;
}

JNIEXPORT void JNICALL Java_org_directscreenapi_adapter_HardwareBufferBridge_nativeCloseFd(
    JNIEnv* env,
    jclass clazz,
    jobject fd_obj
) {
    (void)clazz;
    if (env == NULL || fd_obj == NULL) {
        return;
    }
    jclass fd_class = (*env)->FindClass(env, "java/io/FileDescriptor");
    if (fd_class == NULL) {
        return;
    }
    jfieldID fd_field = resolve_file_descriptor_field(env, fd_class);
    if (fd_field == NULL) {
        return;
    }
    jint fd = (*env)->GetIntField(env, fd_obj, fd_field);
    if (fd >= 0) {
        close((int)fd);
        (*env)->SetIntField(env, fd_obj, fd_field, -1);
    }
}
