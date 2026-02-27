#include <stdio.h>

#include "directscreen_api.h"

int main(void) {
    dsapi_context_t* ctx = dsapi_context_create();
    if (!ctx) {
        fprintf(stderr, "failed_to_create_context\n");
        return 1;
    }

    printf("version=%s\n", dsapi_version());

    dsapi_display_state_t display = {
        .width = 1440,
        .height = 3168,
        .refresh_hz = 120.0f,
        .density_dpi = 640,
        .rotation = 0,
    };

    if (dsapi_set_display_state(ctx, &display) != DSAPI_OK) {
        fprintf(stderr, "set_display_failed\n");
        dsapi_context_destroy(ctx);
        return 2;
    }

    dsapi_region_clear(ctx);
    dsapi_region_add_rect(ctx, 101, DSAPI_BLOCK, 100.0f, 100.0f, 300.0f, 300.0f);

    dsapi_route_result_t result = {0};
    dsapi_route_point(ctx, 120.0f, 180.0f, &result);
    printf("route_decision=%d region_id=%d\n", result.decision, result.region_id);

    dsapi_render_stats_t stats = {0};
    if (dsapi_render_submit_stats(ctx, 12, 2, 3, &stats) != DSAPI_OK) {
        fprintf(stderr, "render_submit_stats_failed\n");
        dsapi_context_destroy(ctx);
        return 3;
    }
    printf(
        "render_stats frame_seq=%llu draw_calls=%u frost_passes=%u text_calls=%u\n",
        (unsigned long long)stats.frame_seq,
        stats.draw_calls,
        stats.frost_passes,
        stats.text_calls
    );

    uint8_t pixels[4] = {255, 0, 0, 255};
    dsapi_render_frame_info_t frame_info = {0};
    if (dsapi_render_submit_frame_rgba(ctx, 1, 1, pixels, 4, &frame_info) != DSAPI_OK) {
        fprintf(stderr, "render_submit_frame_failed\n");
        dsapi_context_destroy(ctx);
        return 4;
    }
    printf(
        "render_frame frame_seq=%llu bytes=%u checksum=%u\n",
        (unsigned long long)frame_info.frame_seq,
        frame_info.byte_len,
        frame_info.checksum_fnv1a32
    );

    dsapi_render_frame_chunk_t chunk = {0};
    uint8_t chunk_buf[4] = {0};
    if (dsapi_render_frame_read_chunk(ctx, 0, 4, &chunk, chunk_buf, 4) != DSAPI_OK) {
        fprintf(stderr, "render_read_chunk_failed\n");
        dsapi_context_destroy(ctx);
        return 5;
    }
    printf(
        "render_chunk frame_seq=%llu total=%u offset=%u chunk_len=%u\n",
        (unsigned long long)chunk.frame_seq,
        chunk.total_bytes,
        chunk.offset,
        chunk.chunk_len
    );

    dsapi_render_present_info_t present = {0};
    if (dsapi_render_present(ctx, &present) != DSAPI_OK) {
        fprintf(stderr, "render_present_failed\n");
        dsapi_context_destroy(ctx);
        return 6;
    }
    printf(
        "render_present present_seq=%llu frame_seq=%llu bytes=%u checksum=%u\n",
        (unsigned long long)present.present_seq,
        (unsigned long long)present.frame_seq,
        present.byte_len,
        present.checksum_fnv1a32
    );

    dsapi_context_destroy(ctx);
    return 0;
}
