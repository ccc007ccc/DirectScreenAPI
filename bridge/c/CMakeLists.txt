cmake_minimum_required(VERSION 3.16)
project(directscreen_c_examples C)

set(CMAKE_C_STANDARD 11)
set(CMAKE_C_STANDARD_REQUIRED ON)

add_executable(dsapi_example
    examples/simple_route.c
)

target_include_directories(dsapi_example PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}/include
)

target_link_directories(dsapi_example PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}/../../target/release
)

target_link_libraries(dsapi_example PRIVATE
    directscreen_core
    dl
    pthread
    m
)
