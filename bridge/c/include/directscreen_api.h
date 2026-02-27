#ifndef DIRECTSCREEN_API_H
#define DIRECTSCREEN_API_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

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

const char* dsapi_version(void);

dsapi_context_t* dsapi_context_create(void);
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

#ifdef __cplusplus
}
#endif

#endif
