// A small check macro for the tests: no framework, so no dependency.

#ifndef CLEANRULES_TESTS_CHECK_HPP
#define CLEANRULES_TESTS_CHECK_HPP

#include <cstdio>
#include <cstdlib>

namespace check {

inline int &failures() {
    static int count = 0;
    return count;
}

inline void fail(const char *file, int line, const char *what) {
    std::fprintf(stderr, "%s:%d: check failed: %s\n", file, line, what);
    ++failures();
}

// The exit status of a test program: 0 when every check passed.
inline int report(const char *name) {
    if (failures() == 0) {
        std::printf("%s: all checks passed\n", name);
        return EXIT_SUCCESS;
    }
    std::fprintf(stderr, "%s: %d checks failed\n", name, failures());
    return EXIT_FAILURE;
}

}  // namespace check

#define CHECK(condition)                                    \
    do {                                                    \
        if (!(condition)) {                                 \
            ::check::fail(__FILE__, __LINE__, #condition);  \
        }                                                   \
    } while (false)

#endif  // CLEANRULES_TESTS_CHECK_HPP
