# cleanrules from this repository's checkout: an overlay port
# (vcpkg install cleanrules --overlay-ports=bindings/cpp/vcpkg/ports). The port
# in the vcpkg registry will take a release with vcpkg_from_github instead.
#
# The sources are the checkout the port lives in, so vcpkg's binary cache does
# not see them change (nor rustc's version): remove the port and install with
# --binarysource=clear after changing them.
get_filename_component(SOURCE_PATH "${CURRENT_PORT_DIR}/../../../../.." ABSOLUTE)
if(NOT EXISTS "${SOURCE_PATH}/crates/ffi/Cargo.toml")
  message(FATAL_ERROR "The cleanrules overlay port must stay in bindings/cpp/vcpkg/ports")
endif()

if(VCPKG_LIBRARY_LINKAGE STREQUAL "dynamic")
  set(shared ON)
else()
  set(shared OFF)
endif()

# The Rust target of the triplet, always explicit (vcpkg.json "supports" lists
# the ones mapped here).
set(arch_x64 x86_64)
set(arch_x86 i686)
set(arch_arm64 aarch64)
set(rust_arch "${arch_${VCPKG_TARGET_ARCHITECTURE}}")
if(VCPKG_TARGET_IS_WINDOWS AND NOT VCPKG_TARGET_IS_MINGW AND NOT VCPKG_TARGET_IS_UWP AND rust_arch)
  set(rust_target "${rust_arch}-pc-windows-msvc")
elseif(VCPKG_TARGET_IS_LINUX AND VCPKG_TARGET_ARCHITECTURE MATCHES "^(x64|arm64)$")
  set(rust_target "${rust_arch}-unknown-linux-gnu")
elseif(VCPKG_TARGET_IS_OSX AND VCPKG_TARGET_ARCHITECTURE MATCHES "^(x64|arm64)$")
  set(rust_target "${rust_arch}-apple-darwin")
else()
  message(FATAL_ERROR "cleanrules has no Rust target for ${TARGET_TRIPLET}")
endif()

# MSVC's C runtime as the triplet asks; Rust uses the release one in Debug too.
set(static_crt OFF)
set(crt_option "")
if(VCPKG_TARGET_IS_WINDOWS)
  if(VCPKG_CRT_LINKAGE STREQUAL "static")
    set(static_crt ON)
  endif()
  set(VCPKG_POLICY_ONLY_RELEASE_CRT enabled)
  set(crt_option "-DCLEANRULES_MSVC_STATIC_CRT=${static_crt}")
endif()

# bindings/cpp builds the C library with cargo, which the build machine needs.
vcpkg_cmake_configure(
  SOURCE_PATH "${SOURCE_PATH}/bindings/cpp"
  OPTIONS
    -DCLEANRULES_SHARED=${shared}
    -DCLEANRULES_BUILD_TESTS=OFF
    -DCLEANRULES_INSTALL=ON
    "-DCLEANRULES_RUST_TARGET=${rust_target}"
    ${crt_option}
    "-DCLEANRULES_CARGO_TARGET_DIR=${CURRENT_BUILDTREES_DIR}/cargo-${TARGET_TRIPLET}"
)
vcpkg_cmake_install()
vcpkg_cmake_config_fixup(CONFIG_PATH lib/cmake/cleanrules)

file(REMOVE_RECURSE
  "${CURRENT_PACKAGES_DIR}/debug/include"
  "${CURRENT_PACKAGES_DIR}/debug/share"
  # In the copyright file below.
  "${CURRENT_PACKAGES_DIR}/share/cleanrules/LICENSE"
  "${CURRENT_PACKAGES_DIR}/share/cleanrules/NOTICE"
)

file(INSTALL "${CMAKE_CURRENT_LIST_DIR}/usage" DESTINATION "${CURRENT_PACKAGES_DIR}/share/${PORT}")
vcpkg_install_copyright(FILE_LIST "${SOURCE_PATH}/LICENSE" "${SOURCE_PATH}/NOTICE")
