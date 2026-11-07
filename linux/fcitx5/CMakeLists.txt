cmake_minimum_required(VERSION 3.16)
project(voicekey-fcitx5 VERSION 1.9.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_SKIP_RPATH ON)

find_package(Fcitx5Core REQUIRED)
find_package(Fcitx5Utils REQUIRED)

add_library(voicekey MODULE src/voicekey.cpp)
target_link_libraries(voicekey PRIVATE Fcitx5::Core Fcitx5::Utils)
target_compile_options(voicekey PRIVATE -Wall -Wextra)
set_target_properties(voicekey PROPERTIES PREFIX "" OUTPUT_NAME "voicekey")

install(TARGETS voicekey LIBRARY DESTINATION lib/x86_64-linux-gnu/fcitx5)
install(FILES data/voicekey.conf DESTINATION share/fcitx5/addon)
