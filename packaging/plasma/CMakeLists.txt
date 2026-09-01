cmake_minimum_required(VERSION 3.21)
project(skwd_paper_plasma LANGUAGES CXX)

set(CMAKE_AUTOMOC ON)
set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

include(GNUInstallDirs)
find_package(Qt6 REQUIRED COMPONENTS Core Gui OpenGL Qml Quick)
find_package(PkgConfig REQUIRED)
pkg_check_modules(LIBDRM REQUIRED IMPORTED_TARGET libdrm)

add_library(skwdwallpaperplugin MODULE
    src/plugin.cpp
    src/skwdvideoitem.cpp
    src/skwdvideoitem.h
)

target_link_libraries(skwdwallpaperplugin PRIVATE Qt6::Core Qt6::Gui Qt6::OpenGL Qt6::Qml Qt6::Quick EGL PkgConfig::LIBDRM)

set(SKWD_QML_DIR "${CMAKE_INSTALL_LIBDIR}/qt6/qml/org/skwd/wallpaper")
set(SKWD_LICENSE_DIR "${CMAKE_INSTALL_DATADIR}/licenses/skwd-paper-plasma")
set(SKWD_PAPER_ROOT "${CMAKE_CURRENT_SOURCE_DIR}/../..")
install(TARGETS skwdwallpaperplugin LIBRARY DESTINATION "${SKWD_QML_DIR}")
install(FILES qmldir DESTINATION "${SKWD_QML_DIR}")
install(DIRECTORY wallpaper/ DESTINATION "${CMAKE_INSTALL_DATADIR}/plasma/wallpapers/org.skwd.wall.plasma")
install(FILES "${SKWD_PAPER_ROOT}/LICENSE" DESTINATION "${SKWD_LICENSE_DIR}")
install(DIRECTORY "${SKWD_PAPER_ROOT}/LICENSES/" DESTINATION "${SKWD_LICENSE_DIR}/LICENSES")
