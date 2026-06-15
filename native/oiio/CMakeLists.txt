cmake_minimum_required(VERSION 3.24)

project(clip_oiio_plugin LANGUAGES CXX)

find_package(OpenImageIO CONFIG REQUIRED)

add_library(clip_imageio MODULE src/clip_input.cpp)
set(CLIP_CAPI_INCLUDE_DIR
    "${CMAKE_CURRENT_SOURCE_DIR}/../rust/crates/clip_capi/include"
    CACHE PATH
    "Directory containing clip_capi.h"
)
set(CLIP_CAPI_LIBRARY
    ""
    CACHE FILEPATH
    "Rust clip_capi import or static library"
)

target_include_directories(clip_imageio PRIVATE "${CLIP_CAPI_INCLUDE_DIR}")
target_link_libraries(clip_imageio PRIVATE OpenImageIO::OpenImageIO)
if(CLIP_CAPI_LIBRARY)
    target_link_libraries(clip_imageio PRIVATE "${CLIP_CAPI_LIBRARY}")
endif()
target_compile_features(clip_imageio PRIVATE cxx_std_20)
set_target_properties(
    clip_imageio
    PROPERTIES
        PREFIX ""
        OUTPUT_NAME "clip.imageio"
)

option(CLIP_OIIO_BUILD_PROBE "Build a tiny OpenImageIO plugin probe executable" ON)

if(CLIP_OIIO_BUILD_PROBE)
    add_executable(clip_oiio_probe tools/probe_open.cpp)
    target_link_libraries(clip_oiio_probe PRIVATE OpenImageIO::OpenImageIO)
    target_compile_features(clip_oiio_probe PRIVATE cxx_std_20)
endif()
