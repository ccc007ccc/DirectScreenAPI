# C ABI (v0.1)

Header: `bridge/c/include/directscreen_api.h`

## Context Lifecycle

- `dsapi_context_create`
- `dsapi_context_destroy`

## Display APIs

- `dsapi_set_display_state`
- `dsapi_get_display_state`

## Routing APIs

- `dsapi_set_default_decision`
- `dsapi_region_clear`
- `dsapi_region_add_rect`
- `dsapi_route_point`

## Status Codes

- `0`: OK
- `1`: NULL_POINTER
- `2`: INVALID_ARGUMENT
- `3`: OUT_OF_RANGE
- `4`: INTERNAL_ERROR

## ABI Policy

- Existing symbols are never changed in-place.
- Breaking changes require a new major ABI version namespace.
