#ifndef DIRECTSCREEN_API_H
#define DIRECTSCREEN_API_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define DSAPI_ABI_VERSION 0x00010000u

typedef struct dsapi_context dsapi_context_t;

typedef enum dsapi_status {
    DSAPI_OK = 0,
    DSAPI_NULL_POINTER = 1,
    DSAPI_INVALID_ARGUMENT = 2,
    DSAPI_OUT_OF_RANGE = 3,
    DSAPI_INTERNAL_ERROR = 4
} dsapi_status_t;

typedef enum dsapi_decision {
    DSAPI_PASS = 0,
    DSAPI_BLOCK = 1
} dsapi_decision_t;

typedef struct dsapi_display_state {
    uint32_t width;
    uint32_t height;
    float refresh_hz;
    uint32_t density_dpi;
    uint32_t rotation;
} dsapi_display_state_t;

typedef struct dsapi_route_result {
    int32_t decision;
    int32_t region_id;
} dsapi_route_result_t;

typedef struct dsapi_render_stats {
    uint64_t frame_seq;
    uint32_t draw_calls;
    uint32_t frost_passes;
    uint32_t text_calls;
} dsapi_render_stats_t;

typedef struct dsapi_render_frame_info {
    uint64_t frame_seq;
    uint32_t width;
    uint32_t height;
    uint32_t byte_len;
    uint32_t checksum_fnv1a32;
} dsapi_render_frame_info_t;

typedef struct dsapi_render_frame_chunk {
    uint64_t frame_seq;
    uint32_t total_bytes;
    uint32_t offset;
    uint32_t chunk_len;
} dsapi_render_frame_chunk_t;

typedef struct dsapi_render_present_info {
    uint64_t present_seq;
    uint64_t frame_seq;
    uint32_t width;
    uint32_t height;
    uint32_t byte_len;
    uint32_t checksum_fnv1a32;
} dsapi_render_present_info_t;

const char* dsapi_version(void);
uint32_t dsapi_abi_version(void);

dsapi_context_t* dsapi_context_create(void);
int32_t dsapi_context_create_with_abi(uint32_t abi_version, dsapi_context_t** out_ctx);
void dsapi_context_destroy(dsapi_context_t* ctx);

int32_t dsapi_set_default_decision(dsapi_context_t* ctx, int32_t decision);
int32_t dsapi_set_display_state(dsapi_context_t* ctx, const dsapi_display_state_t* display);
int32_t dsapi_get_display_state(dsapi_context_t* ctx, dsapi_display_state_t* out_display);

int32_t dsapi_region_clear(dsapi_context_t* ctx);
int32_t dsapi_region_add_rect(
    dsapi_context_t* ctx,
    int32_t region_id,
    int32_t decision,
    float x,
    float y,
    float w,
    float h
);

int32_t dsapi_route_point(
    dsapi_context_t* ctx,
    float x,
    float y,
    dsapi_route_result_t* out_result
);

int32_t dsapi_touch_down(
    dsapi_context_t* ctx,
    int32_t pointer_id,
    float x,
    float y,
    dsapi_route_result_t* out_result
);

int32_t dsapi_touch_move(
    dsapi_context_t* ctx,
    int32_t pointer_id,
    float x,
    float y,
    dsapi_route_result_t* out_result
);

int32_t dsapi_touch_up(
    dsapi_context_t* ctx,
    int32_t pointer_id,
    float x,
    float y,
    dsapi_route_result_t* out_result
);

int32_t dsapi_touch_cancel(
    dsapi_context_t* ctx,
    int32_t pointer_id,
    dsapi_route_result_t* out_result
);

int32_t dsapi_touch_clear(dsapi_context_t* ctx);
int32_t dsapi_touch_count(dsapi_context_t* ctx, uint32_t* out_count);

int32_t dsapi_render_submit_stats(
    dsapi_context_t* ctx,
    uint32_t draw_calls,
    uint32_t frost_passes,
    uint32_t text_calls,
    dsapi_render_stats_t* out_stats
);

int32_t dsapi_render_get_stats(dsapi_context_t* ctx, dsapi_render_stats_t* out_stats);

int32_t dsapi_render_submit_frame_rgba(
    dsapi_context_t* ctx,
    uint32_t width,
    uint32_t height,
    const uint8_t* pixels_rgba8,
    uint32_t pixels_len,
    dsapi_render_frame_info_t* out_info
);

int32_t dsapi_render_get_frame_info(
    dsapi_context_t* ctx,
    dsapi_render_frame_info_t* out_info
);

int32_t dsapi_render_clear_frame(dsapi_context_t* ctx);

int32_t dsapi_render_frame_read_chunk(
    dsapi_context_t* ctx,
    uint32_t offset,
    uint32_t max_bytes,
    dsapi_render_frame_chunk_t* out_chunk,
    uint8_t* out_bytes,
    uint32_t out_bytes_cap
);

int32_t dsapi_render_present(
    dsapi_context_t* ctx,
    dsapi_render_present_info_t* out_present
);

int32_t dsapi_render_get_present(
    dsapi_context_t* ctx,
    dsapi_render_present_info_t* out_present
);

#ifdef __cplusplus
}
#endif

#endif
