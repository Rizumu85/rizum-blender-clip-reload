#ifndef RIZUM_CLIP_CAPI_H
#define RIZUM_CLIP_CAPI_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define CLIP_RENDERER_ABI_VERSION 1u

typedef struct ClipRendererSession ClipRendererSession;

typedef enum ClipRendererStatus {
    ClipRendererStatus_Ok = 0,
    ClipRendererStatus_NullArgument = 1,
    ClipRendererStatus_InvalidUtf8Path = 2,
    ClipRendererStatus_OpenFailed = 3,
    ClipRendererStatus_InvalidRegion = 4,
    ClipRendererStatus_ReadFailed = 5,
} ClipRendererStatus;

typedef struct ClipRendererImageInfo {
    uint32_t width;
    uint32_t height;
    uint32_t root_layer_id;
    size_t layer_count;
    size_t external_data_count;
} ClipRendererImageInfo;

uint32_t clip_renderer_abi_version(void);

const char* clip_renderer_last_error(void);

ClipRendererStatus clip_renderer_session_open(
    const char* path,
    ClipRendererSession** out_session);

void clip_renderer_session_close(ClipRendererSession* session);

ClipRendererStatus clip_renderer_session_info(
    const ClipRendererSession* session,
    ClipRendererImageInfo* out_info);

ClipRendererStatus clip_renderer_session_read_rgba8(
    ClipRendererSession* session,
    uint32_t x,
    uint32_t y,
    uint32_t width,
    uint32_t height,
    uint8_t* out_pixels,
    size_t out_len);

#ifdef __cplusplus
}
#endif

#endif
