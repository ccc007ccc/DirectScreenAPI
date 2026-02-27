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

    dsapi_context_destroy(ctx);
    return 0;
}
