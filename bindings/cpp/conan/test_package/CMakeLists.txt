# Consumers of the package, in C++ and C. Neither asks for C++17: the
# package's target does.
cmake_minimum_required(VERSION 3.21)
project(test_package LANGUAGES C CXX)

find_package(cleanrules CONFIG REQUIRED)

add_executable(test_package test_package.cpp)
target_link_libraries(test_package PRIVATE cleanrules::cleanrules)

add_executable(test_package_c test_package.c)
target_link_libraries(test_package_c PRIVATE cleanrules::c)
