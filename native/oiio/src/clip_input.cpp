#include <OpenImageIO/imageio.h>
#include <OpenImageIO/filesystem.h>

#include "clip_capi.h"

#include <array>
#include <cstddef>
#include <cstdint>
#include <fstream>
#include <limits>
#include <string>
#include <vector>

OIIO_PLUGIN_NAMESPACE_BEGIN

namespace {

constexpr int kChannelCount = 4;
constexpr std::array<unsigned char, 8> kClipMagic = {
    'C', 'S', 'F', 'C', 'H', 'U', 'N', 'K'
};

bool
has_clip_magic(const std::string& filename)
{
    std::ifstream file(filename, std::ios::binary);
    if (!file)
        return false;

    std::array<unsigned char, kClipMagic.size()> header {};
    file.read(reinterpret_cast<char*>(header.data()),
              static_cast<std::streamsize>(header.size()));
    return file.gcount() == static_cast<std::streamsize>(header.size())
           && header == kClipMagic;
}

bool
has_clip_magic(Filesystem::IOProxy* proxy)
{
    if (!proxy)
        return false;

    std::array<unsigned char, kClipMagic.size()> header {};
    return proxy->pread(header.data(), header.size(), 0) == header.size()
           && header == kClipMagic;
}

bool
read_proxy_bytes(Filesystem::IOProxy* proxy, std::vector<std::uint8_t>& bytes)
{
    if (!proxy)
        return false;

    const std::size_t len = proxy->size();
    if (len < kClipMagic.size())
        return false;

    bytes.resize(len);
    return proxy->pread(bytes.data(), bytes.size(), 0) == bytes.size();
}

class ClipInput final : public ImageInput {
public:
    ClipInput() = default;
    ~ClipInput() override { close(); }

    const char* format_name() const override { return "clip"; }

    int supports(string_view feature) const override
    {
        if (feature == "ioproxy")
            return 1;
        return 0;
    }

    bool valid_file(const std::string& filename) const override
    {
        return has_clip_magic(filename);
    }

    bool valid_file(Filesystem::IOProxy* proxy) const override
    {
        return has_clip_magic(proxy);
    }

    bool open(const std::string& name, ImageSpec& spec) override
    {
        close();

        if (Filesystem::IOProxy* proxy = ioproxy()) {
            const std::string display_name = name.empty() ? "<IOProxy>" : name;
            return open_proxy_session(display_name, proxy, spec);
        }

        if (!has_clip_magic(name)) {
            errorfmt("{} is not a CLIP file", name);
            return false;
        }

        if (clip_renderer_abi_version() != CLIP_RENDERER_ABI_VERSION) {
            errorfmt("clip renderer ABI mismatch: adapter expects {}, library reports {}",
                     CLIP_RENDERER_ABI_VERSION, clip_renderer_abi_version());
            return false;
        }

        ClipRendererSession* session = nullptr;
        ClipRendererStatus status =
            clip_renderer_session_open(name.c_str(), &session);
        if (status != ClipRendererStatus_Ok || !session) {
            errorfmt("clip renderer open failed for {}: {}", name,
                     renderer_error());
            return false;
        }

        return open_session(name, session, spec);
    }

    bool open(const std::string& name, ImageSpec& spec,
              const ImageSpec& config) override
    {
        close();

        const std::string display_name = name.empty() ? "<IOProxy>" : name;
        Filesystem::IOProxy* proxy = ioproxy();
        const ParamValue* param = config.find_attribute("oiio:ioproxy",
                                                        TypeDesc::PTR);
        if (!proxy && param)
            proxy = param->get<Filesystem::IOProxy*>();
        if (!proxy)
            return open(name, spec);

        return open_proxy_session(display_name, proxy, spec);
    }

    bool close() override
    {
        if (m_session) {
            clip_renderer_session_close(m_session);
            m_session = nullptr;
        }
        m_opened = false;
        return true;
    }

