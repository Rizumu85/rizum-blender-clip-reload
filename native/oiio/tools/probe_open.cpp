#include <OpenImageIO/imageio.h>
#include <OpenImageIO/filesystem.h>

#include <cstdint>
#include <fstream>
#include <iostream>
#include <memory>
#include <string>
#include <vector>

namespace {

std::vector<std::uint8_t>
read_file_bytes(const std::string& filename)
{
    std::ifstream file(filename, std::ios::binary | std::ios::ate);
    if (!file)
        return {};

    const std::streamoff size = file.tellg();
    if (size <= 0)
        return {};

    std::vector<std::uint8_t> bytes(static_cast<std::size_t>(size));
    file.seekg(0, std::ios::beg);
    file.read(reinterpret_cast<char*>(bytes.data()),
              static_cast<std::streamsize>(bytes.size()));
    if (!file)
        return {};
    return bytes;
}

}  // namespace

int
main(int argc, char** argv)
{
    if (argc != 3 && argc != 4) {
        std::cerr << "usage: clip_oiio_probe [--memory] <plugin-dir> <file.clip>\n";
        return 2;
    }

    const bool memory_mode = std::string(argv[1]) == "--memory";
    if (argc == 4 && !memory_mode) {
        std::cerr << "unknown option: " << argv[1] << "\n";
        return 2;
    }

    const std::string plugin_dir = memory_mode ? argv[2] : argv[1];
    const std::string filename = memory_mode ? argv[3] : argv[2];
    OIIO::attribute("plugin_searchpath", plugin_dir);

    std::vector<std::uint8_t> bytes;
    std::unique_ptr<OIIO::Filesystem::IOMemReader> reader;
    if (memory_mode) {
        bytes = read_file_bytes(filename);
        if (bytes.empty()) {
            std::cerr << "failed to read input bytes: " << filename << "\n";
            return 1;
        }
        reader = std::make_unique<OIIO::Filesystem::IOMemReader>(
            bytes.data(), bytes.size());
    }

    std::unique_ptr<OIIO::ImageInput> input(
        OIIO::ImageInput::open(filename, nullptr, reader.get()));
    if (!input) {
        std::cerr << "open failed: " << OIIO::geterror() << "\n";
        return 1;
    }

    const OIIO::ImageSpec& spec = input->spec();
    std::vector<std::uint8_t> pixels(
        static_cast<std::size_t>(spec.width) * static_cast<std::size_t>(spec.height)
        * static_cast<std::size_t>(spec.nchannels));

    if (!input->read_image(0, 0, 0, spec.nchannels, spec.format, pixels.data())) {
        std::cerr << "read failed: " << input->geterror() << "\n";
        input->close();
        return 1;
    }

    if (!input->close()) {
        std::cerr << "close failed: " << input->geterror() << "\n";
        return 1;
    }

    std::cout << "opened clip image"
              << (memory_mode ? " from memory: " : ": ") << spec.width << "x"
              << spec.height << " channels=" << spec.nchannels
              << " first_pixel=["
              << static_cast<int>(pixels[0]) << ","
              << static_cast<int>(pixels[1]) << ","
              << static_cast<int>(pixels[2]) << ","
              << static_cast<int>(pixels[3]) << "]\n";
    return 0;
}
