#include <OpenImageIO/imageio.h>

#include <cstdint>
#include <iostream>
#include <memory>
#include <string>
#include <vector>

int
main(int argc, char** argv)
{
    if (argc != 3) {
        std::cerr << "usage: clip_oiio_probe <plugin-dir> <file.clip>\n";
        return 2;
    }

    const std::string plugin_dir = argv[1];
    const std::string filename = argv[2];
    OIIO::attribute("plugin_searchpath", plugin_dir);

    std::unique_ptr<OIIO::ImageInput> input(OIIO::ImageInput::open(filename));
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

    std::cout << "opened clip image: " << spec.width << "x" << spec.height
              << " channels=" << spec.nchannels << " first_pixel=["
              << static_cast<int>(pixels[0]) << ","
              << static_cast<int>(pixels[1]) << ","
              << static_cast<int>(pixels[2]) << ","
              << static_cast<int>(pixels[3]) << "]\n";
    return 0;
}