    bool read_native_scanline(int subimage, int miplevel, int y, int z,
                              void* data) override
    {
        if (!m_opened || subimage != 0 || miplevel != 0 || z != 0
            || y < 0 || y >= m_spec.height || !m_session) {
            return false;
        }

        const auto out_len = static_cast<std::size_t>(m_spec.width)
                             * static_cast<std::size_t>(kChannelCount);
        ClipRendererStatus status = clip_renderer_session_read_rgba8(
            m_session, 0, static_cast<std::uint32_t>(y),
            static_cast<std::uint32_t>(m_spec.width), 1,
            static_cast<std::uint8_t*>(data), out_len);
        if (status != ClipRendererStatus_Ok) {
            errorfmt("clip renderer scanline read failed at y={}: {}",
                     y, renderer_error());
            return false;
        }

        return true;
    }

private:
    bool open_proxy_session(const std::string& display_name,
                            Filesystem::IOProxy* proxy, ImageSpec& spec)
    {
        if (!has_clip_magic(proxy)) {
            errorfmt("{} is not a CLIP file", display_name);
            return false;
        }

        if (clip_renderer_abi_version() != CLIP_RENDERER_ABI_VERSION) {
            errorfmt("clip renderer ABI mismatch: adapter expects {}, library reports {}",
                     CLIP_RENDERER_ABI_VERSION, clip_renderer_abi_version());
            return false;
        }

        std::vector<std::uint8_t> bytes;
        if (!read_proxy_bytes(proxy, bytes)) {
            errorfmt("could not read {} through IOProxy", display_name);
            return false;
        }

        ClipRendererSession* session = nullptr;
        ClipRendererStatus status = clip_renderer_session_open_memory(
            bytes.data(), bytes.size(), &session);
        if (status != ClipRendererStatus_Ok || !session) {
            errorfmt("clip renderer memory open failed for {}: {}",
                     display_name, renderer_error());
            return false;
        }

        return open_session(display_name, session, spec);
    }

    bool open_session(const std::string& name, ClipRendererSession* session,
                      ImageSpec& spec)
    {
        ClipRendererImageInfo info {};
        ClipRendererStatus status = clip_renderer_session_info(session, &info);
        if (status != ClipRendererStatus_Ok) {
            errorfmt("clip renderer metadata failed for {}: {}", name,
                     renderer_error());
            clip_renderer_session_close(session);
            return false;
        }

        if (info.width == 0 || info.height == 0
            || info.width > static_cast<std::uint32_t>(std::numeric_limits<int>::max())
            || info.height > static_cast<std::uint32_t>(std::numeric_limits<int>::max())) {
            errorfmt("clip renderer returned invalid dimensions {}x{}",
                     info.width, info.height);
            clip_renderer_session_close(session);
            return false;
        }

        m_spec.x = 0;
        m_spec.y = 0;
        m_spec.z = 0;
        m_spec.width = static_cast<int>(info.width);
        m_spec.height = static_cast<int>(info.height);
        m_spec.depth = 1;
        m_spec.full_x = 0;
        m_spec.full_y = 0;
        m_spec.full_z = 0;
        m_spec.full_width = static_cast<int>(info.width);
        m_spec.full_height = static_cast<int>(info.height);
        m_spec.full_depth = 1;
        m_spec.tile_width = 0;
        m_spec.tile_height = 0;
        m_spec.tile_depth = 0;
        m_spec.nchannels = kChannelCount;
        m_spec.format.basetype = TypeDesc::UINT8;
        m_spec.format.aggregate = TypeDesc::SCALAR;
        m_spec.format.vecsemantics = TypeDesc::NOSEMANTICS;
        m_spec.format.arraylen = 0;
        m_spec.channelformats.clear();
        m_spec.channelnames = { "R", "G", "B", "A" };
        m_spec.alpha_channel = 3;
        m_spec.z_channel = -1;
        m_spec.deep = false;
        m_spec.attribute("clip:root_layer_id",
                         static_cast<int>(info.root_layer_id));
        m_spec.attribute("clip:layer_count",
                         std::to_string(info.layer_count));
        m_spec.attribute("clip:external_data_count",
                         std::to_string(info.external_data_count));
        spec = m_spec;
        m_session = session;
        m_opened = true;
        return true;
    }

    static const char* renderer_error()
    {
        const char* message = clip_renderer_last_error();
        return message ? message : "unknown renderer error";
    }

    ClipRendererSession* m_session = nullptr;
    bool m_opened = false;
};

}  // namespace

OIIO_PLUGIN_EXPORTS_BEGIN

OIIO_EXPORT int clip_imageio_version = OIIO_PLUGIN_VERSION;

OIIO_EXPORT const char*
clip_imageio_library_version()
{
    return "rizum clip rust-abi placeholder 0.2";
}

OIIO_EXPORT ImageInput*
clip_input_imageio_create()
{
    return new ClipInput;
}

OIIO_EXPORT const char* clip_input_extensions[] = { "clip", nullptr };

OIIO_PLUGIN_EXPORTS_END

OIIO_PLUGIN_NAMESPACE_END
