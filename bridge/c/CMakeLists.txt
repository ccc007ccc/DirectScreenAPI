cmake_minimum_required(VERSION 3.16)
project(directscreen_c_examples C)

set(CMAKE_C_STANDARD 11)
set(CMAKE_C_STANDARD_REQUIRED ON)

if(DEFINED ENV{CARGO_TARGET_DIR} AND NOT "$ENV{CARGO_TARGET_DIR}" STREQUAL "")
    set(DSAPI_CARGO_TARGET_DIR "$ENV{CARGO_TARGET_DIR}")
else()
    set(DSAPI_CARGO_TARGET_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../../target")
endif()

add_executable(dsapi_example
    examples/simple_route.c
)

target_include_directories(dsapi_example PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}/include
)

target_link_directories(dsapi_example PRIVATE
    ${DSAPI_CARGO_TARGET_DIR}/release
)

target_link_libraries(dsapi_example PRIVATE
    directscreen_core
    dl
    pthread
    m
)
